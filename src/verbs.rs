//! Task verbs on top of the taskmd library: create, list/query, show, and the
//! status transitions. Thin functions that return [`TaskView`]s; the CLI layer
//! only renders.

use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;

use crate::output::TaskView;
use crate::query::{self, EvalCtx};
use crate::taskmd::{
    Effort, Entropy, Graph, Priority, Status, TaskType, Vault, Workflow, template,
};

/// Everything `create` accepts. Enum-valued fields are parsed here so a typo
/// fails before any file is written.
#[derive(Debug, Default)]
pub struct CreateSpec {
    pub title: String,
    pub priority: Option<String>,
    pub effort: Option<String>,
    pub task_type: Option<String>,
    pub phase: Option<String>,
    pub tags: Vec<String>,
    pub dependencies: Vec<String>,
    pub template: Option<String>,
    pub body: Option<String>,
}

/// Create a new task file in the vault. Template defaults apply first,
/// explicit fields override.
pub fn create(
    root: &Path,
    spec: &CreateSpec,
    today: NaiveDate,
    entropy: &mut dyn Entropy,
) -> Result<TaskView> {
    if spec.title.trim().is_empty() {
        bail!("title must not be empty");
    }
    // Parse enum-valued inputs up front (loud errors, nothing written yet).
    let priority = spec
        .priority
        .as_deref()
        .map(|p| Priority::parse(p).with_context(|| format!("invalid priority `{p}`")))
        .transpose()?;
    let effort = spec
        .effort
        .as_deref()
        .map(|e| Effort::parse(e).with_context(|| format!("invalid effort `{e}`")))
        .transpose()?;
    let task_type = spec
        .task_type
        .as_deref()
        .map(|t| TaskType::parse(t).with_context(|| format!("invalid type `{t}`")))
        .transpose()?;
    let tpl = spec
        .template
        .as_deref()
        .map(|name| template::resolve(root, name))
        .transpose()?;

    let vault = Vault::open(root)?;

    // Dependencies must exist; a dangling ref would fail taskmd validation.
    if !spec.dependencies.is_empty() {
        let scan = vault.scan()?;
        for dep in &spec.dependencies {
            if scan.find(dep).is_none() {
                bail!("dependency `{dep}` does not exist");
            }
        }
    }

    let task = vault.create(&spec.title, today, entropy, &|t| {
        if let Some(tpl) = &tpl {
            template::apply(tpl, t);
        }
        if let Some(p) = priority {
            t.set_priority(p);
        }
        if let Some(e) = effort {
            t.set_effort(e);
        }
        if let Some(tt) = task_type {
            t.set_task_type(tt);
        }
        if let Some(phase) = &spec.phase {
            t.set_phase(Some(phase));
        }
        if !spec.tags.is_empty() {
            t.set_tags(&spec.tags);
        }
        if !spec.dependencies.is_empty() {
            t.set_dependencies(&spec.dependencies);
        }
        if let Some(body) = &spec.body {
            t.set_body(body);
        }
    })?;

    let scan = vault.scan()?;
    let graph = Graph::build(&scan.tasks);
    Ok(TaskView::build(&task, &graph, true))
}

/// A partial edit of an existing task. Every field is optional: `None` leaves
/// it untouched. `phase`/`owner` use a nested option so the API can *clear* a
/// field (`Some(None)`) as distinct from leaving it (`None`).
#[derive(Debug, Default)]
pub struct EditSpec {
    pub title: Option<String>,
    pub priority: Option<String>,
    pub effort: Option<String>,
    pub task_type: Option<String>,
    pub phase: Option<Option<String>>,
    pub owner: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
    pub dependencies: Option<Vec<String>>,
    pub body: Option<String>,
}

