//! Read-only aggregate views over a vault (#014): the dependency `graph` export
//! and computed `stats`. Both are thin layers over the #008 model + graph and
//! render through the shared #011 serializer, so `--json`/`--yaml` come for free.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::taskmd::{Graph, Task};

/// One node of the exported dependency graph.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub title: String,
    pub status: String,
    /// All dependencies completed.
    pub ready: bool,
    /// Ids this task depends on.
    pub dependencies: Vec<String>,
    /// Open dependencies currently blocking it.
    pub blockers: Vec<String>,
}

/// The dependency graph as serializable data (for `--json`/`--yaml`); the human
/// format renders it as Graphviz DOT via [`to_dot`].
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphView {
    pub nodes: Vec<GraphNode>,
}

impl GraphView {
    pub fn build(tasks: &[Task], graph: &Graph) -> GraphView {
        let nodes = tasks
            .iter()
            .map(|t| GraphNode {
                id: t.id(),
                title: t.title(),
                status: t.effective_status().as_str().to_string(),
                ready: graph.is_ready(t),
                dependencies: t.dependencies(),
                blockers: graph.blockers(t).iter().map(|d| d.id()).collect(),
            })
            .collect();
        GraphView { nodes }
    }
}

/// Escape a string for a DOT double-quoted label.
fn dot_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Render the graph as Graphviz DOT: an edge points from each dependency to the
/// task that needs it (dependency -> dependent), so arrows follow the flow of
/// unblocking.
pub fn to_dot(view: &GraphView) -> String {
    let mut out = String::from("digraph tasks {\n  rankdir=LR;\n");
    for n in &view.nodes {
        out.push_str(&format!(
            "  \"{}\" [label=\"{}: {}\"];\n",
            dot_escape(&n.id),
            dot_escape(&n.id),
            dot_escape(&n.title)
        ));
    }
    for n in &view.nodes {
        for dep in &n.dependencies {
            out.push_str(&format!(
                "  \"{}\" -> \"{}\";\n",
                dot_escape(dep),
                dot_escape(&n.id)
            ));
        }
    }
    out.push('}');
    out
}

/// Computed vault metrics.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StatsView {
    pub total: usize,
    /// Count per (effective) status.
    pub by_status: BTreeMap<String, usize>,
    /// Count per (effective) priority.
    pub by_priority: BTreeMap<String, usize>,
    /// Count per phase; tasks with no phase are grouped under `(none)`.
    pub by_phase: BTreeMap<String, usize>,
    /// Actionable (all deps completed) tasks that are not themselves terminal.
    pub ready: usize,
    /// Non-terminal tasks with at least one open dependency.
    pub blocked: usize,
    /// Broken task-like files found while scanning.
    pub invalid: usize,
}

impl StatsView {
    pub fn build(tasks: &[Task], graph: &Graph, invalid: usize) -> StatsView {
        let mut by_status = BTreeMap::new();
        let mut by_priority = BTreeMap::new();
        let mut by_phase = BTreeMap::new();
        let mut ready = 0;
        let mut blocked = 0;
        for t in tasks {
            *by_status
                .entry(t.effective_status().as_str().to_string())
                .or_insert(0) += 1;
            *by_priority
                .entry(t.effective_priority().as_str().to_string())
                .or_insert(0) += 1;
            *by_phase
                .entry(t.phase().unwrap_or_else(|| "(none)".to_string()))
                .or_insert(0) += 1;
            // Readiness/blocked only make sense for not-yet-finished work.
            if !t.effective_status().is_terminal() {
                if graph.is_ready(t) {
                    ready += 1;
                } else {
                    blocked += 1;
                }
            }
        }
        StatsView {
            total: tasks.len(),
            by_status,
            by_priority,
            by_phase,
            ready,
            blocked,
            invalid,
        }
    }
}

