//! Safe vault I/O: scanning, creating, and updating task files.
//!
//! Three actors write this vault concurrently (Obsidian via sync, the
//! recurring generator, and this library), so every write here is defensive:
//! updates go through temp-file + rename (never a partial file a syncing
//! client could pick up), creates reserve their filename with `create_new`
//! (never clobber), ids are allocated by scanning *at write time* (never a
//! cached max), and updates re-read the file first (never clobber a change
//! synced in since load).

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;

use super::config::{Config, IdStrategy};
use super::model::{ParseOutcome, Task};
use crate::task::slugify;

/// Entropy source for id generation, injectable so tests are deterministic.
pub trait Entropy {
    /// Milliseconds since the Unix epoch (for `ulid`).
    fn now_ms(&mut self) -> u64;
    /// A fresh pseudo-random value (for `random` and the `ulid` tail).
    fn rand_u64(&mut self) -> u64;
}

/// Production entropy: system clock plus a splitmix64 stream seeded from the
/// clock and pid. Not cryptographic; task ids only need to avoid collisions,
/// and collisions are re-checked against disk anyway.
pub struct SystemEntropy {
    state: u64,
}

impl Default for SystemEntropy {
    fn default() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        SystemEntropy {
            state: now ^ ((std::process::id() as u64) << 32),
        }
    }
}

impl Entropy for SystemEntropy {
    fn now_ms(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn rand_u64(&mut self) -> u64 {
        // splitmix64: tiny, well distributed, no dependency.
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
}

/// A file inside the tasks dir that carries task-like frontmatter but is
/// broken (malformed YAML, missing id/title). Skipped by consumers, reported
/// by `validate`.
#[derive(Debug, Clone, PartialEq)]
pub struct InvalidFile {
    pub rel_path: PathBuf,
    pub reason: String,
}

/// The result of scanning a vault's tasks dir.
#[derive(Debug, Default, PartialEq)]
pub struct Scan {
    pub tasks: Vec<Task>,
    pub invalid: Vec<InvalidFile>,
}

impl Scan {
    pub fn find(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id() == id)
    }
}

/// A taskmd project rooted at `root`, with its config loaded.
#[derive(Debug, Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub config: Config,
}

impl Vault {
    /// Open a vault: load `.taskmd.yaml` (defaults when absent, loud error
    /// when malformed).
    pub fn open(root: &Path) -> Result<Vault> {
        Ok(Vault {
            root: root.to_path_buf(),
            config: Config::load(root)?,
        })
    }

    pub fn tasks_dir(&self) -> PathBuf {
        self.config.tasks_dir(&self.root)
    }

    /// Scan the tasks dir recursively. Subdirectories become the dir-derived
    /// group (immediate parent dir name); dot-files and dot-dirs are skipped
    /// (hidden files, `.taskmd/`, sync droppings); non-task `.md` files are
    /// silently ignored; task-like-but-broken files are collected in
    /// [`Scan::invalid`]. A missing tasks dir scans as empty.
    pub fn scan(&self) -> Result<Scan> {
        let mut scan = Scan::default();
        let dir = self.tasks_dir();
        if !dir.exists() {
            return Ok(scan);
        }
        self.scan_into(&dir, &dir, &mut scan)?;
        // Deterministic order: by relative path.
        scan.tasks.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        scan.invalid.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        Ok(scan)
    }