/// Apply a partial edit to an existing task. Enum-valued inputs and dependency
/// existence are checked up front (loud errors, nothing written); the mutation
/// itself goes through the defensive re-reading store. Status is not editable
/// here — transitions go through [`set_status`]/[`complete`].
pub fn edit(root: &Path, id: &str, spec: &EditSpec) -> Result<TaskView> {
    if let Some(title) = &spec.title
        && title.trim().is_empty()
    {
        bail!("title must not be empty");
    }
    let priority = spec
        .priority
        .as_deref()
        .map(|p| Priority::parse(p).with_context(|| format!("invalid priority `{p}`")))
        .transpose()?;
    let effort = spec
        .effort
        .as_deref()
        .map(|e| Effort::parse(e).with_context(|| format!("invalid effort `{e}`")))
        .transpose()?;
    let task_type = spec
        .task_type
        .as_deref()
        .map(|t| TaskType::parse(t).with_context(|| format!("invalid type `{t}`")))
        .transpose()?;

    let vault = Vault::open(root)?;

    if let Some(deps) = &spec.dependencies
        && !deps.is_empty()
    {
        let scan = vault.scan()?;
        for dep in deps {
            if dep == id {
                bail!("a task cannot depend on itself (`{id}`)");
            }
            if scan.find(dep).is_none() {
                bail!("dependency `{dep}` does not exist");
            }
        }
    }

    let task = vault.update(id, &mut |t| {
        if let Some(title) = &spec.title {
            t.set_title(title);
        }
        if let Some(p) = priority {
            t.set_priority(p);
        }
        if let Some(e) = effort {
            t.set_effort(e);
        }
        if let Some(tt) = task_type {
            t.set_task_type(tt);
        }
        if let Some(phase) = &spec.phase {
            t.set_phase(phase.as_deref());
        }
        if let Some(owner) = &spec.owner {
            t.set_owner(owner.as_deref());
        }
        if let Some(tags) = &spec.tags {
            t.set_tags(tags);
        }
        if let Some(deps) = &spec.dependencies {
            t.set_dependencies(deps);
        }
        if let Some(body) = &spec.body {
            t.set_body(body);
        }
        Ok(())
    })?;
    let scan = vault.scan()?;
    let graph = Graph::build(&scan.tasks);
    Ok(TaskView::build(&task, &graph, true))
}

/// List tasks, optionally filtered by a query. Returns the matching views and
/// the number of invalid (broken task-like) files, which callers surface as a
/// warning.
pub fn list(root: &Path, query_str: Option<&str>) -> Result<(Vec<TaskView>, usize)> {
    let expr = query_str.map(query::parse).transpose()?;
    let vault = Vault::open(root)?;
    let scan = vault.scan()?;
    let graph = Graph::build(&scan.tasks);
    let views = scan
        .tasks
        .iter()
        .filter(|t| {
            expr.as_ref().is_none_or(|e| {
                let ctx = EvalCtx {
                    ready: graph.is_ready(t),
                };
                query::eval(e, t, &ctx)
            })
        })
        .map(|t| TaskView::build(t, &graph, false))
        .collect();
    Ok((views, scan.invalid.len()))
}

/// Full-text search across task titles and bodies (case-insensitive substring).
/// Returns matching views (no body) and the count of broken files, like
/// [`list`].
pub fn search(root: &Path, needle: &str) -> Result<(Vec<TaskView>, usize)> {
    let query = needle.to_lowercase();
    let vault = Vault::open(root)?;
    let scan = vault.scan()?;
    let graph = Graph::build(&scan.tasks);
    let views = scan
        .tasks
        .iter()
        .filter(|t| {
            t.title().to_lowercase().contains(&query) || t.body.to_lowercase().contains(&query)
        })
        .map(|t| TaskView::build(t, &graph, false))
        .collect();
    Ok((views, scan.invalid.len()))
}

/// One task with its body (detail view).
pub fn show(root: &Path, id: &str) -> Result<TaskView> {
    let vault = Vault::open(root)?;
    let scan = vault.scan()?;
    let graph = Graph::build(&scan.tasks);
    let task = scan
        .find(id)
        .with_context(|| format!("no task with id `{id}`"))?;
    Ok(TaskView::build(task, &graph, true))
}

/// Set an explicit status (full enum). Auto timestamps are handled by the
/// model.
pub fn set_status(root: &Path, id: &str, status: Status, today: NaiveDate) -> Result<TaskView> {
    let vault = Vault::open(root)?;
    let task = vault.update(id, &mut |t| {
        t.set_status(status, today);
        Ok(())
    })?;
    let scan = vault.scan()?;
    let graph = Graph::build(&scan.tasks);
    Ok(TaskView::build(&task, &graph, false))
}

