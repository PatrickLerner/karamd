//! Ranked next-task recommendation, a faithful port of taskmd's `next`
//! algorithm (sdk/go/next, taskmd 0.2.5) so both tools agree on the same
//! vault.
//!
//! Actionable = `pending` or `in-progress`, every dependency `completed` (a
//! `cancelled` dependency blocks forever — it never completes), and no
//! unresolved children. Score = priority base (40/30/20/10) + critical-path
//! bonus (15) + downstream bonus (3 per unblocked task, capped at 15) — the
//! last two scaled by the *max priority found downstream* (critical/high x1,
//! medium x0.5, else x0.25) — + effort bonus (small +5 "quick win", medium
//! +2) + phase bonus (25 - 5 x phase index). Ties break by id.

use serde::Serialize;

use crate::taskmd::{Effort, Graph, Priority, Status, Task};
use std::collections::{HashMap, HashSet};

const SCORE_PRIORITY: [(Priority, i64, &str); 4] = [
    (Priority::Critical, 40, "critical priority"),
    (Priority::High, 30, "high priority"),
    (Priority::Medium, 20, ""),
    (Priority::Low, 10, ""),
];
const SCORE_CRITICAL_PATH: i64 = 15;
const SCORE_PER_DOWNSTREAM: i64 = 3;
const SCORE_DOWNSTREAM_MAX: i64 = 15;
const SCORE_EFFORT_SMALL: i64 = 5;
const SCORE_EFFORT_MEDIUM: i64 = 2;
const SCORE_PHASE_BASE: i64 = 25;
const SCORE_PHASE_DECAY: i64 = 5;

/// One recommendation; field names and shapes match `taskmd next --format
/// json` exactly, so outputs are diffable for parity.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Recommendation {
    pub rank: usize,
    pub id: String,
    pub title: String,
    pub file_path: String,
    pub status: String,
    pub priority: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub effort: String,
    pub score: i64,
    pub reasons: Vec<String>,
    pub downstream_count: usize,
    pub on_critical_path: bool,
}

/// A blocked task worth surfacing (high/critical priority, not actionable),
/// with what blocks it — so the user sees the real next action is clearing a
/// blocker.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Blocked {
    pub id: String,
    pub title: String,
    pub priority: String,
    /// Open dependencies (id + status) standing in the way.
    pub blocked_on: Vec<String>,
}

/// Options mirroring taskmd's `next` flags.
#[derive(Debug, Default, Clone)]
pub struct Options {
    /// Max recommendations; 0 means taskmd's default of 5.
    pub limit: usize,
    /// Only `effort: small` tasks.
    pub quick_wins: bool,
    /// Only tasks on the critical path.
    pub critical: bool,
    /// Only tasks in this phase.
    pub phase: Option<String>,
    /// Sort by phase order before score.
    pub strict_phases: bool,
}

/// The full result: ranked recommendations plus the blocker view.
#[derive(Debug, Serialize, PartialEq)]
pub struct NextReport {
    pub recommendations: Vec<Recommendation>,
    /// High-priority tasks that are NOT actionable, with their blockers.
    pub blocked: Vec<Blocked>,
    /// Id of the actionable task that unblocks the most downstream work, if
    /// any recommendation unblocks anything at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_unblocker: Option<String>,
}

fn priority_weight(p: Option<Priority>) -> i64 {
    match p {
        Some(Priority::Critical) => 4,
        Some(Priority::High) => 3,
        Some(Priority::Medium) => 2,
        _ => 1,
    }
}

/// taskmd scales critical-path and downstream bonuses by the most important
/// thing waiting downstream: unblocking critical work is worth more than
/// unblocking low-priority work.
fn downstream_multiplier(max_priority: Option<Priority>) -> f64 {
    match max_priority {
        Some(Priority::Critical) | Some(Priority::High) => 1.0,
        Some(Priority::Medium) => 0.5,
        _ => 0.25,
    }
}

