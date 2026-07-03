//! Dependency graph and parent hierarchy over a set of tasks.
//!
//! Dependencies are the backbone of `next`: a task is *ready* only when every
//! dependency is `completed` (taskmd's actionability rule — a `cancelled`
//! dependency does NOT satisfy it). `parent` is hierarchy only: no blocking,
//! no status cascade.

use std::collections::{HashMap, HashSet};

use super::model::{Status, Task};

/// A cycle or reference defect found in the graph.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphIssue {
    /// `task` depends on `missing`, which no task carries as id.
    DanglingDependency { task: String, missing: String },
    /// A dependency cycle, listed in walk order (first repeats implicitly).
    DependencyCycle { ids: Vec<String> },
    /// `task` names a `parent` that does not exist.
    MissingParent { task: String, parent: String },
    /// `task` is its own parent.
    SelfParent { task: String },
    /// A cycle in the parent chain, listed in walk order.
    ParentCycle { ids: Vec<String> },
}

/// Graph over borrowed tasks. Build once per scan; all queries are by id.
pub struct Graph<'a> {
    tasks: &'a [Task],
    by_id: HashMap<String, &'a Task>,
    /// Reverse dependency edges: id -> ids of tasks that depend on it.
    dependents: HashMap<String, Vec<String>>,
}

impl<'a> Graph<'a> {
    pub fn build(tasks: &'a [Task]) -> Graph<'a> {
        let mut by_id = HashMap::new();
        for t in tasks {
            // First file wins on duplicate ids; validate reports the defect.
            by_id.entry(t.id()).or_insert(t);
        }
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
        for t in tasks {
            for dep in t.dependencies() {
                dependents.entry(dep).or_default().push(t.id());
            }
        }
        Graph {
            tasks,
            by_id,
            dependents,
        }
    }

    pub fn get(&self, id: &str) -> Option<&'a Task> {
        self.by_id.get(id).copied()
    }

    /// Ready = every dependency exists and is `completed`. A dangling
    /// dependency makes a task unready (it can never be satisfied until the
    /// vault is fixed), and so does a `cancelled` one — matching taskmd, which
    /// only counts `completed`.
    pub fn is_ready(&self, task: &Task) -> bool {
        task.dependencies().iter().all(|dep| {
            self.get(dep)
                .is_some_and(|d| d.effective_status() == Status::Completed)
        })
    }

