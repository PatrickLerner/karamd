//! Task templates for `create --template`.
//!
//! taskmd 0.2.5 ships three built-ins (`feature`, `bug`, `chore`); their
//! frontmatter defaults and bodies here are byte-matched against its actual
//! output. A custom template at `.taskmd/templates/<name>.md` (frontmatter =
//! field defaults, body = task body) takes precedence over a built-in of the
//! same name.

use std::fs;
use std::path::Path;

use anyhow::{Result, bail};

use super::model::{Priority, Task, TaskType, split_frontmatter};

/// Field defaults + body a template contributes to a new task.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub priority: Option<Priority>,
    pub task_type: Option<TaskType>,
    pub body: String,
}

const FEATURE_BODY: &str = "## Objective\n\n<!-- Describe the goal of this feature -->\n\n## Tasks\n\n- [ ] TODO\n\n## Acceptance Criteria\n\n- TODO";

const BUG_BODY: &str = "## Steps to Reproduce\n\n1. ...\n\n## Expected Behavior\n\n<!-- What should happen -->\n\n## Actual Behavior\n\n<!-- What actually happens -->\n\n## Environment\n\n- OS:\n- Version:";

const CHORE_BODY: &str =
    "## Description\n\n<!-- Describe the maintenance work needed -->\n\n## Tasks\n\n- [ ] TODO";

fn builtin(name: &str) -> Option<Template> {
    match name {
        "feature" => Some(Template {
            priority: Some(Priority::Medium),
            task_type: Some(TaskType::Feature),
            body: FEATURE_BODY.to_string(),
        }),
        "bug" => Some(Template {
            priority: Some(Priority::High),
            task_type: Some(TaskType::Bug),
            body: BUG_BODY.to_string(),
        }),
        "chore" => Some(Template {
            priority: Some(Priority::Low),
            task_type: Some(TaskType::Chore),
            body: CHORE_BODY.to_string(),
        }),
        _ => None,
    }
}

/// Parse a custom template file: optional frontmatter for field defaults
/// (`priority`, `type`), the rest is the body. A file without frontmatter is
/// all body.
fn parse_custom(raw: &str) -> Template {
    let (fm, body) = match split_frontmatter(raw) {
        Some((fm, body)) => (Some(fm), body),
        None => (None, raw),
    };
    let doc = fm
        .and_then(|f| serde_norway::from_str::<serde_norway::Value>(f).ok())
        .and_then(|v| v.as_mapping().cloned())
        .unwrap_or_default();
    let get = |k: &str| {
        doc.get(serde_norway::Value::String(k.to_string()))
            .and_then(|v| v.as_str().map(str::to_string))
    };
    Template {
        priority: get("priority").as_deref().and_then(Priority::parse),
        task_type: get("type").as_deref().and_then(TaskType::parse),
        body: body.trim().to_string(),
    }
}

/// Resolve a template by name for a vault: `.taskmd/templates/<name>.md`
/// first, then the built-ins. Unknown names error with the available set.
pub fn resolve(vault: &Path, name: &str) -> Result<Template> {
    let custom = vault.join(".taskmd/templates").join(format!("{name}.md"));
    if custom.is_file() {
        let raw = fs::read_to_string(&custom)?;
        return Ok(parse_custom(&raw));
    }
    match builtin(name) {
        Some(t) => Ok(t),
        None => bail!("unknown template `{name}` (built-ins: feature, bug, chore)"),
    }
}

