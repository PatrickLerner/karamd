//! Autonomous task execution (#039): run a configured AI agent against tasks
//! explicitly tagged `ai-runnable`, tracking attempts so a failing task is
//! never retried forever.
//!
//! This module is the pure, testable core: selection, prompt rendering, working
//! -dir/agent resolution, the frontmatter state transitions, and the
//! orchestration loop. The actual subprocess spawn lives behind the
//! [`AgentRunner`] trait; the real process implementation is in
//! [`crate::run_spawn`] (excluded from coverage, like the web PTY glue).
//!
//! Safety model (three independent locks, all required before anything spawns):
//!   1. `run.enabled` is true in config (off by default).
//!   2. The task carries the [`RUNNABLE_TAG`].
//!   3. The command comes only from `run.agents` in config; a task may pick
//!      *which* named agent, never an arbitrary command.
//!
//! Retry bound: `ai_attempts` is incremented *before* the spawn, so a crash
//! still counts as an attempt (no free infinite retry). At `max_attempts` the
//! task is parked with the [`FAILED_TAG`] and no longer selected until a human
//! removes the tag or resets the counter.

use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use serde_norway::Value;

use crate::taskmd::config::{AgentSpec, RunConfig};
use crate::taskmd::{Task, Vault};

/// Tag a task must carry to be eligible (per-task opt-in).
pub const RUNNABLE_TAG: &str = "ai-runnable";
/// Tag added when a task exhausts `max_attempts`; excludes it from selection.
pub const FAILED_TAG: &str = "ai-failed";

// Frontmatter markers karamd writes to track execution state. All are cleared
// on success; `ai_working_dir`/`ai_agent` are user-set config, not markers, and
// are left alone.
const K_ATTEMPTS: &str = "ai_attempts";
const K_STATUS: &str = "ai_status";
const K_STARTED: &str = "ai_run_started";
const K_LAST_ERROR: &str = "ai_last_error";
const K_LAST_RUN: &str = "ai_last_run";
const K_AGENT: &str = "ai_agent";
const K_WORKING_DIR: &str = "ai_working_dir";
const STATUS_RUNNING: &str = "running";
const STATUS_FAILED: &str = "failed";

/// What a single agent invocation produced. Spawn errors and timeouts are
/// encoded as `success: false` with a `detail`, so the runner never fails the
/// whole loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOutcome {
    pub success: bool,
    pub detail: String,
}

/// Spawns one agent. Behind a trait so the orchestration is tested with a fake
/// and only the real process glue ([`crate::run_spawn`]) is excluded.
pub trait AgentRunner {
    fn run(
        &self,
        spec: &AgentSpec,
        prompt: &str,
        working_dir: &Path,
        timeout_secs: u64,
    ) -> AgentOutcome;
}

/// Outcome of running one task, for reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub id: String,
    pub succeeded: bool,
    /// Failure reason (empty on success).
    pub detail: String,
    /// True when this failure pushed the task to `max_attempts` and parked it.
    pub parked: bool,
}

/// What a `run` invocation did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunReport {
    pub ran: Vec<RunResult>,
}

/// Current attempt count, tolerating a hand-edited string value.
fn read_attempts(task: &Task) -> u32 {
    match task.get(K_ATTEMPTS) {
        Some(v) => v
            .as_u64()
            .map(|n| n as u32)
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0),
        None => 0,
    }
}

/// Is a `running` marker still fresh (a real in-flight run), rather than a
/// crashed leftover? Stale once older than `2 * timeout`. A missing or
/// unparseable timestamp counts as stale so it can never block forever.
fn is_stale(started: Option<&str>, now: DateTime<Utc>, timeout_secs: u64) -> bool {
    match started.and_then(|s| DateTime::parse_from_rfc3339(s).ok()) {
        Some(t) => (now - t.with_timezone(&Utc)).num_seconds() >= (2 * timeout_secs) as i64,
        None => true,
    }
}

/// A task is locked when a prior run marked it `running` and that marker is
/// still fresh, so a second overlapping `run` invocation skips it.
fn is_locked(task: &Task, cfg: &RunConfig, now: DateTime<Utc>) -> bool {
    let running = task.get(K_STATUS).and_then(|v| v.as_str()) == Some(STATUS_RUNNING);
    let started = task.get(K_STARTED).and_then(|v| v.as_str());
    running && !is_stale(started, now, cfg.timeout_secs)
}

