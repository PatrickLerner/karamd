//! One serializable task view backing all output formats: the human table,
//! `--json`, and `--yaml` render from the same [`TaskView`] so they can never
//! drift apart.

use anyhow::Result;
use serde::Serialize;

use crate::taskmd::{Graph, Task};

/// Output format selector shared by every reading command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Human,
    Json,
    Yaml,
}

impl Format {
    /// Combine the `--json` / `--yaml` flags (clap enforces exclusivity).
    pub fn from_flags(json: bool, yaml: bool) -> Format {
        match (json, yaml) {
            (true, _) => Format::Json,
            (_, true) => Format::Yaml,
            _ => Format::Human,
        }
    }
}

/// The serializable slice of a task every output shares. Field names match
/// the frontmatter so machine consumers see familiar keys.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskView {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// Target date (`YYYY-MM-DD`), verbatim; may be malformed (validate flags it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    pub tags: Vec<String>,
    pub dependencies: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancelled_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurring: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// All dependencies `completed` (from the graph).
    pub ready: bool,
    /// Ids of the open dependencies blocking this task.
    pub blockers: Vec<String>,
    /// Markdown body; only populated for detail views.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

impl TaskView {
    /// Build a view for one task. `graph` supplies readiness and blockers;
    /// `with_body` is for detail consumers (single task, web API).
    pub fn build(task: &Task, graph: &Graph, with_body: bool) -> TaskView {
        let date = |d: Option<chrono::NaiveDate>| d.map(|d| d.format("%Y-%m-%d").to_string());
        TaskView {
            id: task.id(),
            title: task.title(),
            // Raw-but-invalid enum values still show as written; effective
            // defaults fill true absences. `validate` reports the invalid.
            status: task
                .status_raw()
                .unwrap_or_else(|| task.effective_status().as_str().to_string()),
            priority: task
                .priority_raw()
                .unwrap_or_else(|| task.effective_priority().as_str().to_string()),
            effort: task.effort_raw(),
            task_type: task.task_type_raw(),
            phase: task.phase(),
            due: task.due_raw(),
            tags: task.tags(),
            dependencies: task.dependencies(),
            group: task.group(),
            owner: task.owner(),
            parent: task.parent(),
            created_at: date(task.created_at()),
            completed_at: date(task.completed_at()),
            cancelled_at: date(task.cancelled_at()),
            recurring: task.recurring(),
            file: task
                .rel_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            ready: graph.is_ready(task),
            blockers: graph.blockers(task).iter().map(|t| t.id()).collect(),
            body: with_body.then(|| task.body.trim().to_string()),
        }
    }
}

/// Pretty JSON for any serializable payload. Kept as single-expression
/// generics (instantiated per payload type) so every instantiation is fully
/// exercised by whichever call reaches it.
pub fn to_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string_pretty(value)?)
}

/// YAML for any serializable payload.
pub fn to_yaml<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_norway::to_string(value)?)
}