/// Workflow-aware completion: `solo` marks `completed`; `pr-review` marks
/// `in-review` (a PR merge completes it later) and records `--pr` when given.
pub fn complete(root: &Path, id: &str, pr: Option<&str>, today: NaiveDate) -> Result<TaskView> {
    let vault = Vault::open(root)?;
    let target = match vault.config.workflow {
        Workflow::Solo => Status::Completed,
        Workflow::PrReview => Status::InReview,
    };
    let task = vault.update(id, &mut |t| {
        t.set_status(target, today);
        if let Some(url) = pr {
            t.add_pr(url);
        }
        Ok(())
    })?;
    let scan = vault.scan()?;
    let graph = Graph::build(&scan.tasks);
    Ok(TaskView::build(&task, &graph, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    struct FixedEntropy;
    impl Entropy for FixedEntropy {
        fn now_ms(&mut self) -> u64 {
            1_783_000_000_000
        }
        fn rand_u64(&mut self) -> u64 {
            42
        }
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("karamd-verbs-{uniq}"));
        fs::create_dir_all(base.join("tasks")).unwrap();
        base
    }

    fn write_task(root: &Path, rel: &str, content: &str) {
        fs::write(root.join("tasks").join(rel), content).unwrap();
    }

    fn spec(title: &str) -> CreateSpec {
        CreateSpec {
            title: title.into(),
            ..CreateSpec::default()
        }
    }

    #[test]
    fn create_with_ulid_strategy_uses_entropy_clock() {
        let root = tempdir();
        fs::write(root.join(".taskmd.yaml"), "id:\n  strategy: ulid\n").unwrap();
        let v = create(&root, &spec("Timed"), day(2026, 7, 2), &mut FixedEntropy).unwrap();
        assert_eq!(v.id.len(), 6); // default length
        assert!(root.join(format!("tasks/{}-timed.md", v.id)).exists());
    }

    #[test]
    fn create_minimal_task() {
        let root = tempdir();
        let v = create(
            &root,
            &spec("Do the thing"),
            day(2026, 7, 2),
            &mut FixedEntropy,
        )
        .unwrap();
        assert_eq!(v.id, "001");
        assert_eq!(v.status, "pending");
        assert_eq!(v.created_at.as_deref(), Some("2026-07-02"));
        assert!(root.join("tasks/001-do-the-thing.md").exists());
    }

    #[test]
    fn create_full_task_with_flags() {
        let root = tempdir();
        write_task(
            &root,
            "001-dep.md",
            "---\nid: \"001\"\ntitle: Dep\nstatus: completed\n---\n",
        );
        let s = CreateSpec {
            title: "Full".into(),
            priority: Some("critical".into()),
            effort: Some("small".into()),
            task_type: Some("bug".into()),
            phase: Some("v1".into()),
            tags: vec!["a".into(), "b".into()],
            dependencies: vec!["001".into()],
            template: None,
            body: Some("## My body\n\n- [ ] step".into()),
        };
        let v = create(&root, &s, day(2026, 7, 2), &mut FixedEntropy).unwrap();
        assert_eq!(v.id, "002");
        assert_eq!(v.priority, "critical");
        assert_eq!(v.effort.as_deref(), Some("small"));
        assert_eq!(v.task_type.as_deref(), Some("bug"));
        assert_eq!(v.phase.as_deref(), Some("v1"));
        assert_eq!(v.tags, vec!["a", "b"]);
        assert_eq!(v.dependencies, vec!["001"]);
        assert!(v.ready); // dep is completed
        assert!(v.body.as_deref().unwrap().contains("## My body"));
        // On-disk file is taskmd-parseable with the right frontmatter.
        let raw = fs::read_to_string(root.join("tasks/002-full.md")).unwrap();
        assert!(raw.contains("priority: critical"));
        assert!(raw.contains("dependencies:"));
    }

    #[test]
    fn create_with_template_and_overrides() {
        let root = tempdir();
        let s = CreateSpec {
            title: "Broken".into(),
            template: Some("bug".into()),
            priority: Some("low".into()), // overrides the template's high
            ..CreateSpec::default()
        };
        let v = create(&root, &s, day(2026, 7, 2), &mut FixedEntropy).unwrap();
        assert_eq!(v.priority, "low");
        assert_eq!(v.task_type.as_deref(), Some("bug"));
        assert!(v.body.as_deref().unwrap().contains("## Steps to Reproduce"));
    }

    #[test]
    fn create_rejects_bad_inputs() {
        let root = tempdir();
        let mut e = FixedEntropy;
        let cases: Vec<(CreateSpec, &str)> = vec![
            (spec("  "), "title"),
            (
                CreateSpec {
                    priority: Some("urgent".into()),
                    ..spec("X")
                },
                "invalid priority",
            ),
            (
                CreateSpec {
                    effort: Some("epic".into()),
                    ..spec("X")
                },
                "invalid effort",
            ),
            (
                CreateSpec {
                    task_type: Some("story".into()),
                    ..spec("X")
                },
                "invalid type",
            ),
            (
                CreateSpec {
                    template: Some("nope".into()),
                    ..spec("X")
                },
                "unknown template",
            ),
            (
                CreateSpec {
                    dependencies: vec!["404".into()],
                    ..spec("X")
                },
                "dependency `404` does not exist",
            ),
        ];
        for (s, needle) in cases {
            let err = create(&root, &s, day(2026, 7, 2), &mut e).unwrap_err();
            assert!(
                err.to_string().contains(needle),
                "expected `{needle}` in `{err}`"
            );
        }
        // Nothing was written by any failed attempt.
        assert_eq!(fs::read_dir(root.join("tasks")).unwrap().count(), 0);
    }

    #[test]
    fn list_all_and_filtered() {
        let root = tempdir();
        write_task(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nstatus: completed\npriority: high\n---\n",
        );
        write_task(
            &root,
            "002-b.md",
            "---\nid: \"002\"\ntitle: B\nstatus: pending\ndependencies: [\"001\"]\n---\n",
        );
        write_task(
            &root,
            "003-c.md",
            "---\nid: \"003\"\ntitle: C\nstatus: pending\ndependencies: [\"002\"]\n---\n",
        );
        let (all, invalid) = list(&root, None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(invalid, 0);

        let (pending, _) = list(&root, Some("status:pending")).unwrap();
        assert_eq!(pending.len(), 2);

        // ready uses the graph: 002's dep is completed, 003's is not.
        let (ready, _) = list(&root, Some("status:pending AND ready:true")).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "002");
    }

    #[test]
    fn list_counts_invalid_files() {
        let root = tempdir();
        write_task(
            &root,
            "001-broken.md",
            "---\nid: \"001\"\nstatus: pending\n---\n",
        );
        let (views, invalid) = list(&root, None).unwrap();
        assert!(views.is_empty());
        assert_eq!(invalid, 1);
    }

    #[test]
    fn list_bad_query_errors() {
        let root = tempdir();
        assert!(list(&root, Some("bogus:x")).is_err());
    }

    #[test]
    fn search_matches_title_and_body() {
        let root = tempdir();
        write_task(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: Fix the Login\n---\n\n# Fix the Login\n\nnothing special\n",
        );
        write_task(
            &root,
            "002-b.md",
            "---\nid: \"002\"\ntitle: Unrelated\n---\n\n# Unrelated\n\nmentions login in the body\n",
        );
        write_task(
            &root,
            "003-c.md",
            "---\nid: \"003\"\ntitle: Nope\n---\n\n# Nope\n\nnope\n",
        );
        // Case-insensitive, matches title (001) and body (002).
        let (hits, invalid) = search(&root, "LOGIN").unwrap();
        let ids: Vec<&str> = hits.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec!["001", "002"]);
        assert_eq!(invalid, 0);
        // No match → empty.
        assert!(search(&root, "zzz").unwrap().0.is_empty());
    }

    #[test]
    fn search_propagates_scan_error() {
        let root = tempdir();
        fs::write(root.join(".taskmd.yaml"), "dir: [unclosed\n").unwrap();
        assert!(search(&root, "x").is_err());
    }

    #[test]
    fn show_returns_body() {
        let root = tempdir();
        write_task(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\n---\n\n# A\n\ndetails\n",
        );
        let v = show(&root, "001").unwrap();
        assert!(v.body.as_deref().unwrap().contains("details"));
        assert!(show(&root, "404").is_err());
    }

    #[test]
    fn set_status_full_enum_with_timestamps() {
        let root = tempdir();
        write_task(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nstatus: pending\n---\n",
        );
        let v = set_status(&root, "001", Status::InProgress, day(2026, 7, 2)).unwrap();
        assert_eq!(v.status, "in-progress");
        let v = set_status(&root, "001", Status::Blocked, day(2026, 7, 2)).unwrap();
        assert_eq!(v.status, "blocked");
        let v = set_status(&root, "001", Status::Cancelled, day(2026, 7, 3)).unwrap();
        assert_eq!(v.status, "cancelled");
        assert_eq!(v.cancelled_at.as_deref(), Some("2026-07-03"));
        // Reopen clears the terminal timestamp.
        let v = set_status(&root, "001", Status::Pending, day(2026, 7, 4)).unwrap();
        assert_eq!(v.status, "pending");
        assert_eq!(v.cancelled_at, None);
    }

    #[test]
    fn complete_solo_marks_completed() {
        let root = tempdir();
        write_task(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nstatus: in-progress\n---\n",
        );
        let v = complete(&root, "001", None, day(2026, 7, 2)).unwrap();
        assert_eq!(v.status, "completed");
        assert_eq!(v.completed_at.as_deref(), Some("2026-07-02"));
    }

    #[test]
    fn complete_pr_review_marks_in_review_and_records_pr() {
        let root = tempdir();
        fs::write(root.join(".taskmd.yaml"), "workflow: pr-review\n").unwrap();
        write_task(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nstatus: in-progress\n---\n",
        );
        let v = complete(&root, "001", Some("https://x/pull/1"), day(2026, 7, 2)).unwrap();
        assert_eq!(v.status, "in-review");
        assert_eq!(v.completed_at, None);
        let raw = fs::read_to_string(root.join("tasks/001-a.md")).unwrap();
        assert!(raw.contains("https://x/pull/1"));
    }

    #[test]
    fn edit_updates_fields_and_can_clear() {
        let root = tempdir();
        write_task(
            &root,
            "001-dep.md",
            "---\nid: \"001\"\ntitle: Dep\nstatus: completed\n---\n",
        );
        write_task(
            &root,
            "002-a.md",
            "---\nid: \"002\"\ntitle: Old\nstatus: pending\nphase: v1\nowner: nobody\n---\n\n# Old\n\nold body\n",
        );
        let spec = EditSpec {
            title: Some("New title".into()),
            priority: Some("high".into()),
            effort: Some("small".into()),
            task_type: Some("bug".into()),
            phase: Some(None), // clear phase
            owner: Some(Some("me".into())),
            tags: Some(vec!["x".into()]),
            dependencies: Some(vec!["001".into()]),
            body: Some("new body".into()),
        };
        let v = edit(&root, "002", &spec).unwrap();
        assert_eq!(v.title, "New title");
        assert_eq!(v.priority, "high");
        assert_eq!(v.effort.as_deref(), Some("small"));
        assert_eq!(v.task_type.as_deref(), Some("bug"));
        assert_eq!(v.phase, None);
        assert_eq!(v.owner.as_deref(), Some("me"));
        assert_eq!(v.tags, vec!["x"]);
        assert_eq!(v.dependencies, vec!["001"]);
        assert!(v.ready); // dep completed
        assert!(v.body.as_deref().unwrap().contains("new body"));
    }

    #[test]
    fn edit_no_op_leaves_task_intact() {
        let root = tempdir();
        write_task(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: Keep\nstatus: pending\nphase: v1\nrecurring: \"k\"\n---\n\n# Keep\n\nbody\n",
        );
        let v = edit(&root, "001", &EditSpec::default()).unwrap();
        assert_eq!(v.title, "Keep");
        assert_eq!(v.phase.as_deref(), Some("v1"));
        let raw = fs::read_to_string(root.join("tasks/001-a.md")).unwrap();
        assert!(raw.contains("recurring:"));
        assert!(raw.contains("body"));
    }

    #[test]
    fn edit_rejects_bad_inputs() {
        let root = tempdir();
        write_task(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nstatus: pending\n---\n",
        );
        let cases: Vec<(EditSpec, &str)> = vec![
            (
                EditSpec {
                    title: Some("   ".into()),
                    ..EditSpec::default()
                },
                "title must not be empty",
            ),
            (
                EditSpec {
                    priority: Some("urgent".into()),
                    ..EditSpec::default()
                },
                "invalid priority",
            ),
            (
                EditSpec {
                    effort: Some("epic".into()),
                    ..EditSpec::default()
                },
                "invalid effort",
            ),
            (
                EditSpec {
                    task_type: Some("story".into()),
                    ..EditSpec::default()
                },
                "invalid type",
            ),
            (
                EditSpec {
                    dependencies: Some(vec!["404".into()]),
                    ..EditSpec::default()
                },
                "does not exist",
            ),
            (
                EditSpec {
                    dependencies: Some(vec!["001".into()]),
                    ..EditSpec::default()
                },
                "cannot depend on itself",
            ),
        ];
        for (s, needle) in cases {
            let err = edit(&root, "001", &s).unwrap_err();
            assert!(
                err.to_string().contains(needle),
                "expected `{needle}` in `{err}`"
            );
        }
        // The task was never rewritten by a rejected edit.
        let raw = fs::read_to_string(root.join("tasks/001-a.md")).unwrap();
        assert!(raw.contains("title: A"));
    }

    #[test]
    fn edit_missing_task_errors() {
        let root = tempdir();
        assert!(edit(&root, "404", &EditSpec::default()).is_err());
    }

    #[test]
    fn mutations_preserve_custom_fields() {
        let root = tempdir();
        write_task(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nstatus: pending\nrecurring: \"checkin\"\nweird: [1, 2]\n---\n",
        );
        complete(&root, "001", None, day(2026, 7, 2)).unwrap();
        let raw = fs::read_to_string(root.join("tasks/001-a.md")).unwrap();
        assert!(raw.contains("recurring:"));
        assert!(raw.contains("weird:"));
    }

    #[test]
    fn missing_task_errors_on_mutations() {
        let root = tempdir();
        assert!(set_status(&root, "404", Status::Completed, day(2026, 7, 2)).is_err());
        assert!(complete(&root, "404", None, day(2026, 7, 2)).is_err());
    }

    #[test]
    fn malformed_config_fails_every_verb() {
        let root = tempdir();
        fs::write(root.join(".taskmd.yaml"), "dir: [unclosed\n").unwrap();
        let mut e = FixedEntropy;
        assert!(create(&root, &spec("X"), day(2026, 7, 2), &mut e).is_err());
        assert!(list(&root, None).is_err());
        assert!(show(&root, "001").is_err());
        assert!(set_status(&root, "001", Status::Pending, day(2026, 7, 2)).is_err());
        assert!(complete(&root, "001", None, day(2026, 7, 2)).is_err());
    }

    #[test]
    fn create_dependency_check_propagates_scan_error() {
        // tasks dir is a file: the dependency-existence scan fails loudly.
        let root = tempdir();
        fs::remove_dir_all(root.join("tasks")).unwrap();
        fs::write(root.join("tasks"), "a file").unwrap();
        let s = CreateSpec {
            dependencies: vec!["001".into()],
            ..spec("X")
        };
        assert!(create(&root, &s, day(2026, 7, 2), &mut FixedEntropy).is_err());
    }

    #[test]
    fn create_propagates_store_error() {
        // Every reachable sequential id is occupied by a file the scanner
        // cannot see: the store gives up and create surfaces it.
        let root = tempdir();
        for n in 1..=16 {
            fs::write(root.join("tasks").join(format!("{n:03}-x.md")), "").unwrap();
        }
        let err = create(&root, &spec("X"), day(2026, 7, 2), &mut FixedEntropy).unwrap_err();
        assert!(err.to_string().contains("16 attempts"));
    }
}