    fn scan_into(&self, base: &Path, dir: &Path, scan: &mut Scan) -> Result<()> {
        for entry in fs::read_dir(dir)
            .with_context(|| format!("reading tasks dir {}", dir.display()))?
            .flatten()
        {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                self.scan_into(base, &path, scan)?;
                continue;
            }
            if !name.ends_with(".md") {
                continue;
            }
            let content =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let rel = path.strip_prefix(base).expect("under base").to_path_buf();
            match Task::parse(&content) {
                ParseOutcome::Task(mut t) => {
                    t.dir_group = rel
                        .parent()
                        .filter(|p| !p.as_os_str().is_empty())
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned());
                    t.rel_path = Some(rel);
                    scan.tasks.push(t);
                }
                ParseOutcome::NotATask => {}
                ParseOutcome::Invalid(reason) => scan.invalid.push(InvalidFile {
                    rel_path: rel,
                    reason,
                }),
            }
        }
        Ok(())
    }

    /// Find one task by id via a fresh scan. Errors when the id is ambiguous
    /// (duplicate ids are a vault defect `validate` reports; mutating an
    /// arbitrary one of them would be worse than refusing).
    pub fn find(&self, id: &str) -> Result<Task> {
        let scan = self.scan()?;
        let mut matches: Vec<&Task> = scan.tasks.iter().filter(|t| t.id() == id).collect();
        match matches.len() {
            0 => bail!("no task with id `{id}`"),
            1 => Ok(matches.remove(0).clone()),
            n => bail!("id `{id}` is ambiguous: {n} files carry it (run validate)"),
        }
    }

    /// Allocate a fresh id per the configured strategy, against the ids
    /// currently on disk. Sequential/prefixed take max+1; random/ulid retry on
    /// collision.
    pub fn allocate_id(&self, existing: &HashSet<String>, entropy: &mut dyn Entropy) -> String {
        let cfg = &self.config.id;
        match cfg.strategy {
            IdStrategy::Sequential => {
                let next = existing
                    .iter()
                    .filter_map(|id| id.parse::<u64>().ok())
                    .max()
                    .unwrap_or(0)
                    + 1;
                format!("{next:0width$}", width = cfg.padding)
            }
            IdStrategy::Prefixed => {
                // taskmd 0.2.5 emits `<prefix><NNN>` with no separator.
                let next = existing
                    .iter()
                    .filter_map(|id| id.strip_prefix(&cfg.prefix))
                    .filter_map(|rest| rest.parse::<u64>().ok())
                    .max()
                    .unwrap_or(0)
                    + 1;
                format!("{}{next:0width$}", cfg.prefix, width = cfg.padding)
            }
            IdStrategy::Random => loop {
                let id = random_id(cfg.length, entropy);
                if !existing.contains(&id) {
                    return id;
                }
            },
            IdStrategy::Ulid => {
                let mut bump = 0;
                loop {
                    let id = ulid_id(cfg.length, entropy.now_ms() + bump, entropy);
                    if !existing.contains(&id) {
                        return id;
                    }
                    // A short configured length can truncate away the random
                    // tail; bumping the timestamp guarantees progress.
                    bump += 1;
                }
            }
        }
    }

    /// Create a new task file: allocate an id at write time, reserve the
    /// filename with `create_new` (never clobber), and write the full content.
    /// A filename collision (concurrent writer, or a file the scanner cannot
    /// see as a task) marks that id as taken locally and retries with the
    /// next one, bounded.
    ///
    /// Known limitation shared with taskmd itself: two writers creating
    /// *different* titles in the same instant can still race the same id into
    /// two filenames; `validate` detects the duplicate.
    pub fn create(
        &self,
        title: &str,
        today: NaiveDate,
        entropy: &mut dyn Entropy,
        build: &dyn Fn(&mut Task),
    ) -> Result<Task> {
        let dir = self.tasks_dir();
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let mut taken: HashSet<String> = self.scan()?.tasks.iter().map(|t| t.id()).collect();
        for _ in 0..16 {
            let id = self.allocate_id(&taken, entropy);
            let mut task = Task::new(&id, title, today);
            build(&mut task);
            let filename = format!("{id}-{}.md", slugify(title));
            let path = dir.join(&filename);
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    // Plain `?`: a write failure on a just-created fd carries
                    // enough context in the io error itself.
                    f.write_all(task.to_markdown().as_bytes())?;
                    task.rel_path = Some(PathBuf::from(filename));
                    return Ok(task);
                }
                // The filename exists (a concurrent writer won the id, or a
                // non-task file occupies it): burn the id and try the next.
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    taken.insert(id);
                    continue;
                }
                Err(e) => {
                    return Err(e).with_context(|| format!("creating {}", path.display()));
                }
            }
        }
        bail!("could not allocate a free task id after 16 attempts")
    }

    /// Update a task by id: re-read it fresh (so an edit synced in since the
    /// caller last looked is not clobbered), apply `mutate`, and write back
    /// atomically (temp + rename).
    pub fn update(
        &self,
        id: &str,
        mutate: &mut dyn FnMut(&mut Task) -> Result<()>,
    ) -> Result<Task> {
        let mut task = self.find(id)?;
        mutate(&mut task)?;
        self.save(&task)?;
        Ok(task)
    }

    /// Atomically overwrite a loaded task's file (temp + rename in the same
    /// dir; the dot-prefixed temp name is invisible to the scanner and to
    /// Obsidian sync).
    pub fn save(&self, task: &Task) -> Result<()> {
        let rel = task
            .rel_path
            .as_ref()
            .context("task has no file path; use create for new tasks")?;
        let path = self.tasks_dir().join(rel);
        let dir = path.parent().context("task path has no parent")?;
        let tmp = dir.join(format!(".karamd-tmp-{}", std::process::id()));
        fs::write(&tmp, task.to_markdown())
            .with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("renaming into {}", path.display()))?;
        Ok(())
    }
}