/// Selection predicate: eligible iff tagged runnable, not parked, not terminal,
/// under the attempt cap, and not currently locked by a fresh run.
pub fn is_runnable(task: &Task, cfg: &RunConfig, now: DateTime<Utc>) -> bool {
    let tags = task.tags();
    tags.iter().any(|t| t == RUNNABLE_TAG)
        && !tags.iter().any(|t| t == FAILED_TAG)
        && !task.effective_status().is_terminal()
        && read_attempts(task) < cfg.max_attempts
        && !is_locked(task, cfg, now)
}

/// Render the prompt from the template, interpolating the task's fields and its
/// absolute path.
pub fn render_prompt(template: &str, task: &Task, path: &str) -> String {
    template
        .replace("{id}", &task.id())
        .replace("{title}", &task.title())
        .replace("{body}", task.body.trim())
        .replace("{path}", path)
}

/// Where the agent runs: task `ai_working_dir` wins, else config `working_dir`,
/// else the vault root.
pub fn resolve_working_dir(task: &Task, cfg: &RunConfig, vault_root: &Path) -> PathBuf {
    if let Some(dir) = task.get(K_WORKING_DIR).and_then(|v| v.as_str()) {
        PathBuf::from(dir)
    } else if let Some(dir) = &cfg.working_dir {
        PathBuf::from(dir)
    } else {
        vault_root.to_path_buf()
    }
}

/// Resolve which configured agent a task uses (`ai_agent` override, else the
/// config default). An unknown name is an error, never an arbitrary command.
pub fn resolve_agent<'a>(cfg: &'a RunConfig, task: &Task) -> Result<&'a AgentSpec, String> {
    let name = task
        .get(K_AGENT)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| cfg.agent.clone());
    cfg.agents
        .get(&name)
        .ok_or_else(|| format!("unknown agent `{name}` (not configured in run.agents)"))
}

/// Substitute every `token` occurrence in an argv with `value`. Used by the
/// real runner to build `{prompt}` / `{prompt_file}` commands.
pub fn substitute_prompt(args: &[String], token: &str, value: &str) -> Vec<String> {
    args.iter().map(|a| a.replace(token, value)).collect()
}

/// Pre-run marker write: increment the attempt counter (before the spawn, so a
/// crash still costs an attempt) and record the running lock.
pub fn mark_running(task: &mut Task, now: DateTime<Utc>) {
    let next = read_attempts(task) + 1;
    task.set(K_ATTEMPTS, Value::from(next as u64));
    task.set(K_STATUS, Value::String(STATUS_RUNNING.into()));
    task.set(K_STARTED, Value::String(now.to_rfc3339()));
}

/// Clear every runtime marker on success (`ai_working_dir`/`ai_agent` are user
/// config and are left intact).
pub fn record_success(task: &mut Task) {
    for key in [K_STATUS, K_STARTED, K_LAST_ERROR, K_LAST_RUN, K_ATTEMPTS] {
        task.remove(key);
    }
}

/// Record a failed run: stamp the error, drop the running lock, and park the
/// task with [`FAILED_TAG`] once the (already-incremented) attempts reach the
/// cap.
pub fn record_failure(task: &mut Task, detail: &str, today: NaiveDate, max_attempts: u32) {
    task.set(K_STATUS, Value::String(STATUS_FAILED.into()));
    task.set(K_LAST_ERROR, Value::String(detail.to_string()));
    task.set(
        K_LAST_RUN,
        Value::String(today.format("%Y-%m-%d").to_string()),
    );
    task.remove(K_STARTED);
    if read_attempts(task) >= max_attempts {
        let mut tags = task.tags();
        if !tags.iter().any(|t| t == FAILED_TAG) {
            tags.push(FAILED_TAG.to_string());
            task.set_tags(&tags);
        }
    }
}

/// One human-readable status line for a finished run (used by the CLI).
pub fn render_result_line(r: &RunResult) -> String {
    if r.succeeded {
        format!("karamd: ran {} -> completed", r.id)
    } else if r.parked {
        format!(
            "karamd: ran {} -> failed, parked (ai-failed): {}",
            r.id, r.detail
        )
    } else {
        format!("karamd: ran {} -> failed: {}", r.id, r.detail)
    }
}

/// The ids `run` would execute this tick, without mutating anything (dry-run).
pub fn plan(vault: &Vault, now: DateTime<Utc>) -> Result<Vec<String>> {
    let cfg = &vault.config.run;
    if !cfg.enabled {
        return Ok(Vec::new());
    }
    let scan = vault.scan()?;
    Ok(scan
        .tasks
        .iter()
        .filter(|t| is_runnable(t, cfg, now))
        .map(Task::id)
        .collect())
}