/// Human-readable stats summary.
pub fn render_stats(s: &StatsView) -> String {
    let section = |title: &str, map: &BTreeMap<String, usize>| {
        let mut lines = format!("{title}:");
        for (k, v) in map {
            lines.push_str(&format!("\n  {k}: {v}"));
        }
        lines
    };
    let mut out = format!("{} task(s)", s.total);
    if s.invalid > 0 {
        out.push_str(&format!(" ({} broken file(s))", s.invalid));
    }
    out.push('\n');
    out.push_str(&section("by status", &s.by_status));
    out.push('\n');
    out.push_str(&section("by priority", &s.by_priority));
    out.push('\n');
    out.push_str(&section("by phase", &s.by_phase));
    out.push_str(&format!("\nready: {}\nblocked: {}", s.ready, s.blocked));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskmd::Task;

    fn task(yaml: &str) -> Task {
        Task::parse_required(&format!("---\n{yaml}\n---\n\n# body\n")).unwrap()
    }

    #[test]
    fn graph_view_maps_nodes_edges_and_readiness() {
        let tasks = vec![
            task("id: \"001\"\ntitle: Dep\nstatus: completed"),
            task("id: \"002\"\ntitle: Mid\nstatus: pending\ndependencies: [\"001\"]"),
            task("id: \"003\"\ntitle: Late\nstatus: pending\ndependencies: [\"002\"]"),
        ];
        let graph = Graph::build(&tasks);
        let view = GraphView::build(&tasks, &graph);
        assert_eq!(view.nodes.len(), 3);
        // 002's dep is completed → ready, no blockers.
        let mid = view.nodes.iter().find(|n| n.id == "002").unwrap();
        assert!(mid.ready);
        assert!(mid.blockers.is_empty());
        assert_eq!(mid.dependencies, vec!["001"]);
        // 003 waits on the still-open 002.
        let late = view.nodes.iter().find(|n| n.id == "003").unwrap();
        assert!(!late.ready);
        assert_eq!(late.blockers, vec!["002"]);
    }

    #[test]
    fn dot_export_has_nodes_and_edges_and_escapes() {
        let tasks = vec![
            task("id: \"001\"\ntitle: 'A \"quoted\" one'\nstatus: pending"),
            task("id: \"002\"\ntitle: B\nstatus: pending\ndependencies: [\"001\"]"),
        ];
        let graph = Graph::build(&tasks);
        let dot = to_dot(&GraphView::build(&tasks, &graph));
        assert!(dot.starts_with("digraph tasks {"));
        assert!(dot.contains("\"001\" -> \"002\";"));
        // The embedded quotes are escaped.
        assert!(dot.contains("\\\"quoted\\\""));
        assert!(dot.trim_end().ends_with('}'));
    }

    #[test]
    fn dot_escapes_backslashes() {
        let tasks = vec![task("id: \"001\"\ntitle: 'a\\\\b'\nstatus: pending")];
        let graph = Graph::build(&tasks);
        let dot = to_dot(&GraphView::build(&tasks, &graph));
        assert!(dot.contains("\\\\"));
    }

    #[test]
    fn stats_counts_and_groups() {
        let tasks = vec![
            task("id: \"001\"\ntitle: A\nstatus: completed\npriority: high\nphase: v1"),
            task("id: \"002\"\ntitle: B\nstatus: pending\npriority: high\ndependencies: [\"001\"]"),
            task("id: \"003\"\ntitle: C\nstatus: pending\ndependencies: [\"002\"]"),
            task("id: \"004\"\ntitle: D"), // no status/priority/phase → defaults
        ];
        let graph = Graph::build(&tasks);
        let s = StatsView::build(&tasks, &graph, 2);
        assert_eq!(s.total, 4);
        assert_eq!(s.invalid, 2);
        assert_eq!(s.by_status["pending"], 3);
        assert_eq!(s.by_status["completed"], 1);
        assert_eq!(s.by_priority["high"], 2);
        assert_eq!(s.by_priority["medium"], 2); // 003 + 004 default
        assert_eq!(s.by_phase["v1"], 1);
        assert_eq!(s.by_phase["(none)"], 3);
        // 002 ready (dep completed); 003 blocked (dep open); 004 ready (no deps);
        // 001 terminal → neither.
        assert_eq!(s.ready, 2);
        assert_eq!(s.blocked, 1);
    }

    #[test]
    fn render_stats_is_readable() {
        let tasks = vec![task("id: \"001\"\ntitle: A\nstatus: pending")];
        let graph = Graph::build(&tasks);
        let out = render_stats(&StatsView::build(&tasks, &graph, 1));
        assert!(out.contains("1 task(s)"));
        assert!(out.contains("broken file(s)"));
        assert!(out.contains("by status:"));
        assert!(out.contains("ready: 1"));
    }

    #[test]
    fn render_stats_without_invalid_omits_broken_note() {
        let tasks = vec![task("id: \"001\"\ntitle: A\nstatus: pending")];
        let graph = Graph::build(&tasks);
        let out = render_stats(&StatsView::build(&tasks, &graph, 0));
        assert!(!out.contains("broken"));
    }
}