    /// The unmet dependencies blocking a task (missing ids are reported by
    /// [`Graph::issues`], not here).
    pub fn blockers(&self, task: &Task) -> Vec<&'a Task> {
        task.dependencies()
            .iter()
            .filter_map(|dep| self.get(dep))
            .filter(|d| d.effective_status() != Status::Completed)
            .collect()
    }

    /// Direct dependents of a task (tasks listing it in `dependencies`).
    pub fn direct_dependents(&self, id: &str) -> Vec<&'a Task> {
        self.dependents
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|d| self.get(d))
            .collect()
    }

    /// All tasks transitively depending on this one. Cycle-safe.
    pub fn downstream_ids(&self, id: &str) -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut stack = vec![id.to_string()];
        while let Some(cur) = stack.pop() {
            for dep in self.dependents.get(&cur).into_iter().flatten() {
                if seen.insert(dep.clone()) {
                    stack.push(dep.clone());
                }
            }
        }
        seen.remove(id); // a cycle could route back to the start
        seen
    }

    /// How many tasks this one transitively unblocks (taskmd's
    /// `downstream_count`).
    pub fn downstream_count(&self, id: &str) -> usize {
        self.downstream_ids(id).len()
    }

    /// Length of the longest dependent chain hanging off this task; > 0 means
    /// the task sits on a path other work is waiting behind (taskmd's
    /// "on critical path" reason). Cycle-safe.
    pub fn downstream_depth(&self, id: &str) -> usize {
        fn depth(
            g: &Graph,
            id: &str,
            visiting: &mut HashSet<String>,
            memo: &mut HashMap<String, usize>,
        ) -> usize {
            if let Some(&d) = memo.get(id) {
                return d;
            }
            if !visiting.insert(id.to_string()) {
                return 0; // cycle guard
            }
            let d = g
                .dependents
                .get(id)
                .into_iter()
                .flatten()
                .map(|dep| 1 + depth(g, dep, visiting, memo))
                .max()
                .unwrap_or(0);
            visiting.remove(id);
            memo.insert(id.to_string(), d);
            d
        }
        depth(self, id, &mut HashSet::new(), &mut HashMap::new())
    }

    /// Children of a task in the `parent` hierarchy (computed, not stored).
    pub fn children(&self, id: &str) -> Vec<&'a Task> {
        self.tasks
            .iter()
            .filter(|t| t.parent().as_deref() == Some(id))
            .collect()
    }

    /// All reference/cycle defects: dangling dependencies, dependency cycles,
    /// missing/self/cyclic parents. Deterministic order (by task id).
    pub fn issues(&self) -> Vec<GraphIssue> {
        let mut issues = Vec::new();
        let mut ordered: Vec<&Task> = self.tasks.iter().collect();
        ordered.sort_by_key(|a| a.id());

        for t in &ordered {
            for dep in t.dependencies() {
                if !self.by_id.contains_key(&dep) {
                    issues.push(GraphIssue::DanglingDependency {
                        task: t.id(),
                        missing: dep,
                    });
                }
            }
        }

        issues.extend(self.dependency_cycles());

        for t in &ordered {
            let Some(parent) = t.parent() else { continue };
            if parent == t.id() {
                issues.push(GraphIssue::SelfParent { task: t.id() });
            } else if !self.by_id.contains_key(&parent) {
                issues.push(GraphIssue::MissingParent {
                    task: t.id(),
                    parent,
                });
            }
        }

        issues.extend(self.parent_cycles());
        issues
    }

    /// Dependency cycles via DFS with an explicit recursion stack; each cycle
    /// is reported once (deduplicated by its id set).
    fn dependency_cycles(&self) -> Vec<GraphIssue> {
        let mut cycles: Vec<Vec<String>> = Vec::new();
        let mut done: HashSet<String> = HashSet::new();
        let mut ids: Vec<String> = self.by_id.keys().cloned().collect();
        ids.sort();

        for start in ids {
            if done.contains(&start) {
                continue;
            }
            let mut path: Vec<String> = Vec::new();
            self.dfs_cycles(&start, &mut path, &mut done, &mut cycles);
        }

        let mut seen_sets: HashSet<Vec<String>> = HashSet::new();
        cycles
            .into_iter()
            .filter(|c| {
                let mut key = c.clone();
                key.sort();
                seen_sets.insert(key)
            })
            .map(|ids| GraphIssue::DependencyCycle { ids })
            .collect()
    }

    fn dfs_cycles(
        &self,
        id: &str,
        path: &mut Vec<String>,
        done: &mut HashSet<String>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        if let Some(pos) = path.iter().position(|p| p == id) {
            cycles.push(path[pos..].to_vec());
            return;
        }
        if done.contains(id) {
            return;
        }
        path.push(id.to_string());
        // Callers only pass ids present in `by_id` (start ids come from its
        // keys; deps are filtered), so indexing cannot miss.
        for dep in self.by_id[id].dependencies() {
            if self.by_id.contains_key(&dep) {
                self.dfs_cycles(&dep, path, done, cycles);
            }
        }
        path.pop();
        done.insert(id.to_string());
    }

    /// Cycles in the parent chain (a walks-up-to-itself loop). Self-parents
    /// are reported separately.
    fn parent_cycles(&self) -> Vec<GraphIssue> {
        let mut reported: HashSet<Vec<String>> = HashSet::new();
        let mut out = Vec::new();
        let mut ids: Vec<String> = self.by_id.keys().cloned().collect();
        ids.sort();

        for start in ids {
            let mut chain: Vec<String> = vec![start.clone()];
            let mut cur = start;
            while let Some(parent) = self.by_id.get(&cur).and_then(|t| t.parent()) {
                if parent == cur {
                    break; // self-parent, reported elsewhere
                }
                if let Some(pos) = chain.iter().position(|c| *c == parent) {
                    let cycle = chain[pos..].to_vec();
                    let mut key = cycle.clone();
                    key.sort();
                    if reported.insert(key) {
                        out.push(GraphIssue::ParentCycle { ids: cycle });
                    }
                    break;
                }
                if !self.by_id.contains_key(&parent) {
                    break; // missing parent, reported elsewhere
                }
                chain.push(parent.clone());
                cur = parent;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskmd::model::Task;
    use chrono::NaiveDate;

    fn task(id: &str, deps: &[&str], status: &str) -> Task {
        let mut t = Task::new(
            id,
            &format!("Task {id}"),
            NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
        );
        t.set("status", serde_norway::Value::String(status.into()));
        if !deps.is_empty() {
            t.set_dependencies(&deps.iter().map(|d| d.to_string()).collect::<Vec<_>>());
        }
        t
    }

    fn with_parent(mut t: Task, parent: &str) -> Task {
        t.set_parent(Some(parent));
        t
    }

    #[test]
    fn readiness_requires_all_deps_completed() {
        let tasks = vec![
            task("001", &[], "completed"),
            task("002", &[], "pending"),
            task("003", &["001"], "pending"),
            task("004", &["001", "002"], "pending"),
        ];
        let g = Graph::build(&tasks);
        assert!(g.is_ready(g.get("002").unwrap())); // no deps
        assert!(g.is_ready(g.get("003").unwrap())); // dep completed
        assert!(!g.is_ready(g.get("004").unwrap())); // one dep open
    }

    #[test]
    fn cancelled_dependency_does_not_satisfy_readiness() {
        // taskmd counts only `completed`; a cancelled dep never completes.
        let tasks = vec![
            task("001", &[], "cancelled"),
            task("002", &["001"], "pending"),
        ];
        let g = Graph::build(&tasks);
        assert!(!g.is_ready(g.get("002").unwrap()));
        assert_eq!(g.blockers(g.get("002").unwrap()).len(), 1);
    }

    #[test]
    fn dangling_dependency_makes_task_unready() {
        let tasks = vec![task("002", &["404"], "pending")];
        let g = Graph::build(&tasks);
        assert!(!g.is_ready(g.get("002").unwrap()));
        // ...but the missing task is not a "blocker" (it does not exist).
        assert!(g.blockers(g.get("002").unwrap()).is_empty());
        assert_eq!(
            g.issues(),
            vec![GraphIssue::DanglingDependency {
                task: "002".into(),
                missing: "404".into(),
            }]
        );
    }

    #[test]
    fn blockers_lists_open_dependencies() {
        let tasks = vec![
            task("001", &[], "completed"),
            task("002", &[], "in-progress"),
            task("003", &[], "blocked"),
            task("004", &["001", "002", "003"], "pending"),
        ];
        let g = Graph::build(&tasks);
        let blockers = g.blockers(g.get("004").unwrap());
        let ids: Vec<String> = blockers.iter().map(|t| t.id()).collect();
        assert_eq!(ids, vec!["002", "003"]);
    }

    #[test]
    fn dependents_and_downstream_count_are_transitive() {
        // 001 <- 002 <- 003, and 001 <- 004
        let tasks = vec![
            task("001", &[], "pending"),
            task("002", &["001"], "pending"),
            task("003", &["002"], "pending"),
            task("004", &["001"], "pending"),
        ];
        let g = Graph::build(&tasks);
        let direct: Vec<String> = g.direct_dependents("001").iter().map(|t| t.id()).collect();
        assert_eq!(direct, vec!["002", "004"]);
        assert_eq!(g.downstream_count("001"), 3);
        assert_eq!(g.downstream_count("002"), 1);
        assert_eq!(g.downstream_count("003"), 0);
        assert_eq!(g.downstream_depth("001"), 2);
        assert_eq!(g.downstream_depth("003"), 0);
    }

    #[test]
    fn downstream_depth_memoizes_shared_paths() {
        // Diamond: 001 <- 002 <- 004 and 001 <- 003 <- 004; the second walk
        // to 004 hits the memo.
        let tasks = vec![
            task("001", &[], "pending"),
            task("002", &["001"], "pending"),
            task("003", &["001"], "pending"),
            task("004", &["002", "003"], "pending"),
        ];
        let g = Graph::build(&tasks);
        assert_eq!(g.downstream_depth("001"), 2);
        assert_eq!(g.downstream_count("001"), 3);
    }

    #[test]
    fn downstream_is_cycle_safe() {
        let tasks = vec![
            task("001", &["002"], "pending"),
            task("002", &["001"], "pending"),
        ];
        let g = Graph::build(&tasks);
        // Must terminate; 001's downstream reaches 002 (and itself, excluded).
        assert_eq!(g.downstream_count("001"), 1);
        let _ = g.downstream_depth("001");
    }

    #[test]
    fn dependency_cycle_detected_once() {
        let tasks = vec![
            task("001", &["002"], "pending"),
            task("002", &["003"], "pending"),
            task("003", &["001"], "pending"),
            task("004", &["001"], "pending"), // outside the cycle
        ];
        let g = Graph::build(&tasks);
        let cycles: Vec<GraphIssue> = g
            .issues()
            .into_iter()
            .filter(|i| matches!(i, GraphIssue::DependencyCycle { .. }))
            .collect();
        // Exactly one cycle, in deterministic walk order (DFS from the
        // smallest id).
        assert_eq!(
            cycles,
            vec![GraphIssue::DependencyCycle {
                ids: vec!["001".into(), "002".into(), "003".into()],
            }]
        );
    }

    #[test]
    fn self_dependency_is_a_cycle() {
        let tasks = vec![task("001", &["001"], "pending")];
        let g = Graph::build(&tasks);
        assert!(g.issues().iter().any(
            |i| matches!(i, GraphIssue::DependencyCycle { ids } if ids == &vec!["001".to_string()])
        ));
    }

    #[test]
    fn acyclic_graph_has_no_issues() {
        let tasks = vec![
            task("001", &[], "completed"),
            task("002", &["001"], "pending"),
            with_parent(task("003", &[], "pending"), "001"),
        ];
        let g = Graph::build(&tasks);
        assert!(g.issues().is_empty());
    }

    #[test]
    fn parent_issues_detected() {
        let tasks = vec![
            with_parent(task("001", &[], "pending"), "404"),
            with_parent(task("002", &[], "pending"), "002"),
            with_parent(task("003", &[], "pending"), "004"),
            with_parent(task("004", &[], "pending"), "003"),
        ];
        let g = Graph::build(&tasks);
        let issues = g.issues();
        assert!(issues.contains(&GraphIssue::MissingParent {
            task: "001".into(),
            parent: "404".into(),
        }));
        assert!(issues.contains(&GraphIssue::SelfParent { task: "002".into() }));
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, GraphIssue::ParentCycle { ids } if ids.len() == 2))
        );
        // The 003<->004 cycle is reported exactly once.
        assert_eq!(
            issues
                .iter()
                .filter(|i| matches!(i, GraphIssue::ParentCycle { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn children_computed_from_parent_fields() {
        let tasks = vec![
            task("001", &[], "pending"),
            with_parent(task("002", &[], "pending"), "001"),
            with_parent(task("003", &[], "pending"), "001"),
        ];
        let g = Graph::build(&tasks);
        let kids: Vec<String> = g.children("001").iter().map(|t| t.id()).collect();
        assert_eq!(kids, vec!["002", "003"]);
        assert!(g.children("002").is_empty());
    }

    #[test]
    fn duplicate_ids_first_file_wins_without_panic() {
        let tasks = vec![task("001", &[], "pending"), task("001", &[], "completed")];
        let g = Graph::build(&tasks);
        assert_eq!(g.get("001").unwrap().effective_status(), Status::Pending);
    }

    #[test]
    fn get_missing_is_none() {
        let tasks: Vec<Task> = vec![];
        let g = Graph::build(&tasks);
        assert!(g.get("404").is_none());
        assert_eq!(g.downstream_count("404"), 0);
        assert!(g.direct_dependents("404").is_empty());
        assert!(g.children("404").is_empty());
        assert!(g.issues().is_empty());
    }
}