/// `length` chars of lowercase base36, matching taskmd's random ids.
fn random_id(length: usize, entropy: &mut dyn Entropy) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut out = String::with_capacity(length);
    let mut bits = entropy.rand_u64();
    let mut used = 0;
    for _ in 0..length {
        if used == 10 {
            // 36^10 < 2^64: refresh well before bias-by-exhaustion.
            bits = entropy.rand_u64();
            used = 0;
        }
        out.push(ALPHABET[(bits % 36) as usize] as char);
        bits /= 36;
        used += 1;
    }
    out
}

/// Lowercase Crockford base32 of the 48-bit ms timestamp (10 chars) plus a
/// random tail, truncated to `length` — matches taskmd's observed output
/// (`01kwhx-…` for the default length 6, which keeps only the leading
/// timestamp chars).
fn ulid_id(length: usize, now_ms: u64, entropy: &mut dyn Entropy) -> String {
    const CROCKFORD: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let mut out = String::with_capacity(length.max(10));
    // 48-bit timestamp, most significant 5-bit group first (10 groups).
    for i in (0..10).rev() {
        let idx = ((now_ms >> (5 * i)) & 0x1F) as usize;
        out.push(CROCKFORD[idx] as char);
    }
    let mut bits = entropy.rand_u64();
    while out.len() < length {
        out.push(CROCKFORD[(bits & 0x1F) as usize] as char);
        bits >>= 5;
        if bits == 0 {
            bits = entropy.rand_u64();
        }
    }
    out.truncate(length);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskmd::model::Status;

    /// Deterministic entropy: fixed timestamp, scripted random values.
    struct FakeEntropy {
        ms: u64,
        seq: Vec<u64>,
        at: usize,
    }

    impl FakeEntropy {
        fn new(ms: u64, seq: Vec<u64>) -> Self {
            FakeEntropy { ms, seq, at: 0 }
        }
    }

    impl Entropy for FakeEntropy {
        fn now_ms(&mut self) -> u64 {
            self.ms
        }
        fn rand_u64(&mut self) -> u64 {
            let v = self.seq[self.at % self.seq.len()];
            self.at += 1;
            v
        }
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("karamd-store-{uniq}"));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn vault() -> Vault {
        let root = tempdir();
        fs::create_dir_all(root.join("tasks")).unwrap();
        Vault::open(&root).unwrap()
    }

    fn write_task(v: &Vault, rel: &str, content: &str) {
        let path = v.tasks_dir().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn open_uses_config_dir() {
        let root = tempdir();
        fs::write(root.join(".taskmd.yaml"), "dir: ./elsewhere\n").unwrap();
        let v = Vault::open(&root).unwrap();
        assert_eq!(v.tasks_dir(), root.join("elsewhere"));
    }

    #[test]
    fn open_propagates_config_error() {
        let root = tempdir();
        fs::write(root.join(".taskmd.yaml"), "dir: [unclosed\n").unwrap();
        assert!(Vault::open(&root).is_err());
    }

    #[test]
    fn scan_missing_dir_is_empty() {
        let root = tempdir();
        let v = Vault::open(&root).unwrap();
        assert_eq!(v.scan().unwrap(), Scan::default());
    }

    #[test]
    fn scan_collects_tasks_skips_noise_reports_invalid() {
        let v = vault();
        write_task(&v, "001-a.md", "---\nid: \"001\"\ntitle: A\n---\n\n# A\n");
        // Non-task noise that must be silently ignored:
        write_task(&v, "README.md", "# readme, no frontmatter\n");
        write_task(
            &v,
            "TASKMD_SPEC.md",
            "# Spec\n\n```yaml\n---\nid: \"9\"\ntitle: X\n---\n```\n",
        );
        write_task(&v, "notes.txt", "not markdown");
        write_task(&v, ".hidden.md", "---\nid: \"9\"\ntitle: H\n---\n");
        fs::create_dir_all(v.tasks_dir().join(".obsidian")).unwrap();
        write_task(
            &v,
            ".obsidian/002-x.md",
            "---\nid: \"002\"\ntitle: X\n---\n",
        );
        // Task-like but broken: reported, not loaded.
        write_task(
            &v,
            "002-broken.md",
            "---\nid: \"002\"\nstatus: pending\n---\n",
        );

        let scan = v.scan().unwrap();
        assert_eq!(scan.tasks.len(), 1);
        assert_eq!(scan.tasks[0].id(), "001");
        assert_eq!(scan.invalid.len(), 1);
        assert_eq!(scan.invalid[0].rel_path, PathBuf::from("002-broken.md"));
        assert!(scan.invalid[0].reason.contains("`title`"));
    }

    #[test]
    fn scan_subdirectory_becomes_group() {
        let v = vault();
        write_task(&v, "001-root.md", "---\nid: \"001\"\ntitle: R\n---\n");
        write_task(&v, "web/002-in-web.md", "---\nid: \"002\"\ntitle: W\n---\n");
        write_task(
            &v,
            "web/deep/003-deeper.md",
            "---\nid: \"003\"\ntitle: D\n---\n",
        );
        // Explicit group wins over the directory.
        write_task(
            &v,
            "web/004-explicit.md",
            "---\nid: \"004\"\ntitle: E\ngroup: special\n---\n",
        );
        let scan = v.scan().unwrap();
        let group = |id: &str| scan.find(id).unwrap().group();
        assert_eq!(group("001"), None);
        assert_eq!(group("002").as_deref(), Some("web"));
        assert_eq!(group("003").as_deref(), Some("deep"));
        assert_eq!(group("004").as_deref(), Some("special"));
    }

    #[test]
    fn scan_unreadable_file_errors() {
        let v = vault();
        fs::create_dir(v.tasks_dir().join("001-dir.md")).unwrap();
        // A directory named like a .md file is_dir, so it recurses (empty).
        assert!(v.scan().unwrap().tasks.is_empty());
        // But a genuinely unreadable dir errors.
        let root = tempdir();
        fs::write(root.join(".taskmd.yaml"), "dir: ./tasks\n").unwrap();
        fs::write(root.join("tasks"), "a file, not a dir").unwrap();
        let v2 = Vault::open(&root).unwrap();
        assert!(v2.scan().is_err());
    }

    #[test]
    fn find_by_id() {
        let v = vault();
        write_task(&v, "001-a.md", "---\nid: \"001\"\ntitle: A\n---\n");
        assert_eq!(v.find("001").unwrap().title(), "A");
        let err = v.find("999").unwrap_err();
        assert!(err.to_string().contains("no task with id"));
    }

    #[test]
    fn find_duplicate_id_refuses() {
        let v = vault();
        write_task(&v, "001-a.md", "---\nid: \"001\"\ntitle: A\n---\n");
        write_task(&v, "001-b.md", "---\nid: \"001\"\ntitle: B\n---\n");
        let err = v.find("001").unwrap_err();
        assert!(err.to_string().contains("ambiguous"));
    }

    #[test]
    fn allocate_sequential_pads_and_increments() {
        let v = vault();
        let mut e = FakeEntropy::new(0, vec![0]);
        let ids: HashSet<String> = ["001", "007", "not-numeric"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(v.allocate_id(&ids, &mut e), "008");
        assert_eq!(v.allocate_id(&HashSet::new(), &mut e), "001");
    }

    #[test]
    fn allocate_sequential_grows_past_padding() {
        let v = vault();
        let mut e = FakeEntropy::new(0, vec![0]);
        let ids: HashSet<String> = HashSet::from(["999".to_string()]);
        assert_eq!(v.allocate_id(&ids, &mut e), "1000");
    }

    #[test]
    fn allocate_prefixed_matches_taskmd_no_separator() {
        let root = tempdir();
        fs::write(
            root.join(".taskmd.yaml"),
            "id:\n  strategy: prefixed\n  prefix: dr\n",
        )
        .unwrap();
        let v = Vault::open(&root).unwrap();
        let mut e = FakeEntropy::new(0, vec![0]);
        assert_eq!(v.allocate_id(&HashSet::new(), &mut e), "dr001");
        let ids = HashSet::from(["dr001".to_string(), "dr041".to_string(), "x9".to_string()]);
        assert_eq!(v.allocate_id(&ids, &mut e), "dr042");
    }

    #[test]
    fn allocate_random_respects_length_and_collisions() {
        let root = tempdir();
        fs::write(
            root.join(".taskmd.yaml"),
            "id:\n  strategy: random\n  length: 6\n",
        )
        .unwrap();
        let v = Vault::open(&root).unwrap();
        let mut e = FakeEntropy::new(0, vec![42, 42, 7_777_777_777]);
        let first = v.allocate_id(&HashSet::new(), &mut e);
        assert_eq!(first.len(), 6);
        assert!(
            first
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        );
        // Same entropy value collides with `first`; allocation must retry and
        // return something different.
        let mut e2 = FakeEntropy::new(0, vec![42, 42, 7_777_777_777]);
        let second = v.allocate_id(&HashSet::from([first.clone()]), &mut e2);
        assert_ne!(first, second);
        assert_eq!(second.len(), 6);
    }

    #[test]
    fn random_id_refreshes_entropy_for_long_ids() {
        // length > 10 forces a mid-id refresh of the bit pool.
        let mut e = FakeEntropy::new(0, vec![u64::MAX, 1]);
        let id = random_id(12, &mut e);
        assert_eq!(id.len(), 12);
    }

    #[test]
    fn allocate_ulid_is_time_prefixed_and_length_bound() {
        let root = tempdir();
        fs::write(root.join(".taskmd.yaml"), "id:\n  strategy: ulid\n").unwrap();
        let v = Vault::open(&root).unwrap();
        // 2026-07-02 ~ 1783000000000 ms.
        let mut e = FakeEntropy::new(1_783_000_000_000, vec![99]);
        let id = v.allocate_id(&HashSet::new(), &mut e);
        assert_eq!(id.len(), 6);
        // Same ms + same length collides; the bump must yield a fresh id.
        let mut e2 = FakeEntropy::new(1_783_000_000_000, vec![99]);
        let id2 = v.allocate_id(&HashSet::from([id.clone()]), &mut e2);
        assert_ne!(id, id2);
    }

    #[test]
    fn ulid_long_length_appends_random_tail() {
        let mut e = FakeEntropy::new(1_783_000_000_000, vec![0b11111_00001]);
        let id = ulid_id(14, 1_783_000_000_000, &mut e);
        assert_eq!(id.len(), 14);
        // First 10 chars are the timestamp, deterministic for a fixed ms.
        let again = ulid_id(
            14,
            1_783_000_000_000,
            &mut FakeEntropy::new(0, vec![0b11111_00001]),
        );
        assert_eq!(id[..10], again[..10]);
    }

    #[test]
    fn create_writes_task_with_next_id() {
        let v = vault();
        write_task(&v, "002-existing.md", "---\nid: \"002\"\ntitle: E\n---\n");
        let mut e = FakeEntropy::new(0, vec![0]);
        let t = v
            .create("New thing", day(2026, 7, 2), &mut e, &|t| {
                t.set_priority(crate::taskmd::Priority::High);
            })
            .unwrap();
        assert_eq!(t.id(), "003");
        assert_eq!(t.rel_path.as_deref(), Some(Path::new("003-new-thing.md")));
        let on_disk = fs::read_to_string(v.tasks_dir().join("003-new-thing.md")).unwrap();
        assert!(on_disk.contains("id: '003'"));
        assert!(on_disk.contains("priority: high"));
        assert!(on_disk.contains("created_at: 2026-07-02"));
        // And taskmd-style: re-scannable.
        assert_eq!(v.scan().unwrap().tasks.len(), 2);
    }

    #[test]
    fn create_makes_tasks_dir() {
        let root = tempdir();
        let v = Vault::open(&root).unwrap();
        let mut e = FakeEntropy::new(0, vec![0]);
        v.create("First", day(2026, 7, 2), &mut e, &noop_build)
            .unwrap();
        assert!(root.join("tasks/001-first.md").exists());
    }

    #[test]
    fn create_retries_on_filename_collision() {
        // The filename the first allocation would pick is occupied by a file
        // the scanner cannot see as a task (empty). create must not clobber
        // it: the id is burned locally and the next one is used.
        let v = vault();
        fs::write(v.tasks_dir().join("001-same.md"), "").unwrap();
        let mut e = FakeEntropy::new(0, vec![0]);
        let t = v
            .create("Same", day(2026, 7, 2), &mut e, &noop_build)
            .unwrap();
        assert_eq!(t.id(), "002");
        assert_eq!(t.rel_path.as_deref(), Some(Path::new("002-same.md")));
        assert_eq!(
            fs::read_to_string(v.tasks_dir().join("001-same.md")).unwrap(),
            ""
        );
    }

    #[test]
    fn create_gives_up_after_bounded_retries() {
        // Every id the sequential allocator can reach within the retry bound
        // is occupied by an invisible (empty) file: create must error, not
        // loop or clobber.
        let v = vault();
        for n in 1..=16 {
            fs::write(v.tasks_dir().join(format!("{n:03}-same.md")), "").unwrap();
        }
        let mut e = FakeEntropy::new(0, vec![0]);
        let err = v
            .create("Same", day(2026, 7, 2), &mut e, &noop_build)
            .unwrap_err();
        assert!(err.to_string().contains("16 attempts"));
    }

    #[test]
    fn create_propagates_write_error() {
        let root = tempdir();
        fs::write(root.join(".taskmd.yaml"), "dir: ./blocked/tasks\n").unwrap();
        fs::write(root.join("blocked"), "file in the way").unwrap();
        let v = Vault::open(&root).unwrap();
        let mut e = FakeEntropy::new(0, vec![0]);
        assert!(v.create("X", day(2026, 7, 2), &mut e, &noop_build).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn create_open_error_is_not_swallowed() {
        use std::os::unix::fs::PermissionsExt;
        let v = vault();
        fs::set_permissions(v.tasks_dir(), fs::Permissions::from_mode(0o555)).unwrap();
        let mut e = FakeEntropy::new(0, vec![0]);
        let result = v.create("X", day(2026, 7, 2), &mut e, &noop_build);
        fs::set_permissions(v.tasks_dir(), fs::Permissions::from_mode(0o755)).ok();
        assert!(result.is_err());
    }

    #[test]
    fn update_rereads_and_writes_atomically() {
        let v = vault();
        write_task(
            &v,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nstatus: pending\ncustom: keep\n---\n\n# A\n",
        );
        let updated = v
            .update("001", &mut |t| {
                t.set_status(Status::Completed, day(2026, 7, 2));
                Ok(())
            })
            .unwrap();
        assert_eq!(updated.status(), Some(Status::Completed));
        let on_disk = fs::read_to_string(v.tasks_dir().join("001-a.md")).unwrap();
        assert!(on_disk.contains("status: completed"));
        assert!(on_disk.contains("completed_at: 2026-07-02"));
        // Unknown field survives the update.
        assert!(on_disk.contains("custom: keep"));
        // No temp file left behind.
        assert!(
            !fs::read_dir(v.tasks_dir())
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().contains("tmp"))
        );
    }

    #[test]
    fn update_sees_external_change() {
        // The mutation closure runs on the *fresh* on-disk state, not a stale
        // in-memory copy: an external edit between load and update survives.
        let v = vault();
        write_task(&v, "001-a.md", "---\nid: \"001\"\ntitle: A\n---\n");
        // Caller "loaded" the task earlier... then an external sync changed it.
        write_task(
            &v,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nowner: synced-in\n---\n",
        );
        let updated = v
            .update("001", &mut |t| {
                t.set_priority(crate::taskmd::Priority::Low);
                Ok(())
            })
            .unwrap();
        assert_eq!(updated.owner().as_deref(), Some("synced-in"));
    }

    /// Shared no-op mutation; used both where it runs and where it must not.
    fn ok_mutate(_: &mut Task) -> anyhow::Result<()> {
        Ok(())
    }

    /// Shared no-op builder; some create() tests reach it, some fail first.
    fn noop_build(_: &mut Task) {}

    #[test]
    fn update_missing_task_errors() {
        let v = vault();
        assert!(v.update("404", &mut ok_mutate).is_err());
        // The same mutation succeeds against a real task (and proves the
        // no-op write path is sound).
        write_task(&v, "001-a.md", "---\nid: \"001\"\ntitle: A\n---\n");
        assert_eq!(v.update("001", &mut ok_mutate).unwrap().id(), "001");
    }

    #[test]
    fn update_mutation_error_leaves_file_untouched() {
        let v = vault();
        let original = "---\nid: \"001\"\ntitle: A\n---\n";
        write_task(&v, "001-a.md", original);
        let err = v.update("001", &mut |_| bail!("nope"));
        assert!(err.is_err());
        assert_eq!(
            fs::read_to_string(v.tasks_dir().join("001-a.md")).unwrap(),
            original
        );
    }

    #[test]
    fn update_task_in_subdirectory() {
        let v = vault();
        write_task(&v, "web/001-a.md", "---\nid: \"001\"\ntitle: A\n---\n");
        v.update("001", &mut |t| {
            t.set_owner(Some("p"));
            Ok(())
        })
        .unwrap();
        let on_disk = fs::read_to_string(v.tasks_dir().join("web/001-a.md")).unwrap();
        assert!(on_disk.contains("owner: p"));
    }

    #[test]
    fn save_without_path_errors() {
        let v = vault();
        let t = Task::new("001", "x", day(2026, 7, 2));
        let err = v.save(&t).unwrap_err();
        assert!(err.to_string().contains("no file path"));
    }

    #[test]
    fn scan_non_utf8_file_errors_with_context() {
        let v = vault();
        fs::write(v.tasks_dir().join("001-bin.md"), [0xFF, 0xFE, 0x00, 0x9F]).unwrap();
        let err = v.scan().unwrap_err();
        assert!(err.to_string().contains("reading"));
    }

    #[test]
    fn scan_orders_multiple_invalid_files() {
        let v = vault();
        write_task(&v, "002-b.md", "---\nid: \"002\"\nstatus: pending\n---\n");
        write_task(&v, "001-a.md", "---\ntitle: no id\nstatus: pending\n---\n");
        let scan = v.scan().unwrap();
        assert_eq!(scan.invalid.len(), 2);
        assert_eq!(scan.invalid[0].rel_path, PathBuf::from("001-a.md"));
        assert_eq!(scan.invalid[1].rel_path, PathBuf::from("002-b.md"));
    }

    #[test]
    #[cfg(unix)]
    fn save_tmp_write_failure_errors_with_context() {
        use std::os::unix::fs::PermissionsExt;
        let v = vault();
        write_task(&v, "001-a.md", "---\nid: \"001\"\ntitle: A\n---\n");
        let task = v.find("001").unwrap();
        fs::set_permissions(v.tasks_dir(), fs::Permissions::from_mode(0o555)).unwrap();
        let result = v.save(&task);
        fs::set_permissions(v.tasks_dir(), fs::Permissions::from_mode(0o755)).ok();
        assert!(result.unwrap_err().to_string().contains("writing"));
    }

    #[test]
    fn save_rename_onto_directory_errors_with_context() {
        // The task's target path is occupied by a non-empty directory, so the
        // final rename fails after the temp write succeeded.
        let v = vault();
        write_task(&v, "001-a.md", "---\nid: \"001\"\ntitle: A\n---\n");
        let mut task = v.find("001").unwrap();
        task.rel_path = Some(PathBuf::from("occupied.md"));
        let dir = v.tasks_dir().join("occupied.md");
        fs::create_dir_all(dir.join("inner")).unwrap();
        let err = v.save(&task).unwrap_err();
        assert!(err.to_string().contains("renaming"));
    }

    #[test]
    fn system_entropy_produces_usable_values() {
        let mut e = SystemEntropy::default();
        assert!(e.now_ms() > 1_700_000_000_000); // after 2023
        let a = e.rand_u64();
        let b = e.rand_u64();
        assert_ne!(a, b);
    }

    #[test]
    fn round_trip_through_taskmd_shape_preserves_recurring() {
        // The critical compatibility property end to end: a file with our
        // custom `recurring:` marker survives a load+mutate+save cycle.
        let v = vault();
        write_task(
            &v,
            "001-r.md",
            "---\nid: \"001\"\ntitle: R\nstatus: pending\nrecurring: \"checkin\"\n---\n\n# R\n",
        );
        v.update("001", &mut |t| {
            t.set_status(Status::Completed, day(2026, 7, 2));
            Ok(())
        })
        .unwrap();
        let scan = v.scan().unwrap();
        let t = scan.find("001").unwrap();
        assert_eq!(t.recurring().as_deref(), Some("checkin"));
        assert_eq!(t.completed_at(), Some(day(2026, 7, 2)));
    }
}