struct DownstreamInfo {
    count: usize,
    max_priority: Option<Priority>,
}

fn downstream_info(tasks: &[Task], graph: &Graph) -> HashMap<String, DownstreamInfo> {
    tasks
        .iter()
        .map(|t| {
            let ids = graph.downstream_ids(&t.id());
            let max_priority = ids
                .iter()
                .filter_map(|id| graph.get(id))
                .map(|d| d.priority())
                .max_by_key(|p| priority_weight(*p))
                .flatten();
            (
                t.id(),
                DownstreamInfo {
                    count: ids.len(),
                    max_priority,
                },
            )
        })
        .collect()
}

/// Depth of remaining work: resolved tasks contribute nothing; otherwise
/// 1 + the deepest dependency. Cycle-safe (a cycle contributes 0).
fn depth_map(tasks: &[Task], graph: &Graph) -> HashMap<String, usize> {
    fn depth(
        id: &str,
        graph: &Graph,
        memo: &mut HashMap<String, usize>,
        visiting: &mut HashSet<String>,
    ) -> usize {
        if let Some(&d) = memo.get(id) {
            return d;
        }
        if !visiting.insert(id.to_string()) {
            return 0;
        }
        let Some(task) = graph.get(id) else {
            visiting.remove(id);
            return 0;
        };
        if task.effective_status().is_terminal() {
            visiting.remove(id);
            return 0;
        }
        let max_dep = task
            .dependencies()
            .iter()
            .map(|dep| depth(dep, graph, memo, visiting))
            .max()
            .unwrap_or(0);
        visiting.remove(id);
        memo.insert(id.to_string(), max_dep + 1);
        max_dep + 1
    }
    let mut memo = HashMap::new();
    for t in tasks {
        depth(&t.id(), graph, &mut memo, &mut HashSet::new());
    }
    memo
}

/// Tasks on the critical path: everything at max depth, plus the dependency
/// chains that carry that depth (each dep at exactly depth-1, recursively).
fn critical_path(tasks: &[Task], graph: &Graph) -> HashSet<String> {
    let depths = depth_map(tasks, graph);
    let max_depth = depths.values().copied().max().unwrap_or(0);
    let mut on_path = HashSet::new();
    for (id, &d) in &depths {
        if d == max_depth {
            on_path.insert(id.clone());
            mark_chain(id, graph, &depths, d, &mut on_path);
        }
    }
    on_path
}

fn mark_chain(
    id: &str,
    graph: &Graph,
    depths: &HashMap<String, usize>,
    target: usize,
    on_path: &mut HashSet<String>,
) {
    // Ids reaching here always came out of the depth map, which only holds
    // tasks present in the graph.
    let task = graph.get(id).expect("id came from the depth map");
    for dep in task.dependencies() {
        if depths.get(&dep) == Some(&(target - 1)) && on_path.insert(dep.clone()) {
            mark_chain(&dep, graph, depths, target - 1, on_path);
        }
    }
}

/// taskmd's actionability: status is *explicitly* `pending` or `in-progress`
/// (taskmd does not default a missing status here), deps completed, and no
/// unresolved children (a parent is not actionable while its children are).
fn is_actionable(task: &Task, graph: &Graph) -> bool {
    if !matches!(
        task.status_raw().as_deref().and_then(Status::parse),
        Some(Status::Pending) | Some(Status::InProgress)
    ) {
        return false;
    }
    if !graph.is_ready(task) {
        return false;
    }
    graph
        .children(&task.id())
        .iter()
        .all(|c| c.effective_status().is_terminal())
}