/// The human-readable task table (list, query results).
pub fn task_table(views: &[TaskView]) -> String {
    if views.is_empty() {
        return "no tasks match".to_string();
    }
    let headers = ["id", "status", "prio", "phase", "title"];
    let rows: Vec<[String; 5]> = views
        .iter()
        .map(|v| {
            let title = if v.ready || v.blockers.is_empty() {
                v.title.clone()
            } else {
                format!("{} [waits on {}]", v.title, v.blockers.join(", "))
            };
            [
                v.id.clone(),
                v.status.clone(),
                v.priority.clone(),
                v.phase.clone().unwrap_or_default(),
                title,
            ]
        })
        .collect();
    let mut widths = headers.map(str::len);
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row.iter()) {
            *w = (*w).max(cell.len());
        }
    }
    let fmt_row = |cells: [&str; 5]| {
        let mut line = String::new();
        for (i, (cell, w)) in cells.iter().zip(widths.iter()).enumerate() {
            if i > 0 {
                line.push_str("  ");
            }
            if i == cells.len() - 1 {
                line.push_str(cell); // last column unpadded
            } else {
                line.push_str(&format!("{cell:<w$}"));
            }
        }
        line
    };
    let mut out = fmt_row(headers);
    out.push('\n');
    out.push_str(&fmt_row(["--", "------", "----", "-----", "-----"]));
    for row in &rows {
        out.push('\n');
        out.push_str(&fmt_row([&row[0], &row[1], &row[2], &row[3], &row[4]]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskmd::Task;
    use chrono::NaiveDate;

    fn task(yaml: &str) -> Task {
        Task::parse_required(&format!("---\n{yaml}\n---\n\n# body here\n")).unwrap()
    }

    #[test]
    fn format_from_flags() {
        assert_eq!(Format::from_flags(false, false), Format::Human);
        assert_eq!(Format::from_flags(true, false), Format::Json);
        assert_eq!(Format::from_flags(false, true), Format::Yaml);
    }

    #[test]
    fn view_fills_defaults_and_graph_state() {
        let tasks = vec![
            task("id: \"001\"\ntitle: Dep\nstatus: in-progress"),
            task("id: \"002\"\ntitle: Blocked one\ndependencies: [\"001\"]"),
        ];
        let graph = Graph::build(&tasks);
        let v = TaskView::build(&tasks[1], &graph, false);
        assert_eq!(v.id, "002");
        assert_eq!(v.status, "pending"); // effective default
        assert_eq!(v.priority, "medium"); // effective default
        assert!(!v.ready);
        assert_eq!(v.blockers, vec!["001"]);
        assert_eq!(v.body, None);
        let detail = TaskView::build(&tasks[1], &graph, true);
        assert_eq!(detail.body.as_deref(), Some("# body here"));
    }

    #[test]
    fn view_shows_invalid_raw_enums_verbatim() {
        let tasks = vec![task(
            "id: \"001\"\ntitle: T\nstatus: done\npriority: urgent",
        )];
        let graph = Graph::build(&tasks);
        let v = TaskView::build(&tasks[0], &graph, false);
        assert_eq!(v.status, "done");
        assert_eq!(v.priority, "urgent");
    }

    #[test]
    fn machine_json_and_yaml_render() {
        let tasks = vec![task(
            "id: \"001\"\ntitle: T\nstatus: pending\ntags: [a]\ncreated_at: 2026-07-01",
        )];
        let graph = Graph::build(&tasks);
        let views = vec![TaskView::build(&tasks[0], &graph, false)];
        let json = to_json(&views).unwrap();
        assert!(json.contains("\"id\": \"001\""));
        assert!(json.contains("\"ready\": true"));
        assert!(json.contains("\"created_at\": \"2026-07-01\""));
        // Absent optionals are omitted, not null.
        assert!(!json.contains("effort"));
        assert!(!json.contains("body"));
        let yaml = to_yaml(&views).unwrap();
        assert!(yaml.contains("id: '001'"));
        assert!(yaml.contains("ready: true"));
    }

    #[test]
    fn table_lists_columns_and_blockers() {
        let tasks = vec![
            task("id: \"001\"\ntitle: First\nstatus: in-progress\npriority: high\nphase: v1"),
            task("id: \"002\"\ntitle: Second\ndependencies: [\"001\"]"),
        ];
        let graph = Graph::build(&tasks);
        let views: Vec<TaskView> = tasks
            .iter()
            .map(|t| TaskView::build(t, &graph, false))
            .collect();
        let table = task_table(&views);
        assert!(table.starts_with("id "));
        assert!(table.contains("in-progress"));
        assert!(table.contains("First"));
        assert!(table.contains("Second [waits on 001]"));
    }

    #[test]
    fn table_empty_case() {
        assert_eq!(task_table(&[]), "no tasks match");
    }

    #[test]
    fn view_maps_every_field() {
        let mut t = task(
            "id: \"009\"\ntitle: Full\nstatus: completed\npriority: low\neffort: small\ntype: docs\nphase: v2\ndue: 2026-09-09\ntags: [x]\ndependencies: []\ngroup: g\nowner: o\nparent: \"001\"\ncreated_at: 2026-01-01\ncompleted_at: 2026-02-02\nrecurring: \"k\"",
        );
        t.rel_path = Some(std::path::PathBuf::from("009-full.md"));
        let binding = vec![t];
        let graph = Graph::build(&binding);
        let v = TaskView::build(&binding[0], &graph, false);
        assert_eq!(v.effort.as_deref(), Some("small"));
        assert_eq!(v.task_type.as_deref(), Some("docs"));
        assert_eq!(v.phase.as_deref(), Some("v2"));
        assert_eq!(v.due.as_deref(), Some("2026-09-09"));
        assert_eq!(v.group.as_deref(), Some("g"));
        assert_eq!(v.owner.as_deref(), Some("o"));
        assert_eq!(v.parent.as_deref(), Some("001"));
        assert_eq!(v.completed_at.as_deref(), Some("2026-02-02"));
        assert_eq!(v.recurring.as_deref(), Some("k"));
        assert_eq!(v.file.as_deref(), Some("009-full.md"));
        let _ = NaiveDate::from_ymd_opt(2026, 1, 1); // keep chrono import honest
    }

    #[test]
    fn cancelled_at_is_mapped() {
        let tasks = vec![task(
            "id: \"001\"\ntitle: T\nstatus: cancelled\ncancelled_at: 2026-03-03",
        )];
        let graph = Graph::build(&tasks);
        let v = TaskView::build(&tasks[0], &graph, false);
        assert_eq!(v.cancelled_at.as_deref(), Some("2026-03-03"));
    }
}
