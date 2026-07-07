//! Reading existing taskmd tasks and rendering new ones.
//!
//! karamd only ever *adds* files and *reads* completion state; completions
//! happen in taskmd/Obsidian elsewhere. taskmd stamps `completed_at: YYYY-MM-DD`
//! on completion and preserves our custom `recurring:` marker across edits,
//! which is what makes the dedup contract hold.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::NaiveDate;
use serde::Deserialize;

use crate::rule::Rule;

/// The slice of an existing task's frontmatter that karamd cares about.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExistingTask {
    /// Numeric id from the filename prefix (`007-foo.md` -> 7).
    pub num_id: u32,
    pub status: String,
    /// The `recurring:` marker, if any (rule key, or `key:year` for calendar).
    pub recurring: Option<String>,
    pub completed_at: Option<NaiveDate>,
    pub cancelled_at: Option<NaiveDate>,
    pub created_at: Option<NaiveDate>,
}

impl ExistingTask {
    /// A task is "open" unless it has reached a terminal status.
    pub fn is_open(&self) -> bool {
        !matches!(self.status.as_str(), "completed" | "cancelled")
    }

    /// When this occurrence concluded, for after_completion interval maths. A
    /// completed task uses `completed_at`; a cancelled one uses `cancelled_at`
    /// (cancelling skips this occurrence but keeps the cadence). Either way we
    /// fall back to `created_at` so a terminal task with a missing or malformed
    /// date is never mistaken for "never ran" (which would re-fire forever).
    pub fn occurrence_date(&self) -> Option<NaiveDate> {
        self.completed_at.or(self.cancelled_at).or(self.created_at)
    }
}

#[derive(Debug, Default, Deserialize)]
struct Frontmatter {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    recurring: Option<String>,
    #[serde(default)]
    completed_at: Option<String>,
    #[serde(default)]
    cancelled_at: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TaskmdConfig {
    dir: Option<String>,
}

/// Resolve the tasks directory for a vault by reading its `.taskmd.yaml`
/// (`dir:` key). Falls back to `<vault>/tasks` when the config is missing,
/// unreadable, or does not name a dir.
pub fn tasks_dir(vault: &Path) -> PathBuf {
    let dir = fs::read_to_string(vault.join(".taskmd.yaml"))
        .ok()
        .and_then(|s| serde_norway::from_str::<TaskmdConfig>(&s).ok())
        .and_then(|c| c.dir)
        .unwrap_or_else(|| "tasks".to_string());
    vault.join(dir.trim_start_matches("./"))
}

/// Extract the YAML frontmatter block (between the leading `---` fences).
/// Tolerates CRLF line endings, which a cross-platform synced vault will pick
/// up: if the opening fence were LF-only, a CRLF file would lose its
/// `recurring:` marker and karamd would duplicate the task on every run.
fn frontmatter_block(content: &str) -> Option<&str> {
    let rest = content
        .strip_prefix("---\r\n")
        .or_else(|| content.strip_prefix("---\n"))?;
    // Closing fence at a line boundary; `\n---` matches whether or not the line
    // ended with `\r` (serde_norway tolerates the trailing `\r` in the block).
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// Parse a frontmatter date string (`YYYY-MM-DD`), tolerating surrounding
/// quotes and whitespace (including a stray `\r` from CRLF files).
fn parse_date(raw: Option<String>) -> Option<NaiveDate> {
    let raw = raw?;
    let trimmed = raw.trim().trim_matches('"').trim();
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").ok()
}

/// Parse one task file's content into an [`ExistingTask`]. A file without
/// parseable frontmatter still yields a task (with empty status) so its
/// filename id keeps counting toward `next_id`.
fn parse_task(num_id: u32, content: &str) -> ExistingTask {
    let fm = frontmatter_block(content)
        .and_then(|b| serde_norway::from_str::<Frontmatter>(b).ok())
        .unwrap_or_default();
    ExistingTask {
        num_id,
        status: fm.status.unwrap_or_default(),
        recurring: fm.recurring,
        completed_at: parse_date(fm.completed_at),
        cancelled_at: parse_date(fm.cancelled_at),
        created_at: parse_date(fm.created_at),
    }
}

/// Scan a tasks directory for `NNN-*.md` files and parse each. A missing
/// directory scans as empty (first run against a fresh vault).
pub fn scan_dir(dir: &Path) -> Result<Vec<ExistingTask>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    // `flatten` skips any entry we cannot even stat; the initial `read_dir`
    // still surfaces an unreadable directory as an error.
    for entry in fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".md") {
            continue;
        }
        // Assumes taskmd's `NNN-slug.md` convention: a `.md` file without a
        // numeric prefix is skipped, so a `recurring:` marker would only be seen
        // on conventionally-named files. taskmd always names them this way.
        let Some(num_id) = name.split('-').next().and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        let content = fs::read_to_string(entry.path())?;
        out.push(parse_task(num_id, &content));
    }
    Ok(out)
}