fn score_task(
    task: &Task,
    on_path: &HashSet<String>,
    info: &HashMap<String, DownstreamInfo>,
    phase_order: &[String],
) -> (i64, Vec<String>) {
    let mut score = 0;
    let mut reasons = Vec::new();

    // Raw priority, exactly like taskmd: a *missing* priority scores like
    // `low` (base 10), it is NOT bumped to the spec's display default of
    // medium. Verified against the 0.2.5 binary.
    let priority = task.priority().unwrap_or(Priority::Low);
    for (p, points, reason) in SCORE_PRIORITY {
        if p == priority {
            score += points;
            if !reason.is_empty() {
                reasons.push(reason.to_string());
            }
        }
    }

    let id = task.id();
    // `info` is built over the same task slice, so the entry always exists.
    let info = info.get(&id).expect("info covers all tasks");
    let (count, mult) = (info.count, downstream_multiplier(info.max_priority));

    if on_path.contains(&id) {
        score += (SCORE_CRITICAL_PATH as f64 * mult) as i64;
        reasons.push("on critical path".to_string());
    }

    let capped = (count as i64 * SCORE_PER_DOWNSTREAM).min(SCORE_DOWNSTREAM_MAX);
    score += (capped as f64 * mult) as i64;
    if count > 0 {
        let noun = if count == 1 { "task" } else { "tasks" };
        reasons.push(format!("unblocks {count} {noun}"));
    }

    match task.effort() {
        Some(Effort::Small) => {
            score += SCORE_EFFORT_SMALL;
            reasons.push("quick win".to_string());
        }
        Some(Effort::Medium) => score += SCORE_EFFORT_MEDIUM,
        _ => {}
    }

    if let Some(phase) = task.phase()
        && let Some(idx) = phase_order.iter().position(|p| *p == phase)
    {
        let bonus = SCORE_PHASE_BASE - (idx as i64 * SCORE_PHASE_DECAY);
        if bonus > 0 {
            score += bonus;
            reasons.push(format!("phase {phase}"));
        }
    }

    (score, reasons)
}

/// Rank actionable tasks per taskmd's algorithm and collect the blocker view.
/// `phase_order` comes from the configured phases (their keys, in order).
pub fn recommend(tasks: &[Task], phase_order: &[String], opts: &Options) -> NextReport {
    let graph = Graph::build(tasks);
    let on_path = critical_path(tasks, &graph);
    let info = downstream_info(tasks, &graph);
    let limit = if opts.limit == 0 { 5 } else { opts.limit };

    let mut scored: Vec<(&Task, i64, Vec<String>)> = tasks
        .iter()
        .filter(|t| is_actionable(t, &graph))
        .filter(|t| {
            opts.phase
                .as_ref()
                .is_none_or(|p| t.phase().as_deref() == Some(p))
        })
        .filter(|t| !opts.quick_wins || t.effort() == Some(Effort::Small))
        .filter(|t| !opts.critical || on_path.contains(&t.id()))
        .map(|t| {
            let (score, reasons) = score_task(t, &on_path, &info, phase_order);
            (t, score, reasons)
        })
        .collect();

    let phase_index = |t: &Task| {
        t.phase()
            .and_then(|p| phase_order.iter().position(|o| *o == p))
            .unwrap_or(phase_order.len())
    };
    scored.sort_by(|a, b| {
        if opts.strict_phases && !phase_order.is_empty() {
            let (pa, pb) = (phase_index(a.0), phase_index(b.0));
            if pa != pb {
                return pa.cmp(&pb);
            }
        }
        b.1.cmp(&a.1).then_with(|| a.0.id().cmp(&b.0.id()))
    });

    let recommendations: Vec<Recommendation> = scored
        .iter()
        .take(limit)
        .enumerate()
        .map(|(i, (t, score, reasons))| {
            let id = t.id();
            Recommendation {
                rank: i + 1,
                id: id.clone(),
                title: t.title(),
                file_path: t
                    .rel_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                // Raw values, like taskmd: a missing priority serializes as
                // "" so parity diffs stay clean.
                status: t.status_raw().unwrap_or_default(),
                priority: t.priority_raw().unwrap_or_default(),
                effort: t
                    .effort()
                    .map(|e| e.as_str().to_string())
                    .unwrap_or_default(),
                score: *score,
                reasons: reasons.clone(),
                downstream_count: info.get(&id).map(|i| i.count).unwrap_or(0),
                on_critical_path: on_path.contains(&id),
            }
        })
        .collect();

    // Blocker view: open high/critical tasks that are not actionable, with
    // their unmet dependencies, most important first.
    let mut blocked: Vec<Blocked> = tasks
        .iter()
        .filter(|t| {
            matches!(
                t.effective_status(),
                Status::Pending | Status::InProgress | Status::Blocked
            ) && t.effective_priority() >= Priority::High
                && !is_actionable(t, &graph)
        })
        .map(|t| {
            let mut blocked_on: Vec<String> = graph
                .blockers(t)
                .iter()
                .map(|d| format!("{} ({})", d.id(), d.effective_status().as_str()))
                .collect();
            // Dangling deps block too but have no task to show a status for.
            for dep in t.dependencies() {
                if graph.get(&dep).is_none() {
                    blocked_on.push(format!("{dep} (missing)"));
                }
            }
            Blocked {
                id: t.id(),
                title: t.title(),
                priority: t.effective_priority().as_str().to_string(),
                blocked_on,
            }
        })
        .collect();
    blocked.sort_by(|a, b| a.id.cmp(&b.id));

    // Suggest the recommendation that unblocks the most downstream work.
    let suggested_unblocker = recommendations
        .iter()
        .filter(|r| r.downstream_count > 0)
        .max_by_key(|r| r.downstream_count)
        .map(|r| r.id.clone());

    NextReport {
        recommendations,
        blocked,
        suggested_unblocker,
    }
}

