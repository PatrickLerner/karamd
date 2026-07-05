//! Vault linting against the taskmd spec.
//!
//! Errors are files taskmd would reject (broken frontmatter, missing
//! id/title, invalid enums, duplicate ids, dangling or cyclic references);
//! warnings are things that work but smell (unconfigured phase/scope, missing
//! `created_at`, off-convention filename). Exit codes match taskmd: 0 clean,
//! 1 errors, 2 warnings under `--strict`.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use chrono::NaiveDate;
use serde::Serialize;

use crate::taskmd::{Effort, Graph, GraphIssue, Priority, Status, Task, TaskType, Vault};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// One problem, anchored to a file and (when known) a task id.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    pub message: String,
}

/// Everything one validation run found.
#[derive(Debug, PartialEq, Serialize)]
pub struct Report {
    /// Number of parseable tasks checked.
    pub checked: usize,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count()
    }

    pub fn warnings(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count()
    }

    /// taskmd-compatible exit code: 1 on errors, 2 on warnings under
    /// `--strict`, 0 otherwise.
    pub fn exit_code(&self, strict: bool) -> u8 {
        if self.errors() > 0 {
            1
        } else if strict && self.warnings() > 0 {
            2
        } else {
            0
        }
    }
}

fn file_of(task: &Task) -> Option<String> {
    task.rel_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Lint a vault. Non-task files (docs, templates, fenced yaml examples) are
/// already excluded by the scanner and never flagged.
pub fn validate(root: &Path) -> Result<Report> {
    let vault = Vault::open(root)?;
    let scan = vault.scan()?;
    let mut findings = Vec::new();

    // Broken task-like files (malformed yaml, missing id/title).
    for invalid in &scan.invalid {
        findings.push(Finding {
            severity: Severity::Error,
            file: Some(invalid.rel_path.to_string_lossy().into_owned()),
            task: None,
            message: invalid.reason.clone(),
        });
    }

    // Per-task field checks.
    for task in &scan.tasks {
        let err = |message: String| Finding {
            severity: Severity::Error,
            file: file_of(task),
            task: Some(task.id()),
            message,
        };
        let warn = |message: String| Finding {
            severity: Severity::Warning,
            file: file_of(task),
            task: Some(task.id()),
            message,
        };

        if let Some(raw) = task.status_raw()
            && Status::parse(&raw).is_none()
        {
            findings.push(err(format!(
                "invalid status `{raw}` (pending, in-progress, in-review, completed, blocked, cancelled)"
            )));
        }
        if let Some(raw) = task.priority_raw()
            && Priority::parse(&raw).is_none()
        {
            findings.push(err(format!(
                "invalid priority `{raw}` (low, medium, high, critical)"
            )));
        }
        if let Some(raw) = task.effort_raw()
            && Effort::parse(&raw).is_none()
        {
            findings.push(err(format!(
                "invalid effort `{raw}` (small, medium, large)"
            )));
        }
        if let Some(raw) = task.task_type_raw()
            && TaskType::parse(&raw).is_none()
        {
            findings.push(err(format!(
                "invalid type `{raw}` (feature, bug, improvement, chore, docs)"
            )));
        }
        if let Some(raw) = task.due_raw()
            && NaiveDate::parse_from_str(&raw, "%Y-%m-%d").is_err()
        {
            findings.push(err(format!("invalid due `{raw}` (need YYYY-MM-DD)")));
        }

        // Warnings.
        if !vault.config.phases.is_empty()
            && let Some(phase) = task.phase()
            && vault.config.phase_index(&phase).is_none()
        {
            findings.push(warn(format!("phase `{phase}` is not configured")));
        }
        if !vault.config.scopes.is_empty() {
            for touch in task.touches() {
                if !vault.config.scopes.contains_key(&touch) {
                    findings.push(warn(format!("touches unknown scope `{touch}`")));
                }
            }
        }
        if task.created_at().is_none() {
            findings.push(warn("missing created_at".to_string()));
        }
        // Scanned tasks always carry a rel_path; chained Options keep this a
        // single expression either way.
        let name = task
            .rel_path
            .as_deref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !name.starts_with(&format!("{}-", task.id())) {
            findings.push(warn(format!(
                "filename `{name}` does not follow `{}-<slug>.md`",
                task.id()
            )));
        }
    }

    // Duplicate ids across files.
    let mut by_id: HashMap<String, Vec<String>> = HashMap::new();
    for task in &scan.tasks {
        by_id
            .entry(task.id())
            .or_default()
            .push(file_of(task).unwrap_or_default());
    }
    let mut dups: Vec<(&String, &Vec<String>)> =
        by_id.iter().filter(|(_, files)| files.len() > 1).collect();
    dups.sort_by_key(|(id, _)| (*id).clone());
    for (id, files) in dups {
        findings.push(Finding {
            severity: Severity::Error,
            file: None,
            task: Some(id.clone()),
            message: format!("duplicate id in {}", files.join(", ")),
        });
    }

    // Reference and cycle defects.
    let graph = Graph::build(&scan.tasks);
    for issue in graph.issues() {
        let (task, message) = match issue {
            GraphIssue::DanglingDependency { task, missing } => {
                (Some(task), format!("dependency `{missing}` does not exist"))
            }
            GraphIssue::DependencyCycle { ids } => {
                (None, format!("dependency cycle: {}", ids.join(" -> ")))
            }
            GraphIssue::MissingParent { task, parent } => {
                (Some(task), format!("parent `{parent}` does not exist"))
            }
            GraphIssue::SelfParent { task } => (Some(task), "task is its own parent".to_string()),
            GraphIssue::ParentCycle { ids } => {
                (None, format!("parent cycle: {}", ids.join(" -> ")))
            }
        };
        let file = task
            .as_deref()
            .and_then(|id| graph.get(id))
            .and_then(file_of);
        findings.push(Finding {
            severity: Severity::Error,
            file,
            task,
            message,
        });
    }

    Ok(Report {
        checked: scan.tasks.len(),
        findings,
    })
}

/// Human rendering: one line per finding plus a summary.
pub fn render_human(report: &Report) -> String {
    let mut out = String::new();
    for f in &report.findings {
        let severity = match f.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let mut place = f.file.clone().unwrap_or_default();
        if let Some(task) = &f.task {
            if place.is_empty() {
                place = format!("task {task}");
            } else {
                place = format!("{place} (task {task})");
            }
        }
        out.push_str(&format!("{severity}: {place}: {}\n", f.message));
    }
    out.push_str(&format!(
        "{} task(s) checked: {} error(s), {} warning(s)",
        report.checked,
        report.errors(),
        report.warnings()
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("karamd-val-{uniq}"));
        fs::create_dir_all(base.join("tasks")).unwrap();
        base
    }

    fn write(root: &Path, rel: &str, content: &str) {
        fs::write(root.join("tasks").join(rel), content).unwrap();
    }

    fn messages(report: &Report, severity: Severity) -> Vec<String> {
        report
            .findings
            .iter()
            .filter(|f| f.severity == severity)
            .map(|f| f.message.clone())
            .collect()
    }

    #[test]
    fn clean_vault_reports_nothing() {
        let root = tempdir();
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nstatus: pending\ncreated_at: 2026-07-01\n---\n",
        );
        let r = validate(&root).unwrap();
        assert_eq!(r.checked, 1);
        assert!(r.findings.is_empty());
        assert_eq!(r.exit_code(false), 0);
        assert_eq!(r.exit_code(true), 0);
    }

    #[test]
    fn broken_files_are_errors() {
        let root = tempdir();
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\nstatus: pending\n---\n",
        ); // no title
        write(
            &root,
            "002-b.md",
            "---\nid: \"002\"\ntitle: [unclosed\n---\n",
        ); // bad yaml
        let r = validate(&root).unwrap();
        assert_eq!(r.errors(), 2);
        assert_eq!(r.checked, 0);
        assert_eq!(r.exit_code(false), 1);
    }

    #[test]
    fn invalid_enums_are_errors() {
        let root = tempdir();
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nstatus: done\npriority: urgent\neffort: epic\ntype: story\ncreated_at: 2026-07-01\n---\n",
        );
        let r = validate(&root).unwrap();
        let errs = messages(&r, Severity::Error);
        assert_eq!(errs.len(), 4);
        assert!(errs[0].contains("invalid status `done`"));
        assert!(errs[1].contains("invalid priority `urgent`"));
        assert!(errs[2].contains("invalid effort `epic`"));
        assert!(errs[3].contains("invalid type `story`"));
    }

    #[test]
    fn invalid_due_is_an_error() {
        let root = tempdir();
        // 001 has a malformed due (error); 002 has a well-formed due (clean).
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\ndue: someday\ncreated_at: 2026-07-01\n---\n",
        );
        write(
            &root,
            "002-b.md",
            "---\nid: \"002\"\ntitle: B\ndue: 2026-08-01\ncreated_at: 2026-07-01\n---\n",
        );
        let errs = messages(&validate(&root).unwrap(), Severity::Error);
        assert!(errs.iter().any(|m| m.contains("invalid due `someday`")));
        // The well-formed date never produces a due error.
        assert!(!errs.iter().any(|m| m.contains("invalid due `2026-08-01`")));
    }

    #[test]
    fn duplicate_ids_are_errors() {
        let root = tempdir();
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\ncreated_at: 2026-07-01\n---\n",
        );
        write(
            &root,
            "001-b.md",
            "---\nid: \"001\"\ntitle: B\ncreated_at: 2026-07-01\n---\n",
        );
        // A second duplicated id proves the findings are ordered by id.
        write(
            &root,
            "002-c.md",
            "---\nid: \"002\"\ntitle: C\ncreated_at: 2026-07-01\n---\n",
        );
        write(
            &root,
            "002-d.md",
            "---\nid: \"002\"\ntitle: D\ncreated_at: 2026-07-01\n---\n",
        );
        let r = validate(&root).unwrap();
        let errs = messages(&r, Severity::Error);
        assert_eq!(errs.len(), 2);
        assert!(errs[0].contains("duplicate id"));
        assert!(errs[0].contains("001-a.md"));
        assert!(errs[0].contains("001-b.md"));
        assert!(errs[1].contains("002-c.md"));
    }

    #[test]
    fn reference_defects_are_errors() {
        let root = tempdir();
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\ndependencies: [\"404\"]\nparent: \"001\"\ncreated_at: 2026-07-01\n---\n",
        );
        write(
            &root,
            "002-b.md",
            "---\nid: \"002\"\ntitle: B\ndependencies: [\"003\"]\ncreated_at: 2026-07-01\n---\n",
        );
        write(
            &root,
            "003-c.md",
            "---\nid: \"003\"\ntitle: C\ndependencies: [\"002\"]\nparent: \"404\"\ncreated_at: 2026-07-01\n---\n",
        );
        let r = validate(&root).unwrap();
        let errs = messages(&r, Severity::Error);
        assert!(
            errs.iter()
                .any(|m| m.contains("dependency `404` does not exist"))
        );
        assert!(
            errs.iter()
                .any(|m| m.contains("dependency cycle: 002 -> 003"))
        );
        assert!(errs.iter().any(|m| m.contains("task is its own parent")));
        assert!(
            errs.iter()
                .any(|m| m.contains("parent `404` does not exist"))
        );
    }

    #[test]
    fn parent_cycle_is_an_error() {
        let root = tempdir();
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nparent: \"002\"\ncreated_at: 2026-07-01\n---\n",
        );
        write(
            &root,
            "002-b.md",
            "---\nid: \"002\"\ntitle: B\nparent: \"001\"\ncreated_at: 2026-07-01\n---\n",
        );
        let r = validate(&root).unwrap();
        assert!(
            messages(&r, Severity::Error)
                .iter()
                .any(|m| m.contains("parent cycle"))
        );
    }

    #[test]
    fn unconfigured_phase_and_scope_warn_only_when_configured() {
        let root = tempdir();
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nphase: ghost\ntouches: [x/y]\ncreated_at: 2026-07-01\n---\n",
        );
        // Without phases/scopes config: accepted silently.
        let r = validate(&root).unwrap();
        assert!(r.findings.is_empty());

        fs::write(
            root.join(".taskmd.yaml"),
            "phases:\n  - id: v1\n    name: V1\nscopes:\n  cli/core:\n    paths: []\n",
        )
        .unwrap();
        let r = validate(&root).unwrap();
        let warns = messages(&r, Severity::Warning);
        assert_eq!(r.errors(), 0);
        assert!(warns.iter().any(|m| m.contains("phase `ghost`")));
        assert!(warns.iter().any(|m| m.contains("unknown scope `x/y`")));
        // Warnings gate the exit code only under --strict.
        assert_eq!(r.exit_code(false), 0);
        assert_eq!(r.exit_code(true), 2);
    }

    #[test]
    fn configured_phase_and_scope_do_not_warn() {
        let root = tempdir();
        fs::write(
            root.join(".taskmd.yaml"),
            "phases:\n  - id: v1\n    name: V1\nscopes:\n  cli/core:\n    paths: []\n",
        )
        .unwrap();
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nphase: v1\ntouches: [cli/core]\ncreated_at: 2026-07-01\n---\n",
        );
        assert!(validate(&root).unwrap().findings.is_empty());
    }

    #[test]
    fn missing_created_at_and_bad_filename_warn() {
        let root = tempdir();
        write(&root, "999-wrong.md", "---\nid: \"001\"\ntitle: A\n---\n");
        let r = validate(&root).unwrap();
        let warns = messages(&r, Severity::Warning);
        assert!(warns.iter().any(|m| m.contains("missing created_at")));
        assert!(
            warns
                .iter()
                .any(|m| m.contains("`999-wrong.md` does not follow `001-<slug>.md`"))
        );
    }

    #[test]
    fn non_task_files_are_never_flagged() {
        let root = tempdir();
        write(&root, "README.md", "# docs, no frontmatter\n");
        write(
            &root,
            "SPEC.md",
            "# Spec\n\n```yaml\n---\nid: \"9\"\ntitle: X\n---\n```\n",
        );
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\ncreated_at: 2026-07-01\n---\n",
        );
        let r = validate(&root).unwrap();
        assert_eq!(r.checked, 1);
        assert!(r.findings.is_empty());
    }

    #[test]
    fn open_error_propagates() {
        let root = tempdir();
        fs::write(root.join(".taskmd.yaml"), "dir: [broken\n").unwrap();
        assert!(validate(&root).is_err());
    }

    #[test]
    fn scan_error_propagates() {
        let root = tempdir();
        fs::write(root.join("tasks/001-bin.md"), [0xFF, 0xFE, 0x00, 0x9F]).unwrap();
        assert!(validate(&root).is_err());
    }

    #[test]
    fn human_rendering_formats_findings() {
        let root = tempdir();
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\nstatus: done\n---\n",
        );
        write(
            &root,
            "002-b.md",
            "---\nid: \"002\"\nstatus: pending\n---\n",
        );
        let r = validate(&root).unwrap();
        let out = render_human(&r);
        assert!(out.contains("error: 002-b.md: missing required field `title`"));
        assert!(out.contains("error: 001-a.md (task 001): invalid status `done`"));
        assert!(out.contains("warning: 001-a.md (task 001): missing created_at"));
        assert!(out.contains("1 task(s) checked:"));
        assert!(out.contains("error(s)"));
    }

    #[test]
    fn human_rendering_task_without_file() {
        // Duplicate-id findings carry no single file; the task id is the place.
        let root = tempdir();
        write(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: A\ncreated_at: 2026-07-01\n---\n",
        );
        write(
            &root,
            "001-b.md",
            "---\nid: \"001\"\ntitle: B\ncreated_at: 2026-07-01\n---\n",
        );
        let out = render_human(&validate(&root).unwrap());
        assert!(out.contains("error: task 001: duplicate id"));
    }
}