/// Next numeric id, one past the highest existing one (1 for an empty vault).
pub fn next_id(existing: &[ExistingTask]) -> u32 {
    existing.iter().map(|t| t.num_id).max().unwrap_or(0) + 1
}

/// Match taskmd's filename slug: lowercase, non-`[a-z0-9]` → `-`, collapse and
/// trim. Non-ASCII letters are dropped (e.g. "prüfen" → "pr-fen").
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true; // trims leading dashes
    for c in title.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Quote a string as a YAML double-quoted scalar (escaping `\` and `"`).
fn yaml_quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn render_tags(tags: &[String]) -> String {
    if tags.is_empty() {
        return "[]".to_string();
    }
    let inner: Vec<String> = tags.iter().map(|t| yaml_quote(t)).collect();
    format!("[{}]", inner.join(", "))
}

/// The generated markdown for one task: full taskmd frontmatter (including the
/// `recurring` dedup marker), the `# <title>` heading, a karamd provenance
/// comment, and a body. A rule may supply its own `body`; otherwise a minimal
/// `TODO` stub is used so rules without one keep their output.
pub fn render_task(rule: &Rule, id: &str, marker: &str, today: NaiveDate) -> String {
    let mut fm = String::from("---\n");
    fm.push_str(&format!("id: {}\n", yaml_quote(id)));
    fm.push_str(&format!("title: {}\n", yaml_quote(&rule.title)));
    fm.push_str("status: pending\n");
    fm.push_str(&format!(
        "priority: {}\n",
        rule.priority.as_deref().unwrap_or("medium")
    ));
    if let Some(phase) = &rule.phase {
        fm.push_str(&format!("phase: {phase}\n"));
    }
    fm.push_str("dependencies: []\n");
    fm.push_str(&format!("tags: {}\n", render_tags(&rule.tags)));
    fm.push_str(&format!("created_at: {}\n", today.format("%Y-%m-%d")));
    fm.push_str(&format!("recurring: {}\n", yaml_quote(marker)));
    fm.push_str("---\n\n");

    match &rule.body {
        Some(body) => format!(
            "{fm}# {title}\n\n<!-- Generated by karamd for recurring rule `{key}`. -->\n\n{body}\n",
            title = rule.title,
            key = rule.key,
            body = body.trim(),
        ),
        None => format!(
            "{fm}# {title}\n\n## Objective\n\n<!-- Generated by karamd for recurring rule `{key}`. -->\n\n## Tasks\n\n- [ ] TODO\n\n## Acceptance Criteria\n\n- TODO\n",
            title = rule.title,
            key = rule.key,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::Trigger;

    fn rule() -> Rule {
        Rule {
            key: "periodic-checkin".into(),
            title: "Reach out to [[Someone]]".into(),
            trigger: Trigger::AfterCompletion,
            every_days: Some(18),
            annual: None,
            lead_days: None,
            day_of_month: None,
            day_of_week: None,
            phase: Some("next".into()),
            priority: Some("high".into()),
            tags: vec!["personal".into(), "work".into()],
            body: None,
        }
    }

    #[test]
    fn slugify_matches_taskmd() {
        assert_eq!(slugify("Reach out to [[Someone]]"), "reach-out-to-someone");
        assert_eq!(
            slugify("Monatszusammenfassung prüfen: [[2026-06 June]]"),
            "monatszusammenfassung-pr-fen-2026-06-june"
        );
    }

    #[test]
    fn next_id_from_empty_is_one() {
        assert_eq!(next_id(&[]), 1);
    }

    #[test]
    fn next_id_is_one_past_max() {
        let tasks = vec![
            ExistingTask {
                num_id: 1,
                status: "pending".into(),
                ..Default::default()
            },
            ExistingTask {
                num_id: 6,
                status: "completed".into(),
                ..Default::default()
            },
            ExistingTask {
                num_id: 3,
                status: "pending".into(),
                ..Default::default()
            },
        ];
        assert_eq!(next_id(&tasks), 7);
    }

    #[test]
    fn is_open_terminal_statuses() {
        let mut t = ExistingTask {
            num_id: 1,
            status: "pending".into(),
            ..Default::default()
        };
        assert!(t.is_open());
        t.status = "in-progress".into();
        assert!(t.is_open());
        t.status = "completed".into();
        assert!(!t.is_open());
        t.status = "cancelled".into();
        assert!(!t.is_open());
    }

    #[test]
    fn parse_task_reads_fields() {
        let content = "---\nid: \"007\"\nstatus: completed\nrecurring: \"periodic-checkin\"\ncompleted_at: 2026-06-13\n---\n\n# body\n";
        let t = parse_task(7, content);
        assert_eq!(t.status, "completed");
        assert_eq!(t.recurring.as_deref(), Some("periodic-checkin"));
        assert_eq!(t.completed_at, NaiveDate::from_ymd_opt(2026, 6, 13));
    }

    #[test]
    fn parse_task_quoted_completed_at() {
        let content = "---\nstatus: completed\ncompleted_at: \"2026-06-13\"\n---\n";
        assert_eq!(
            parse_task(1, content).completed_at,
            NaiveDate::from_ymd_opt(2026, 6, 13)
        );
    }

    #[test]
    fn parse_task_without_frontmatter_is_empty() {
        let t = parse_task(9, "no frontmatter here");
        assert_eq!(t.num_id, 9);
        assert_eq!(t.status, "");
        assert_eq!(t.recurring, None);
        assert_eq!(t.completed_at, None);
    }

    #[test]
    fn parse_task_unterminated_frontmatter_is_empty() {
        // Opening fence but no closing `---`.
        let t = parse_task(3, "---\nstatus: pending\nno closing fence\n");
        assert_eq!(t.status, "");
    }

    #[test]
    fn parse_task_bad_completed_at_is_none() {
        let content = "---\nstatus: completed\ncompleted_at: not-a-date\n---\n";
        assert_eq!(parse_task(1, content).completed_at, None);
    }

    #[test]
    fn parse_task_crlf_frontmatter_survives() {
        // A CRLF file must still yield status, recurring, and completed_at, or
        // the dedup marker is lost and the task duplicates every run.
        let content = "---\r\nid: \"007\"\r\nstatus: completed\r\nrecurring: \"checkin\"\r\ncompleted_at: 2026-06-13\r\n---\r\n\r\n# body\r\n";
        let t = parse_task(7, content);
        assert_eq!(t.status, "completed");
        assert_eq!(t.recurring.as_deref(), Some("checkin"));
        assert_eq!(t.completed_at, NaiveDate::from_ymd_opt(2026, 6, 13));
    }

    #[test]
    fn parse_task_reads_cancelled_and_created() {
        let content =
            "---\nstatus: cancelled\ncancelled_at: 2026-06-10\ncreated_at: 2026-05-01\n---\n";
        let t = parse_task(1, content);
        assert_eq!(t.cancelled_at, NaiveDate::from_ymd_opt(2026, 6, 10));
        assert_eq!(t.created_at, NaiveDate::from_ymd_opt(2026, 5, 1));
    }

    #[test]
    fn occurrence_date_prefers_completed_then_cancelled_then_created() {
        let base = ExistingTask {
            created_at: NaiveDate::from_ymd_opt(2026, 1, 1),
            ..Default::default()
        };
        assert_eq!(base.occurrence_date(), NaiveDate::from_ymd_opt(2026, 1, 1));
        let cancelled = ExistingTask {
            cancelled_at: NaiveDate::from_ymd_opt(2026, 2, 2),
            ..base.clone()
        };
        assert_eq!(
            cancelled.occurrence_date(),
            NaiveDate::from_ymd_opt(2026, 2, 2)
        );
        let completed = ExistingTask {
            completed_at: NaiveDate::from_ymd_opt(2026, 3, 3),
            ..cancelled
        };
        assert_eq!(
            completed.occurrence_date(),
            NaiveDate::from_ymd_opt(2026, 3, 3)
        );
        assert_eq!(ExistingTask::default().occurrence_date(), None);
    }

    #[test]
    fn parse_task_unparseable_frontmatter_falls_back() {
        let content = "---\n: : bad\n---\n";
        let t = parse_task(2, content);
        assert_eq!(t.status, "");
    }

    #[test]
    fn tasks_dir_reads_config() {
        let tmp = tempdir();
        fs::write(tmp.join(".taskmd.yaml"), "dir: ./mytasks\n").unwrap();
        assert_eq!(tasks_dir(&tmp), tmp.join("mytasks"));
    }

    #[test]
    fn tasks_dir_defaults_without_config() {
        let tmp = tempdir();
        assert_eq!(tasks_dir(&tmp), tmp.join("tasks"));
    }

    #[test]
    fn tasks_dir_defaults_on_config_without_dir() {
        let tmp = tempdir();
        fs::write(tmp.join(".taskmd.yaml"), "other: value\n").unwrap();
        assert_eq!(tasks_dir(&tmp), tmp.join("tasks"));
    }

    #[test]
    fn scan_dir_missing_is_empty() {
        let tmp = tempdir();
        assert!(scan_dir(&tmp.join("nope")).unwrap().is_empty());
    }

    #[test]
    fn scan_dir_reads_md_skips_others() {
        let tmp = tempdir();
        fs::write(tmp.join("001-a.md"), "---\nstatus: pending\n---\n").unwrap();
        fs::write(tmp.join("notes.txt"), "ignore me").unwrap();
        fs::write(tmp.join("no-number.md"), "---\nstatus: pending\n---\n").unwrap();
        let tasks = scan_dir(&tmp).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].num_id, 1);
    }

    #[test]
    fn scan_dir_on_a_file_errors() {
        let tmp = tempdir();
        let file = tmp.join("not-a-dir");
        fs::write(&file, "x").unwrap();
        assert!(scan_dir(&file).is_err());
    }

    #[test]
    fn scan_dir_unreadable_task_errors() {
        // A directory named like a task file: read_to_string on it fails.
        let tmp = tempdir();
        fs::create_dir(tmp.join("001-weird.md")).unwrap();
        assert!(scan_dir(&tmp).is_err());
    }

    #[test]
    fn render_task_has_frontmatter_and_marker() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let out = render_task(&rule(), "007", "periodic-checkin", today);
        assert!(out.starts_with("---\n"));
        assert!(out.contains("id: \"007\""));
        assert!(out.contains("title: \"Reach out to [[Someone]]\""));
        assert!(out.contains("status: pending"));
        assert!(out.contains("priority: high"));
        assert!(out.contains("phase: next"));
        assert!(out.contains("dependencies: []"));
        assert!(out.contains("tags: [\"personal\", \"work\"]"));
        assert!(out.contains("created_at: 2026-07-01"));
        assert!(out.contains("recurring: \"periodic-checkin\""));
        assert!(out.contains("# Reach out to [[Someone]]"));
    }

    #[test]
    fn render_task_defaults_priority_and_omits_phase() {
        let mut r = rule();
        r.priority = None;
        r.phase = None;
        r.tags = vec![];
        let today = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let out = render_task(&r, "001", "k:2026", today);
        assert!(out.contains("priority: medium"));
        assert!(!out.contains("phase:"));
        assert!(out.contains("tags: []"));
        assert!(out.contains("recurring: \"k:2026\""));
    }

    #[test]
    fn render_task_stub_has_provenance() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let out = render_task(&rule(), "007", "periodic-checkin", today);
        assert!(
            out.contains("<!-- Generated by karamd for recurring rule `periodic-checkin`. -->")
        );
        assert!(out.contains("- [ ] TODO"));
    }

    #[test]
    fn render_task_with_body_replaces_stub() {
        let mut r = rule();
        // Leading and trailing blank lines are trimmed to a single trailing \n.
        r.body = Some("\n\n## Objective\n\nCall the dentist.\n\n".into());
        let today = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let out = render_task(&r, "007", "periodic-checkin", today);
        assert!(out.contains("-->\n\n## Objective"));
        // Frontmatter, heading, and provenance are kept regardless of the body.
        assert!(out.starts_with("---\n"));
        assert!(out.contains("recurring: \"periodic-checkin\""));
        assert!(out.contains("# Reach out to [[Someone]]"));
        assert!(
            out.contains("<!-- Generated by karamd for recurring rule `periodic-checkin`. -->")
        );
        // The provided body replaces the stub: our text is present, TODO is not.
        assert!(out.contains("Call the dentist."));
        assert!(!out.contains("TODO"));
        // Exactly one trailing newline (body's own trailing whitespace trimmed).
        assert!(out.ends_with("Call the dentist.\n"));
        assert!(!out.ends_with("\n\n"));
    }

    #[test]
    fn yaml_quote_escapes() {
        assert_eq!(yaml_quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    /// Minimal throwaway temp dir under the OS temp root, unique per test name.
    fn tempdir() -> PathBuf {
        let base = std::env::temp_dir().join(format!("karamd-t-{}", unique()));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn unique() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        // pid mixes in cross-process uniqueness without needing the clock.
        (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed)
    }
}