/// Human rendering of the report.
pub fn render_human(report: &NextReport) -> String {
    let mut out = String::new();
    if report.recommendations.is_empty() {
        out.push_str("no actionable tasks");
    }
    for r in &report.recommendations {
        let reasons = if r.reasons.is_empty() {
            String::new()
        } else {
            format!("  ({})", r.reasons.join(", "))
        };
        out.push_str(&format!(
            "{}. {} {} [score {}]{}\n",
            r.rank, r.id, r.title, r.score, reasons
        ));
    }
    if !report.blocked.is_empty() {
        out.push_str("\nblocked high-priority work:\n");
        for b in &report.blocked {
            out.push_str(&format!(
                "  {} {} [{}] waits on {}\n",
                b.id,
                b.title,
                b.priority,
                b.blocked_on.join(", ")
            ));
        }
        if let Some(s) = &report.suggested_unblocker {
            out.push_str(&format!("  -> doing {s} unblocks the most work\n"));
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture builder; adds `status: pending` when the yaml has no status
    /// (taskmd's actionability requires an explicit one).
    fn task(yaml: &str) -> Task {
        let yaml = if yaml.contains("status:") {
            yaml.to_string()
        } else {
            format!("{yaml}\nstatus: pending")
        };
        let content = format!("---\n{yaml}\n---\n");
        let mut t = Task::parse_required(&content).unwrap();
        t.rel_path = Some(std::path::PathBuf::from(format!("{}-t.md", t.id())));
        t
    }

    fn recommend_all(tasks: &[Task]) -> NextReport {
        recommend(tasks, &[], &Options::default())
    }

    fn score_of(report: &NextReport, id: &str) -> i64 {
        report
            .recommendations
            .iter()
            .find(|r| r.id == id)
            .unwrap()
            .score
    }

    // --- scores verified against the real taskmd 0.2.5 binary ---

    #[test]
    fn priority_bases_match_taskmd() {
        // Empirical: 4 isolated tasks -> all on the (trivial) critical path,
        // multiplier 0.25 (no downstream): low 13, medium 23, high 33,
        // critical 43.
        let tasks = vec![
            task("id: \"001\"\ntitle: L\npriority: low"),
            task("id: \"002\"\ntitle: M\npriority: medium"),
            task("id: \"003\"\ntitle: H\npriority: high"),
            task("id: \"004\"\ntitle: C\npriority: critical"),
        ];
        let r = recommend_all(&tasks);
        assert_eq!(score_of(&r, "001"), 13);
        assert_eq!(score_of(&r, "002"), 23);
        assert_eq!(score_of(&r, "003"), 33);
        assert_eq!(score_of(&r, "004"), 43);
        // Order: score desc.
        let ids: Vec<&str> = r.recommendations.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["004", "003", "002", "001"]);
    }

    #[test]
    fn downstream_and_critical_path_match_taskmd() {
        // Empirical probe: chain 001<-002<-003 plus hub 004<-{005,006} plus
        // isolated 007 (all explicitly medium): 001 -> 30 (cp), 004 -> 23,
        // 007 -> 20.
        let tasks = vec![
            task("id: \"001\"\ntitle: A\npriority: medium"),
            task("id: \"002\"\ntitle: B\npriority: medium\ndependencies: [\"001\"]"),
            task("id: \"003\"\ntitle: C\npriority: medium\ndependencies: [\"002\"]"),
            task("id: \"004\"\ntitle: D\npriority: medium"),
            task("id: \"005\"\ntitle: E\npriority: medium\ndependencies: [\"004\"]"),
            task("id: \"006\"\ntitle: F\npriority: medium\ndependencies: [\"004\"]"),
            task("id: \"007\"\ntitle: G\npriority: medium"),
        ];
        let r = recommend_all(&tasks);
        assert_eq!(score_of(&r, "001"), 30);
        assert_eq!(score_of(&r, "004"), 23);
        assert_eq!(score_of(&r, "007"), 20);
        let one = r.recommendations.iter().find(|x| x.id == "001").unwrap();
        assert!(one.on_critical_path);
        assert_eq!(one.downstream_count, 2);
        assert_eq!(one.reasons, vec!["on critical path", "unblocks 2 tasks"]);
    }

    #[test]
    fn downstream_cap_and_multiplier_match_taskmd() {
        // Empirical: hubs with 4/8 medium dependents score 33/34
        // (cap 15 * multiplier 0.5, cp 15 * 0.5 = 7).
        let mut tasks = vec![task("id: \"100\"\ntitle: Hub4\npriority: medium")];
        for i in 101..=104 {
            tasks.push(task(&format!(
                "id: \"{i}\"\ntitle: D{i}\npriority: medium\ndependencies: [\"100\"]"
            )));
        }
        let mut more = vec![task("id: \"300\"\ntitle: Hub8\npriority: medium")];
        for i in 301..=308 {
            more.push(task(&format!(
                "id: \"{i}\"\ntitle: D{i}\npriority: medium\ndependencies: [\"300\"]"
            )));
        }
        tasks.extend(more);
        let r = recommend_all(&tasks);
        assert_eq!(score_of(&r, "100"), 33); // 20 + 7(cp) + 6(ds 12*0.5)
        assert_eq!(score_of(&r, "300"), 34); // 20 + 7(cp) + 7(ds capped 15*0.5)
    }

    #[test]
    fn high_priority_downstream_scales_bonuses_up() {
        // The real-vault case: high task unblocking 8 incl. high-priority
        // downstream scored 60 = 30 + 15 + 15.
        let mut tasks = vec![task("id: \"008\"\ntitle: Root\npriority: high")];
        for i in 9..=16 {
            tasks.push(task(&format!(
                "id: \"{i:03}\"\ntitle: D{i}\npriority: high\ndependencies: [\"008\"]"
            )));
        }
        let r = recommend_all(&tasks);
        assert_eq!(score_of(&r, "008"), 60);
    }

    #[test]
    fn effort_bonuses_and_quick_wins_filter() {
        let tasks = vec![
            task("id: \"001\"\ntitle: S\npriority: medium\neffort: small"),
            task("id: \"002\"\ntitle: M\npriority: medium\neffort: medium"),
            task("id: \"003\"\ntitle: L\npriority: medium\neffort: large"),
        ];
        let r = recommend_all(&tasks);
        // All isolated -> cp bonus 3 each (mult 0.25); small +5, medium +2.
        assert_eq!(score_of(&r, "001"), 28);
        assert_eq!(score_of(&r, "002"), 25);
        assert_eq!(score_of(&r, "003"), 23);
        assert!(
            r.recommendations
                .iter()
                .find(|x| x.id == "001")
                .unwrap()
                .reasons
                .contains(&"quick win".to_string())
        );

        let quick = recommend(
            &tasks,
            &[],
            &Options {
                quick_wins: true,
                ..Options::default()
            },
        );
        let ids: Vec<&str> = quick
            .recommendations
            .iter()
            .map(|x| x.id.as_str())
            .collect();
        assert_eq!(ids, vec!["001"]);
    }

    #[test]
    fn phase_bonus_decays_with_order() {
        let tasks = vec![
            task("id: \"001\"\ntitle: A\npriority: medium\nphase: alpha"),
            task("id: \"002\"\ntitle: B\npriority: medium\nphase: beta"),
            task("id: \"003\"\ntitle: C\npriority: medium\nphase: unknown"),
            task("id: \"004\"\ntitle: D\npriority: medium"),
        ];
        let order = vec!["alpha".to_string(), "beta".to_string()];
        let r = recommend(&tasks, &order, &Options::default());
        // base 20 + cp 3 (all trivial-critical) + phase bonus 25 / 20 / 0 / 0.
        assert_eq!(score_of(&r, "001"), 48);
        assert_eq!(score_of(&r, "002"), 43);
        assert_eq!(score_of(&r, "003"), 23);
        assert_eq!(score_of(&r, "004"), 23);
        assert!(
            r.recommendations
                .iter()
                .find(|x| x.id == "001")
                .unwrap()
                .reasons
                .contains(&"phase alpha".to_string())
        );
    }

    #[test]
    fn phase_bonus_never_negative() {
        // Index 5 -> 25 - 25 = 0 -> no bonus, no reason.
        let tasks = vec![task("id: \"001\"\ntitle: A\npriority: medium\nphase: f")];
        let order: Vec<String> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let r = recommend(&tasks, &order, &Options::default());
        assert_eq!(score_of(&r, "001"), 23);
    }

    #[test]
    fn readiness_is_a_hard_gate() {
        let tasks = vec![
            task("id: \"001\"\ntitle: Open dep"),
            task(
                "id: \"002\"\ntitle: Critical blocked\npriority: critical\ndependencies: [\"001\"]",
            ),
        ];
        let r = recommend_all(&tasks);
        // 002 is critical but blocked: never recommended, whatever its score.
        assert!(r.recommendations.iter().all(|x| x.id != "002"));
        // ...and it appears in the blocker view with its open dependency.
        assert_eq!(r.blocked.len(), 1);
        assert_eq!(r.blocked[0].id, "002");
        assert_eq!(r.blocked[0].blocked_on, vec!["001 (pending)"]);
        // The suggested unblocker is the task carrying the downstream work.
        assert_eq!(r.suggested_unblocker.as_deref(), Some("001"));
    }

    #[test]
    fn cancelled_dependency_blocks_forever() {
        let tasks = vec![
            task("id: \"001\"\ntitle: Dead\nstatus: cancelled"),
            task("id: \"002\"\ntitle: Waits\npriority: high\ndependencies: [\"001\"]"),
        ];
        let r = recommend_all(&tasks);
        assert!(r.recommendations.is_empty());
        assert_eq!(r.blocked[0].blocked_on, vec!["001 (cancelled)"]);
    }

    #[test]
    fn missing_dependency_blocks_and_is_labelled() {
        let tasks = vec![task(
            "id: \"002\"\ntitle: Waits\npriority: high\ndependencies: [\"404\"]",
        )];
        let r = recommend_all(&tasks);
        assert!(r.recommendations.is_empty());
        assert_eq!(r.blocked[0].blocked_on, vec!["404 (missing)"]);
    }

    #[test]
    fn parent_with_open_children_is_not_actionable() {
        let tasks = vec![
            task("id: \"001\"\ntitle: Parent"),
            task("id: \"002\"\ntitle: Child\nparent: \"001\""),
        ];
        let r = recommend_all(&tasks);
        let ids: Vec<&str> = r.recommendations.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["002"]); // parent gated, child fine
        // Once the child resolves, the parent becomes actionable.
        let tasks2 = vec![
            task("id: \"001\"\ntitle: Parent"),
            task("id: \"002\"\ntitle: Child\nstatus: completed\nparent: \"001\""),
        ];
        let r2 = recommend_all(&tasks2);
        let ids2: Vec<&str> = r2.recommendations.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids2, vec!["001"]);
    }

    #[test]
    fn only_pending_and_in_progress_are_actionable() {
        let tasks = vec![
            task("id: \"001\"\ntitle: P\nstatus: pending"),
            task("id: \"002\"\ntitle: I\nstatus: in-progress"),
            task("id: \"003\"\ntitle: R\nstatus: in-review"),
            task("id: \"004\"\ntitle: B\nstatus: blocked"),
            task("id: \"005\"\ntitle: C\nstatus: completed"),
            task("id: \"006\"\ntitle: X\nstatus: cancelled"),
        ];
        let r = recommend_all(&tasks);
        let mut ids: Vec<&str> = r.recommendations.iter().map(|x| x.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["001", "002"]);
    }

    #[test]
    fn resolved_dependencies_do_not_stretch_the_critical_path() {
        // A completed dep contributes no depth: 002 sits at depth 1 like 003.
        let tasks = vec![
            task("id: \"001\"\ntitle: Done\nstatus: completed"),
            task("id: \"002\"\ntitle: Next\ndependencies: [\"001\"]"),
            task("id: \"003\"\ntitle: Fresh"),
        ];
        let r = recommend_all(&tasks);
        assert_eq!(score_of(&r, "002"), score_of(&r, "003"));
    }

    #[test]
    fn limit_and_default_limit() {
        let tasks: Vec<Task> = (1..=8)
            .map(|i| task(&format!("id: \"{i:03}\"\ntitle: T{i}")))
            .collect();
        assert_eq!(recommend_all(&tasks).recommendations.len(), 5); // default
        let two = recommend(
            &tasks,
            &[],
            &Options {
                limit: 2,
                ..Options::default()
            },
        );
        assert_eq!(two.recommendations.len(), 2);
        assert_eq!(two.recommendations[0].rank, 1);
        assert_eq!(two.recommendations[1].rank, 2);
    }

    #[test]
    fn critical_and_phase_filters() {
        let tasks = vec![
            task("id: \"001\"\ntitle: A\nphase: v1"),
            task("id: \"002\"\ntitle: B\ndependencies: [\"001\"]"),
            task("id: \"003\"\ntitle: C\nphase: v2"),
        ];
        let crit = recommend(
            &tasks,
            &[],
            &Options {
                critical: true,
                ..Options::default()
            },
        );
        let ids: Vec<&str> = crit.recommendations.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["001"]); // only the chain head is actionable + on path
        let phased = recommend(
            &tasks,
            &[],
            &Options {
                phase: Some("v2".into()),
                ..Options::default()
            },
        );
        let ids: Vec<&str> = phased
            .recommendations
            .iter()
            .map(|x| x.id.as_str())
            .collect();
        assert_eq!(ids, vec!["003"]);
    }

    #[test]
    fn strict_phases_orders_by_phase_before_score() {
        let tasks = vec![
            task("id: \"001\"\ntitle: LaterPhase\npriority: critical\nphase: beta"),
            task("id: \"002\"\ntitle: EarlyPhase\npriority: low\nphase: alpha"),
            task("id: \"003\"\ntitle: NoPhase\npriority: critical"),
        ];
        let order = vec!["alpha".to_string(), "beta".to_string()];
        let strict = recommend(
            &tasks,
            &order,
            &Options {
                strict_phases: true,
                ..Options::default()
            },
        );
        let ids: Vec<&str> = strict
            .recommendations
            .iter()
            .map(|x| x.id.as_str())
            .collect();
        // alpha first despite lower score; no-phase last.
        assert_eq!(ids, vec!["002", "001", "003"]);
        // Without strict, pure score order wins.
        let loose = recommend(&tasks, &order, &Options::default());
        let ids: Vec<&str> = loose
            .recommendations
            .iter()
            .map(|x| x.id.as_str())
            .collect();
        assert_eq!(ids, vec!["001", "003", "002"]);
        // Same phase under strict ordering falls through to score.
        let same_phase = vec![
            task("id: \"001\"\ntitle: A\npriority: low\nphase: alpha"),
            task("id: \"002\"\ntitle: B\npriority: high\nphase: alpha"),
        ];
        let r = recommend(
            &same_phase,
            &order,
            &Options {
                strict_phases: true,
                ..Options::default()
            },
        );
        let ids: Vec<&str> = r.recommendations.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["002", "001"]);
    }

    #[test]
    fn tie_breaks_by_id() {
        let tasks = vec![task("id: \"002\"\ntitle: B"), task("id: \"001\"\ntitle: A")];
        let r = recommend_all(&tasks);
        let ids: Vec<&str> = r.recommendations.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["001", "002"]);
    }

    #[test]
    fn cycles_do_not_hang_scoring() {
        let tasks = vec![
            task("id: \"001\"\ntitle: A\ndependencies: [\"002\"]"),
            task("id: \"002\"\ntitle: B\ndependencies: [\"001\"]"),
            task("id: \"003\"\ntitle: C"),
        ];
        let r = recommend_all(&tasks);
        // The cycle members are never ready; 003 is recommended.
        let ids: Vec<&str> = r.recommendations.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["003"]);
    }

    #[test]
    fn human_rendering() {
        let tasks = vec![
            task("id: \"001\"\ntitle: Open dep"),
            task("id: \"002\"\ntitle: Blocked\npriority: high\ndependencies: [\"001\"]"),
        ];
        let out = render_human(&recommend_all(&tasks));
        assert!(out.contains("1. 001 Open dep [score"));
        assert!(out.contains("blocked high-priority work:"));
        assert!(out.contains("002 Blocked [high] waits on 001 (pending)"));
        assert!(out.contains("-> doing 001 unblocks the most work"));
        // Empty vault.
        assert_eq!(render_human(&recommend_all(&[])), "no actionable tasks");
        // Recommendations without blockers render no blocked section.
        let solo = vec![task("id: \"001\"\ntitle: A")];
        let out = render_human(&recommend_all(&solo));
        assert!(!out.contains("blocked"));
        // A blocked section without any unblocker to suggest (the only
        // recommendation unblocks nothing).
        let stuck = vec![
            task("id: \"001\"\ntitle: Free"),
            task("id: \"002\"\ntitle: Dead\nstatus: cancelled"),
            task("id: \"003\"\ntitle: Stuck\npriority: high\ndependencies: [\"002\"]"),
        ];
        let report = recommend_all(&stuck);
        assert_eq!(report.suggested_unblocker, None);
        let out = render_human(&report);
        assert!(out.contains("blocked high-priority work:"));
        assert!(!out.contains("unblocks the most work"));
        // A recommendation with no reasons at all renders without the
        // parenthesis suffix: off the critical path, nothing downstream.
        let plain = vec![
            task("id: \"001\"\ntitle: Deep"),
            task("id: \"002\"\ntitle: Deeper\ndependencies: [\"001\"]"),
            task("id: \"003\"\ntitle: Plain"),
        ];
        let out = render_human(&recommend_all(&plain));
        assert!(out.contains("003 Plain [score 10]\n") || out.ends_with("003 Plain [score 10]"));
        assert!(!out.contains("003 Plain [score 10]  ("));
    }
}