/// Execute every runnable task once, in id order, recording each outcome.
///
/// Per task: mark running (pre-increment attempts), render the prompt from the
/// staged task, spawn via `runner`, re-read the (possibly agent-modified) task,
/// then record success (exit 0 *and* the agent moved the task to a terminal
/// status) or failure. All writes go through [`Vault::update`], which re-reads
/// before mutating, so a concurrent sync is not clobbered.
pub fn run_all(
    vault: &Vault,
    runner: &dyn AgentRunner,
    now: DateTime<Utc>,
    today: NaiveDate,
) -> Result<RunReport> {
    let cfg = &vault.config.run;
    let mut report = RunReport::default();
    if !cfg.enabled {
        return Ok(report);
    }
    let scan = vault.scan()?;
    let ids: Vec<String> = scan
        .tasks
        .iter()
        .filter(|t| is_runnable(t, cfg, now))
        .map(Task::id)
        .collect();

    for id in ids {
        // Pre-increment attempts + take the running lock (find + save is exactly
        // what Vault::update does: a fresh re-read before the write).
        let mut staged = vault.find(&id)?;
        mark_running(&mut staged, now);
        vault.save(&staged)?;

        let abs_path = staged
            .rel_path
            .as_ref()
            .map(|p| vault.tasks_dir().join(p))
            .unwrap_or_default();
        let prompt = render_prompt(&cfg.prompt_template, &staged, &abs_path.to_string_lossy());
        let working_dir = resolve_working_dir(&staged, cfg, &vault.root);

        let outcome = match resolve_agent(cfg, &staged) {
            Ok(spec) => runner.run(spec, &prompt, &working_dir, cfg.timeout_secs),
            Err(detail) => AgentOutcome {
                success: false,
                detail,
            },
        };

        // Re-read post-agent: the agent may have run `karamd complete`, so the
        // status on disk is the source of truth for the success check.
        let mut after = vault.find(&id)?;
        let terminal = after.effective_status().is_terminal();
        let (succeeded, detail) = if outcome.success && terminal {
            (true, String::new())
        } else if outcome.success {
            (
                false,
                "agent exited 0 but did not mark the task complete".to_string(),
            )
        } else {
            (false, outcome.detail.clone())
        };

        if succeeded {
            record_success(&mut after);
        } else {
            record_failure(&mut after, &detail, today, cfg.max_attempts);
        }
        vault.save(&after)?;
        let parked = after.tags().iter().any(|t| t == FAILED_TAG);
        report.ran.push(RunResult {
            id,
            succeeded,
            detail,
            parked,
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskmd::Status;
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn now() -> DateTime<Utc> {
        "2026-07-09T12:00:00Z".parse().unwrap()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 9).unwrap()
    }

    fn tempdir() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("karamd-run-{uniq}"));
        fs::create_dir_all(base.join("tasks")).unwrap();
        base
    }

    const CFG: &str = "run:\n  enabled: true\n  agent: claude\n  timeout_secs: 100\n  max_attempts: 2\n  prompt_template: \"do {id}: {title}\\n{body}\\npath={path}\"\n  agents:\n    claude:\n      command: [claude, \"-p\", \"{prompt}\"]\n";

    /// Build a vault with the given `.taskmd.yaml` and `(filename, content)` tasks.
    fn build_vault(config: &str, tasks: &[(&str, &str)]) -> (Vault, PathBuf) {
        let dir = tempdir();
        fs::write(dir.join(".taskmd.yaml"), config).unwrap();
        for (name, content) in tasks {
            fs::write(dir.join("tasks").join(name), content).unwrap();
        }
        (Vault::open(&dir).unwrap(), dir)
    }

    fn runnable_task(id: &str) -> String {
        format!(
            "---\nid: \"{id}\"\ntitle: Fetch {id}\nstatus: pending\ntags: [ai-runnable]\n---\n\nbody {id}\n"
        )
    }

    #[derive(Clone)]
    enum Mode {
        /// Simulate the agent completing the task file at this path.
        Complete(PathBuf),
        /// Exit 0 without touching the task.
        SucceedNoComplete,
        Fail(String),
    }

    struct Recorded {
        command: Vec<String>,
        prompt: String,
        working_dir: PathBuf,
        timeout: u64,
    }

    struct FakeRunner {
        mode: Mode,
        calls: RefCell<Vec<Recorded>>,
    }

    impl FakeRunner {
        fn new(mode: Mode) -> FakeRunner {
            FakeRunner {
                mode,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl AgentRunner for FakeRunner {
        fn run(
            &self,
            spec: &AgentSpec,
            prompt: &str,
            working_dir: &Path,
            timeout_secs: u64,
        ) -> AgentOutcome {
            self.calls.borrow_mut().push(Recorded {
                command: spec.command.clone(),
                prompt: prompt.to_string(),
                working_dir: working_dir.to_path_buf(),
                timeout: timeout_secs,
            });
            match &self.mode {
                Mode::Complete(path) => {
                    let mut t = Task::parse_required(&fs::read_to_string(path).unwrap()).unwrap();
                    t.set_status(Status::Completed, today());
                    fs::write(path, t.to_markdown()).unwrap();
                    AgentOutcome {
                        success: true,
                        detail: String::new(),
                    }
                }
                Mode::SucceedNoComplete => AgentOutcome {
                    success: true,
                    detail: String::new(),
                },
                Mode::Fail(d) => AgentOutcome {
                    success: false,
                    detail: d.clone(),
                },
            }
        }
    }

    fn reload(vault: &Vault, id: &str) -> Task {
        vault.find(id).unwrap()
    }

    // ---- pure helpers ----

    #[test]
    fn read_attempts_number_string_and_missing() {
        let num = Task::parse_required("---\nid: \"1\"\ntitle: t\nai_attempts: 3\n---\n").unwrap();
        assert_eq!(read_attempts(&num), 3);
        let s =
            Task::parse_required("---\nid: \"1\"\ntitle: t\nai_attempts: \"2\"\n---\n").unwrap();
        assert_eq!(read_attempts(&s), 2);
        let missing = Task::parse_required("---\nid: \"1\"\ntitle: t\n---\n").unwrap();
        assert_eq!(read_attempts(&missing), 0);
        // A value that is neither a number nor a parseable string reads as 0.
        let weird =
            Task::parse_required("---\nid: \"1\"\ntitle: t\nai_attempts: [x]\n---\n").unwrap();
        assert_eq!(read_attempts(&weird), 0);
    }

    #[test]
    fn is_stale_fresh_old_and_missing() {
        assert!(!is_stale(Some(&now().to_rfc3339()), now(), 100));
        assert!(is_stale(Some("2026-07-01T00:00:00Z"), now(), 100));
        assert!(is_stale(None, now(), 100));
        assert!(is_stale(Some("not-a-timestamp"), now(), 100));
    }

    #[test]
    fn render_prompt_interpolates_all_tokens() {
        let t =
            Task::parse_required("---\nid: \"007\"\ntitle: Do it\n---\n\n# Do it\n\nthe body\n")
                .unwrap();
        let out = render_prompt(
            "id={id} title={title} path={path}\n{body}",
            &t,
            "/abs/007.md",
        );
        assert_eq!(
            out,
            "id=007 title=Do it path=/abs/007.md\n# Do it\n\nthe body"
        );
    }

    #[test]
    fn resolve_working_dir_precedence() {
        let root = Path::new("/vault");
        let with_override =
            Task::parse_required("---\nid: \"1\"\ntitle: t\nai_working_dir: /repo\n---\n").unwrap();
        assert_eq!(
            resolve_working_dir(&with_override, &RunConfig::default(), root),
            PathBuf::from("/repo")
        );
        let plain = Task::parse_required("---\nid: \"1\"\ntitle: t\n---\n").unwrap();
        let cfg = RunConfig {
            working_dir: Some("/cfgdir".into()),
            ..RunConfig::default()
        };
        assert_eq!(
            resolve_working_dir(&plain, &cfg, root),
            PathBuf::from("/cfgdir")
        );
        assert_eq!(
            resolve_working_dir(&plain, &RunConfig::default(), root),
            PathBuf::from("/vault")
        );
    }

    #[test]
    fn resolve_agent_default_override_and_unknown() {
        let (vault, _) = build_vault(CFG, &[]);
        let cfg = &vault.config.run;
        let plain = Task::parse_required("---\nid: \"1\"\ntitle: t\n---\n").unwrap();
        assert_eq!(resolve_agent(cfg, &plain).unwrap().command[0], "claude");
        let bad = Task::parse_required("---\nid: \"1\"\ntitle: t\nai_agent: ghost\n---\n").unwrap();
        assert!(
            resolve_agent(cfg, &bad)
                .unwrap_err()
                .contains("unknown agent `ghost`")
        );
    }

    #[test]
    fn substitute_prompt_replaces_token() {
        let args = vec![
            "claude".to_string(),
            "{prompt}".to_string(),
            "x".to_string(),
        ];
        assert_eq!(
            substitute_prompt(&args, "{prompt}", "hello world"),
            vec!["claude", "hello world", "x"]
        );
    }

    #[test]
    fn record_failure_park_is_idempotent_on_tag() {
        // Calling with attempts already at cap and the tag present must not
        // duplicate the tag.
        let mut t = Task::parse_required(
            "---\nid: \"1\"\ntitle: t\ntags: [ai-runnable, ai-failed]\nai_attempts: 2\n---\n",
        )
        .unwrap();
        record_failure(&mut t, "boom", today(), 2);
        assert_eq!(
            t.tags().iter().filter(|x| *x == FAILED_TAG).count(),
            1,
            "tag not duplicated"
        );
    }

    #[test]
    fn render_result_line_variants() {
        let ok = RunResult {
            id: "1".into(),
            succeeded: true,
            detail: String::new(),
            parked: false,
        };
        assert!(render_result_line(&ok).contains("-> completed"));
        let parked = RunResult {
            id: "2".into(),
            succeeded: false,
            detail: "x".into(),
            parked: true,
        };
        assert!(render_result_line(&parked).contains("parked (ai-failed)"));
        let failed = RunResult {
            id: "3".into(),
            succeeded: false,
            detail: "y".into(),
            parked: false,
        };
        let line = render_result_line(&failed);
        assert!(line.contains("-> failed: y") && !line.contains("parked"));
    }

    // ---- selection ----

    #[test]
    fn disabled_runs_and_plans_nothing() {
        let cfg = "run:\n  enabled: false\n";
        let (vault, _) = build_vault(cfg, &[("001-a.md", &runnable_task("001"))]);
        assert!(plan(&vault, now()).unwrap().is_empty());
        let runner = FakeRunner::new(Mode::SucceedNoComplete);
        assert!(
            run_all(&vault, &runner, now(), today())
                .unwrap()
                .ran
                .is_empty()
        );
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn plan_selects_only_runnable() {
        let tasks = [
            ("001-run.md", runnable_task("001")),
            (
                "002-untagged.md",
                "---\nid: \"002\"\ntitle: t\nstatus: pending\n---\n".to_string(),
            ),
            (
                "003-done.md",
                "---\nid: \"003\"\ntitle: t\nstatus: completed\ntags: [ai-runnable]\n---\n"
                    .to_string(),
            ),
            (
                "004-failed.md",
                "---\nid: \"004\"\ntitle: t\nstatus: pending\ntags: [ai-runnable, ai-failed]\n---\n"
                    .to_string(),
            ),
            (
                "005-maxed.md",
                "---\nid: \"005\"\ntitle: t\nstatus: pending\ntags: [ai-runnable]\nai_attempts: 2\n---\n"
                    .to_string(),
            ),
        ];
        let refs: Vec<(&str, &str)> = tasks.iter().map(|(a, b)| (*a, b.as_str())).collect();
        let (vault, _) = build_vault(CFG, &refs);
        assert_eq!(plan(&vault, now()).unwrap(), vec!["001"]);
    }

    #[test]
    fn locked_fresh_skipped_stale_run() {
        let fresh = format!(
            "---\nid: \"001\"\ntitle: t\nstatus: pending\ntags: [ai-runnable]\nai_status: running\nai_run_started: {}\n---\n",
            now().to_rfc3339()
        );
        let stale = "---\nid: \"002\"\ntitle: t\nstatus: pending\ntags: [ai-runnable]\nai_status: running\nai_run_started: 2026-07-01T00:00:00Z\n---\n";
        let (vault, _) = build_vault(CFG, &[("001-a.md", &fresh), ("002-b.md", stale)]);
        assert_eq!(plan(&vault, now()).unwrap(), vec!["002"]);
    }

    // ---- orchestration ----

    #[test]
    fn success_clears_markers_and_records() {
        let (vault, dir) = build_vault(CFG, &[("001-fetch.md", &runnable_task("001"))]);
        let path = dir.join("tasks/001-fetch.md");
        let runner = FakeRunner::new(Mode::Complete(path));
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert_eq!(report.ran.len(), 1);
        assert!(report.ran[0].succeeded);
        assert!(!report.ran[0].parked);
        // Markers cleared, task completed.
        let t = reload(&vault, "001");
        assert_eq!(t.status(), Some(Status::Completed));
        assert!(t.get("ai_status").is_none());
        assert!(t.get("ai_attempts").is_none());
        // The runner saw a substituted prompt and the default working dir (vault root).
        let calls = runner.calls.borrow();
        assert_eq!(calls[0].command, vec!["claude", "-p", "{prompt}"]);
        assert!(calls[0].prompt.contains("do 001: Fetch 001"));
        assert!(calls[0].prompt.contains("path="));
        assert_eq!(calls[0].working_dir, vault.root);
        assert_eq!(calls[0].timeout, 100);
    }

    #[test]
    fn exit_zero_without_complete_is_failure() {
        let (vault, _) = build_vault(CFG, &[("001-a.md", &runnable_task("001"))]);
        let runner = FakeRunner::new(Mode::SucceedNoComplete);
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert!(!report.ran[0].succeeded);
        assert!(
            report.ran[0]
                .detail
                .contains("did not mark the task complete")
        );
        let t = reload(&vault, "001");
        assert_eq!(read_attempts(&t), 1);
        assert_eq!(t.get("ai_status").and_then(|v| v.as_str()), Some("failed"));
        assert_eq!(
            t.get("ai_last_run").and_then(|v| v.as_str()),
            Some("2026-07-09")
        );
        assert!(t.get("ai_run_started").is_none());
    }

    #[test]
    fn nonzero_exit_records_failure() {
        let (vault, _) = build_vault(CFG, &[("001-a.md", &runnable_task("001"))]);
        let runner = FakeRunner::new(Mode::Fail("boom".into()));
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert_eq!(report.ran[0].detail, "boom");
        assert_eq!(read_attempts(&reload(&vault, "001")), 1);
    }

    #[test]
    fn unknown_agent_is_a_failure() {
        let task = "---\nid: \"001\"\ntitle: t\nstatus: pending\ntags: [ai-runnable]\nai_agent: ghost\n---\n";
        let (vault, _) = build_vault(CFG, &[("001-a.md", task)]);
        let runner = FakeRunner::new(Mode::SucceedNoComplete);
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert!(report.ran[0].detail.contains("unknown agent `ghost`"));
        // Runner was never called for an unresolvable agent.
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn parks_after_max_attempts_then_excluded() {
        let cfg = "run:\n  enabled: true\n  agent: claude\n  max_attempts: 1\n  agents:\n    claude:\n      command: [claude]\n";
        let (vault, _) = build_vault(cfg, &[("001-a.md", &runnable_task("001"))]);
        let runner = FakeRunner::new(Mode::Fail("nope".into()));
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert!(report.ran[0].parked);
        let t = reload(&vault, "001");
        assert!(t.tags().iter().any(|x| x == FAILED_TAG));
        // Now excluded from a subsequent selection.
        assert!(plan(&vault, now()).unwrap().is_empty());
    }

    #[test]
    fn scan_errors_propagate() {
        // tasks path is a file, not a dir: scan() errors, and both entry points
        // surface it rather than silently doing nothing.
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("karamd-run-err-{uniq}"));
        fs::create_dir_all(&base).unwrap();
        fs::write(
            base.join(".taskmd.yaml"),
            "run:\n  enabled: true\n  agents:\n    claude:\n      command: [c]\n",
        )
        .unwrap();
        fs::write(base.join("tasks"), "not a directory").unwrap();
        let vault = Vault::open(&base).unwrap();
        assert!(plan(&vault, now()).is_err());
        let runner = FakeRunner::new(Mode::SucceedNoComplete);
        assert!(run_all(&vault, &runner, now(), today()).is_err());
    }

    #[test]
    fn working_dir_override_is_used() {
        let task = "---\nid: \"001\"\ntitle: t\nstatus: pending\ntags: [ai-runnable]\nai_working_dir: /custom/repo\n---\n";
        let (vault, _) = build_vault(CFG, &[("001-a.md", task)]);
        let runner = FakeRunner::new(Mode::Fail("x".into()));
        run_all(&vault, &runner, now(), today()).unwrap();
        assert_eq!(
            runner.calls.borrow()[0].working_dir,
            PathBuf::from("/custom/repo")
        );
    }
}