/// Apply a template's defaults to a new task. Explicit values set *after*
/// this call (from CLI flags) override the template.
pub fn apply(template: &Template, task: &mut Task) {
    if let Some(p) = template.priority {
        task.set_priority(p);
    }
    if let Some(t) = template.task_type {
        task.set_task_type(t);
    }
    task.set_body(&template.body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::fs;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("karamd-tpl-{uniq}"));
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn builtins_match_taskmd() {
        let vault = tempdir();
        let feature = resolve(&vault, "feature").unwrap();
        assert_eq!(feature.priority, Some(Priority::Medium));
        assert_eq!(feature.task_type, Some(TaskType::Feature));
        assert!(feature.body.starts_with("## Objective"));
        assert!(feature.body.ends_with("- TODO"));

        let bug = resolve(&vault, "bug").unwrap();
        assert_eq!(bug.priority, Some(Priority::High));
        assert_eq!(bug.task_type, Some(TaskType::Bug));
        assert!(bug.body.starts_with("## Steps to Reproduce"));
        assert!(bug.body.ends_with("- Version:"));

        let chore = resolve(&vault, "chore").unwrap();
        assert_eq!(chore.priority, Some(Priority::Low));
        assert_eq!(chore.task_type, Some(TaskType::Chore));
        assert!(chore.body.starts_with("## Description"));
    }

    #[test]
    fn unknown_template_errors() {
        let err = resolve(&tempdir(), "epic").unwrap_err();
        assert!(err.to_string().contains("unknown template `epic`"));
    }

    #[test]
    fn custom_template_overrides_builtin() {
        let vault = tempdir();
        let dir = vault.join(".taskmd/templates");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("bug.md"),
            "---\npriority: critical\ntype: bug\n---\n\n## Custom repro\n\n- TODO\n",
        )
        .unwrap();
        let t = resolve(&vault, "bug").unwrap();
        assert_eq!(t.priority, Some(Priority::Critical));
        assert_eq!(t.task_type, Some(TaskType::Bug));
        assert_eq!(t.body, "## Custom repro\n\n- TODO");
    }

    #[test]
    fn custom_template_without_frontmatter_is_all_body() {
        let vault = tempdir();
        let dir = vault.join(".taskmd/templates");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plain.md"), "just a body\n").unwrap();
        let t = resolve(&vault, "plain").unwrap();
        assert_eq!(t.priority, None);
        assert_eq!(t.task_type, None);
        assert_eq!(t.body, "just a body");
    }

    #[test]
    fn custom_template_with_bad_frontmatter_still_yields_body() {
        let vault = tempdir();
        let dir = vault.join(".taskmd/templates");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("odd.md"), "---\n: : bad\n---\n\nbody text\n").unwrap();
        let t = resolve(&vault, "odd").unwrap();
        assert_eq!(t.priority, None);
        assert_eq!(t.body, "body text");
    }

    #[test]
    fn custom_template_unreadable_errors() {
        let vault = tempdir();
        let dir = vault.join(".taskmd/templates/broken.md");
        fs::create_dir_all(&dir).unwrap(); // a dir named like the file
        // is_file() is false for a dir, so this falls through to built-ins.
        assert!(resolve(&vault, "broken").is_err());
    }

    #[test]
    fn apply_sets_defaults_and_body() {
        let mut task = Task::new("001", "T", NaiveDate::from_ymd_opt(2026, 7, 2).unwrap());
        let vault = tempdir();
        let t = resolve(&vault, "bug").unwrap();
        apply(&t, &mut task);
        assert_eq!(task.priority(), Some(Priority::High));
        assert_eq!(task.task_type(), Some(TaskType::Bug));
        assert!(task.body.contains("## Steps to Reproduce"));
        // Explicit values set after apply win.
        task.set_priority(Priority::Low);
        assert_eq!(task.priority(), Some(Priority::Low));
    }

    #[test]
    fn apply_without_defaults_changes_only_body() {
        let mut task = Task::new("001", "T", NaiveDate::from_ymd_opt(2026, 7, 2).unwrap());
        let t = Template {
            priority: None,
            task_type: None,
            body: "b".into(),
        };
        apply(&t, &mut task);
        assert_eq!(task.priority(), None);
        assert_eq!(task.task_type(), None);
        assert_eq!(task.body, "\nb\n");
    }
}
