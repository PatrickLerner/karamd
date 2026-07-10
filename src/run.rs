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
/// Cancel request marker (#046): set by the web while a run is in flight; the
/// concluding run (or a stale sweep) honours it, and it blocks re-selection in
/// the meantime so a second run can't double-execute the task.
const K_CANCEL: &str = "ai_cancel";
const STATUS_RUNNING: &str = "running";
const STATUS_FAILED: &str = "failed";

/// What a single agent invocation produced. Spawn errors and timeouts are
/// encoded as `success: false` with a `detail`, so the runner never fails the
/// whole loop. `exit_code`/`duration_s` feed the per-run log (#045).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentOutcome {
    pub success: bool,
    pub detail: String,
    /// Process exit code when the agent actually ran to completion; `None` for
    /// a spawn failure, timeout, or unresolved agent.
    pub exit_code: Option<i32>,
    /// Wall-clock seconds the agent ran (0 when it never started).
    pub duration_s: i64,
}

/// Spawns one agent. Behind a trait so the orchestration is tested with a fake
/// and only the real process glue ([`crate::run_spawn`]) is excluded. When
/// `log_path` is set the runner tees the agent's stdout/stderr to that file
/// (#045) in addition to inheriting them to the console.
pub trait AgentRunner {
    fn run(
        &self,
        spec: &AgentSpec,
        prompt: &str,
        working_dir: &Path,
        timeout_secs: u64,
        log_path: Option<&Path>,
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
    /// True when a web cancel (#046) ended this run: not a success, not a
    /// failed attempt.
    pub cancelled: bool,
}

/// What a `run` invocation did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunReport {
    pub ran: Vec<RunResult>,
}

/// One durable per-run log record (#045), appended as a line to `runs.jsonl`
/// and pointing at the `.log` file holding the agent's tee'd output.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub agent: String,
    pub command: Vec<String>,
    pub working_dir: String,
    pub started_at: String,
    pub ended_at: String,
    pub duration_s: i64,
    pub attempt: u32,
    pub exit_code: Option<i32>,
    /// `completed` | `failed` | `parked`.
    pub outcome: String,
    pub last_error: Option<String>,
    /// The per-run `.log` filename (relative to the log dir).
    pub log_file: String,
}

/// The per-run `.log` filename for a task, derived from the run start so the
/// web (#046) can recompute it from `ai_run_started` + id.
pub fn run_log_filename(started: DateTime<Utc>, id: &str) -> String {
    format!("{}-{}.log", started.format("%Y%m%dT%H%M%SZ"), id)
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
        // saturating_mul + clamp so an extreme configured timeout can't overflow
        // the u64 multiply (debug panic) or wrap the i64 cast negative.
        Some(t) => {
            let window = timeout_secs.saturating_mul(2).min(i64::MAX as u64) as i64;
            (now - t.with_timezone(&Utc)).num_seconds() >= window
        }
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

/// True when a task shows `running` but its marker is stale (older than
/// `2 * timeout`): a dead run (SIGKILL, reboot mid-run) that should be treated
/// and rewritten as not-running (#048), never displayed as a live run.
pub fn is_running_stale(task: &Task, timeout_secs: u64, now: DateTime<Utc>) -> bool {
    marker_running_stale(
        task.get(K_STATUS).and_then(|v| v.as_str()),
        task.get(K_STARTED).and_then(|v| v.as_str()),
        timeout_secs,
        now,
    )
}

/// Display-side staleness (#048): the same rule as [`is_running_stale`] but over
/// the already-serialized `ai_status`/`ai_run_started` values, so the web read
/// path corrects a ghost run without re-reading the task file.
pub fn marker_running_stale(
    ai_status: Option<&str>,
    ai_run_started: Option<&str>,
    timeout_secs: u64,
    now: DateTime<Utc>,
) -> bool {
    ai_status == Some(STATUS_RUNNING) && is_stale(ai_run_started, now, timeout_secs)
}

/// Clear stale `running` markers left by a run that never finished cleanly
/// (#048): drop `ai_status`/`ai_run_started` so the file, API, and web reflect
/// reality and the task is selectable again. `ai_attempts` is left intact — the
/// cleanup is not a new attempt. Returns the ids rewritten.
pub fn reconcile_stale(vault: &Vault, now: DateTime<Utc>) -> Result<Vec<String>> {
    let timeout = vault.config.run.timeout_secs;
    let scan = vault.scan()?;
    let ids: Vec<String> = scan
        .tasks
        .iter()
        .filter(|t| is_running_stale(t, timeout, now))
        .map(Task::id)
        .collect();
    for id in &ids {
        let mut t = vault.find(id)?;
        t.remove(K_STATUS);
        t.remove(K_STARTED);
        // Also clear an orphaned cancel (#046): the cancelling run died before
        // concluding, so drop the marker rather than block the task forever.
        t.remove(K_CANCEL);
        vault.save(&t)?;
    }
    Ok(ids)
}

/// One in-flight `karamd run` execution, for the web sidebar (#046).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OngoingRun {
    pub id: String,
    pub title: String,
    pub started_at: String,
    pub attempts: u32,
    /// The per-run `.log` file (relative to the log dir) holding its output.
    pub log_file: String,
}

/// The runs currently executing: tasks with a fresh (non-stale) `running`
/// marker and no pending cancel (#046). Ordered by id for a stable sidebar.
pub fn ongoing(vault: &Vault, now: DateTime<Utc>) -> Result<Vec<OngoingRun>> {
    let timeout = vault.config.run.timeout_secs;
    let mut runs: Vec<OngoingRun> = vault
        .scan()?
        .tasks
        .iter()
        .filter_map(|t| {
            if t.get(K_STATUS).and_then(|v| v.as_str()) != Some(STATUS_RUNNING) {
                return None;
            }
            if t.get(K_CANCEL).is_some() {
                return None;
            }
            let started = t.get(K_STARTED).and_then(|v| v.as_str())?;
            let start_dt = DateTime::parse_from_rfc3339(started)
                .ok()?
                .with_timezone(&Utc);
            if is_stale(Some(started), now, timeout) {
                return None;
            }
            Some(OngoingRun {
                id: t.id(),
                title: t.title(),
                started_at: started.to_string(),
                attempts: read_attempts(t),
                log_file: run_log_filename(start_dt, &t.id()),
            })
        })
        .collect();
    runs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(runs)
}

/// Tail of a per-run `.log` file (#046), at most `max_bytes` from the end.
/// A missing file (agent produced nothing yet, or logging was off) reads as
/// empty rather than an error, so the web log view degrades gracefully.
pub fn run_log_tail(log_dir: &Path, log_file: &str, max_bytes: usize) -> String {
    let bytes = std::fs::read(log_dir.join(log_file)).unwrap_or_default();
    let start = bytes.len().saturating_sub(max_bytes);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

/// Request cancellation of an in-flight run (#046): stamp the cancel marker so
/// the concluding run (or a stale sweep) clears it without counting a failed
/// attempt, and it drops out of the sidebar and the selectable set immediately.
/// Returns whether a running task was actually marked (false for an unknown or
/// not-running task).
pub fn cancel_run(vault: &Vault, id: &str, now: DateTime<Utc>) -> Result<bool> {
    let mut task = vault.find(id)?;
    if task.get(K_STATUS).and_then(|v| v.as_str()) != Some(STATUS_RUNNING) {
        return Ok(false);
    }
    task.set(K_CANCEL, Value::String(now.to_rfc3339()));
    vault.save(&task)?;
    Ok(true)
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
        // A pending cancel (#046) blocks re-selection until the current run
        // concludes and clears it, so a cancelled task is never double-picked.
        && task.get(K_CANCEL).is_none()
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
    if r.cancelled {
        format!("karamd: ran {} -> cancelled", r.id)
    } else if r.succeeded {
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

/// Append one JSON line to the run index, creating the file if needed.
fn append_jsonl(path: &Path, line: &str) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

/// Persist one run record (#045): ensure the log dir exists, append the record
/// as a line to `runs.jsonl`, then prune to `retention`. Best-effort: the
/// caller warns and continues on error rather than failing the run.
pub fn write_run_log(log_dir: &Path, record: &RunRecord, retention: usize) -> Result<()> {
    std::fs::create_dir_all(log_dir)?;
    let line = serde_json::to_string(record)?;
    append_jsonl(&log_dir.join("runs.jsonl"), &line)?;
    prune_logs(log_dir, retention)?;
    Ok(())
}

/// Keep only the `keep` most-recent records in `runs.jsonl`, deleting the `.log`
/// files of the records that fall off the end. `keep == 0` disables pruning.
///
/// Only the dropped records' own `.log` files are removed, and only when no
/// retained record still points at them: karamd never deletes a file it didn't
/// record (so a shared/misconfigured `log_dir` keeps its foreign files) nor a
/// concurrent run's not-yet-indexed `.log`.
pub fn prune_logs(log_dir: &Path, keep: usize) -> Result<()> {
    if keep == 0 {
        return Ok(());
    }
    let index = log_dir.join("runs.jsonl");
    let content = std::fs::read_to_string(&index)?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= keep {
        return Ok(());
    }
    let (dropped, kept) = lines.split_at(lines.len() - keep);
    // Log files a retained record still points at must survive even if an older
    // dropped record referenced the same file.
    let kept_refs: std::collections::HashSet<String> = kept
        .iter()
        .filter_map(|l| serde_json::from_str::<RunRecord>(l).ok())
        .map(|r| r.log_file)
        .collect();
    // Rewrite the index to the kept lines. A pid + per-call counter temp name
    // avoids two concurrent prunes (another process, or another #042 batch
    // thread in this one) clobbering a shared temp path.
    static PRUNE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = PRUNE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = log_dir.join(format!("runs.jsonl.{}-{seq}.tmp", std::process::id()));
    std::fs::write(&tmp, format!("{}\n", kept.join("\n")))?;
    std::fs::rename(&tmp, &index)?;
    // Delete only the dropped records' own log files (skipping unparseable
    // lines), never anything a kept record still references.
    for line in dropped {
        if let Ok(r) = serde_json::from_str::<RunRecord>(line)
            && !kept_refs.contains(&r.log_file)
        {
            let _ = std::fs::remove_file(log_dir.join(&r.log_file));
        }
    }
    Ok(())
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

/// Choose up to `n` ids to run concurrently this round, at most one per resolved
/// working dir (#042): tasks sharing a working dir would clobber the same repo,
/// so only the first candidate for each dir joins the batch; the rest wait for a
/// later round (after the first frees the dir). `candidates` is in scan order.
pub fn pick_batch(candidates: &[(String, PathBuf)], n: usize) -> Vec<String> {
    let mut seen_dirs: std::collections::HashSet<&PathBuf> = std::collections::HashSet::new();
    let mut batch = Vec::new();
    for (id, dir) in candidates {
        if batch.len() >= n {
            break;
        }
        if seen_dirs.insert(dir) {
            batch.push(id.clone());
        }
    }
    batch
}

/// Run a batch of tasks (distinct working dirs, so safe to parallelise). A
/// single-task batch runs inline; a larger one fans out over a scoped thread per
/// task. Every task's writes go to its own file, so the only shared sink is the
/// per-run log index, whose append is atomic.
fn run_batch(
    vault: &Vault,
    cfg: &RunConfig,
    log_dir: &Path,
    runner: &(dyn AgentRunner + Sync),
    ids: &[String],
    now: DateTime<Utc>,
    today: NaiveDate,
) -> Result<Vec<RunResult>> {
    if ids.len() == 1 {
        let r = run_one(vault, cfg, log_dir, runner, &ids[0], now, today)?;
        return Ok(vec![r]);
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = ids
            .iter()
            .map(|id| scope.spawn(move || run_one(vault, cfg, log_dir, runner, id, now, today)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("run_one thread panicked"))
            .collect()
    })
}

/// Run a single task end to end: mark running (pre-increment attempts), render
/// the prompt from the staged task, spawn via `runner`, re-read the (possibly
/// agent-modified) task, record success (exit 0 *and* the agent moved the task
/// to a terminal status) or failure, and append the per-run log record (#045).
/// All writes go through a re-read then save, so a concurrent sync is not
/// clobbered.
fn run_one(
    vault: &Vault,
    cfg: &RunConfig,
    log_dir: &Path,
    runner: &(dyn AgentRunner + Sync),
    id: &str,
    now: DateTime<Utc>,
    today: NaiveDate,
) -> Result<RunResult> {
    // Real wall-clock start of this task, for the audit record: with the
    // re-scan loop (#049) tasks run sequentially over time, so the per-invocation
    // tick `now` (used for the lock marker) is not each task's true start.
    let started = Utc::now();
    let mut staged = vault.find(id)?;
    mark_running(&mut staged, now);
    vault.save(&staged)?;

    let abs_path = staged
        .rel_path
        .as_ref()
        .map(|p| vault.tasks_dir().join(p))
        .unwrap_or_default();
    let prompt = render_prompt(&cfg.prompt_template, &staged, &abs_path.to_string_lossy());
    let working_dir = resolve_working_dir(&staged, cfg, &vault.root);
    let agent_name = staged
        .get(K_AGENT)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| cfg.agent.clone());
    let log_file = run_log_filename(now, id);
    let log_path = log_dir.join(&log_file);

    // The resolved agent supplies the command for the log; an unknown agent
    // is a failure that never spawns, with no exit code or output file.
    let (outcome, command) = match resolve_agent(cfg, &staged) {
        Ok(spec) => (
            runner.run(
                spec,
                &prompt,
                &working_dir,
                cfg.timeout_secs,
                Some(&log_path),
            ),
            spec.command.clone(),
        ),
        Err(detail) => (
            AgentOutcome {
                success: false,
                detail,
                ..Default::default()
            },
            Vec::new(),
        ),
    };

    // Re-read post-agent: the agent may have run `karamd complete`, so the
    // status on disk is the source of truth for the success check.
    let mut after = vault.find(id)?;
    // The attempt number as run (post-increment), captured before
    // record_success clears the counter.
    let attempt = read_attempts(&after);

    // A cancel requested mid-run (#046): clear the run markers, refund the
    // pre-incremented attempt (a cancel is not a failed attempt), and record the
    // outcome as cancelled rather than success/failure.
    if after.get(K_CANCEL).is_some() {
        after.remove(K_STATUS);
        after.remove(K_STARTED);
        after.remove(K_CANCEL);
        after.remove(K_LAST_ERROR);
        let refunded = attempt.saturating_sub(1);
        if refunded == 0 {
            after.remove(K_ATTEMPTS);
        } else {
            after.set(K_ATTEMPTS, Value::from(refunded as u64));
        }
        vault.save(&after)?;
        let record = RunRecord {
            id: id.to_string(),
            agent: agent_name,
            command,
            working_dir: working_dir.to_string_lossy().into_owned(),
            started_at: started.to_rfc3339(),
            ended_at: (started + chrono::Duration::seconds(outcome.duration_s)).to_rfc3339(),
            duration_s: outcome.duration_s,
            attempt,
            exit_code: outcome.exit_code,
            outcome: "cancelled".to_string(),
            last_error: None,
            log_file,
        };
        if let Err(e) = write_run_log(log_dir, &record, cfg.log_retention) {
            eprintln!("karamd: warning: run log write failed for {id}: {e}");
        }
        return Ok(RunResult {
            id: id.to_string(),
            succeeded: false,
            detail: "cancelled".to_string(),
            parked: false,
            cancelled: true,
        });
    }

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

    // Durable per-run record (#045). Best-effort: a log failure must not
    // abort the run or lose the outcome.
    let outcome_str = if succeeded {
        "completed"
    } else if parked {
        "parked"
    } else {
        "failed"
    };
    let record = RunRecord {
        id: id.to_string(),
        agent: agent_name,
        command,
        working_dir: working_dir.to_string_lossy().into_owned(),
        started_at: started.to_rfc3339(),
        ended_at: (started + chrono::Duration::seconds(outcome.duration_s)).to_rfc3339(),
        duration_s: outcome.duration_s,
        attempt,
        exit_code: outcome.exit_code,
        outcome: outcome_str.to_string(),
        last_error: (!detail.is_empty()).then(|| detail.clone()),
        log_file,
    };
    if let Err(e) = write_run_log(log_dir, &record, cfg.log_retention) {
        eprintln!("karamd: warning: run log write failed for {id}: {e}");
    }

    Ok(RunResult {
        id: id.to_string(),
        succeeded,
        detail,
        parked,
        cancelled: false,
    })
}

/// Execute runnable tasks, re-scanning after each so tasks that become eligible
/// mid-invocation are drained too (#049), until nothing new is runnable.
///
/// A task is run at most once per invocation (tracked in `done`), so a task that
/// fails but is still under `max_attempts` is retried on the *next* invocation,
/// not immediately in a tight loop. `run.max_per_invocation` caps the total to
/// bound a pathological rule/agent that keeps spawning new runnable tasks; the
/// remainder is deferred to the next run with a clear log line. Results are
/// recorded in the order tasks were run.
pub fn run_all(
    vault: &Vault,
    runner: &(dyn AgentRunner + Sync),
    now: DateTime<Utc>,
    today: NaiveDate,
) -> Result<RunReport> {
    let cfg = &vault.config.run;
    let mut report = RunReport::default();
    if !cfg.enabled {
        return Ok(report);
    }
    let log_dir = cfg.resolve_log_dir(&vault.root);
    // Create the log dir up front so the runner can open a per-run log file on
    // the very first run against a fresh vault (#045). Best-effort: if it fails,
    // the runner falls back to no capture and the run still proceeds.
    let _ = std::fs::create_dir_all(&log_dir);
    // Rewrite ghost `running` markers from crashed prior runs before selecting,
    // so a dead run never lingers as "running" (#048).
    reconcile_stale(vault, now)?;

    let cap = cfg.max_per_invocation;
    let concurrency = cfg.concurrency.max(1);
    let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
    loop {
        if cap != 0 && report.ran.len() >= cap {
            // Re-scan once more only to report whether anything was left behind.
            let remaining = vault
                .scan()?
                .tasks
                .iter()
                .any(|t| is_runnable(t, cfg, now) && !done.contains(&t.id()));
            if remaining {
                eprintln!(
                    "karamd: reached max_per_invocation ({cap}); deferring remaining runnable tasks to the next run"
                );
            }
            break;
        }
        // Re-scan every round (#049) so tasks that became runnable during the
        // previous round are picked up; skip anything already run this run.
        let scan = vault.scan()?;
        let candidates: Vec<(String, PathBuf)> = scan
            .tasks
            .iter()
            .filter(|t| is_runnable(t, cfg, now) && !done.contains(&t.id()))
            .map(|t| (t.id(), resolve_working_dir(t, cfg, &vault.root)))
            .collect();
        // Up to `concurrency` distinct-working-dir tasks per round (#042), never
        // exceeding the remaining `max_per_invocation` budget (#049).
        let budget = if cap == 0 {
            concurrency
        } else {
            concurrency.min(cap - report.ran.len())
        };
        let batch = pick_batch(&candidates, budget);
        if batch.is_empty() {
            break;
        }
        for id in &batch {
            done.insert(id.clone());
        }
        report
            .ran
            .extend(run_batch(vault, cfg, &log_dir, runner, &batch, now, today)?);
    }
    // Deterministic output regardless of concurrent completion order (#042).
    report.ran.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskmd::Status;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;
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
        log_path: Option<PathBuf>,
    }

    struct FakeRunner {
        mode: Mode,
        // Mutex (not RefCell) so the runner is Sync and can be shared across the
        // scoped threads a concurrent batch (#042) uses.
        calls: Mutex<Vec<Recorded>>,
    }

    impl FakeRunner {
        fn new(mode: Mode) -> FakeRunner {
            FakeRunner {
                mode,
                calls: Mutex::new(Vec::new()),
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
            log_path: Option<&Path>,
        ) -> AgentOutcome {
            self.calls.lock().unwrap().push(Recorded {
                command: spec.command.clone(),
                prompt: prompt.to_string(),
                working_dir: working_dir.to_path_buf(),
                timeout: timeout_secs,
                log_path: log_path.map(Path::to_path_buf),
            });
            // Simulate the tee: drop some captured output at the log path so the
            // orchestration's per-run logging (#045) has a real file to point at.
            // Best-effort, like the real runner: an unwritable path is ignored.
            if let Some(p) = log_path {
                let _ = p.parent().map(std::fs::create_dir_all);
                let _ = fs::write(p, b"fake agent output\n");
            }
            match &self.mode {
                Mode::Complete(path) => {
                    let mut t = Task::parse_required(&fs::read_to_string(path).unwrap()).unwrap();
                    t.set_status(Status::Completed, today());
                    fs::write(path, t.to_markdown()).unwrap();
                    AgentOutcome {
                        success: true,
                        detail: String::new(),
                        exit_code: Some(0),
                        duration_s: 7,
                    }
                }
                Mode::SucceedNoComplete => AgentOutcome {
                    success: true,
                    exit_code: Some(0),
                    ..Default::default()
                },
                Mode::Fail(d) => AgentOutcome {
                    success: false,
                    detail: d.clone(),
                    exit_code: Some(1),
                    duration_s: 3,
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
            cancelled: false,
        };
        assert!(render_result_line(&ok).contains("-> completed"));
        let parked = RunResult {
            id: "2".into(),
            succeeded: false,
            detail: "x".into(),
            parked: true,
            cancelled: false,
        };
        assert!(render_result_line(&parked).contains("parked (ai-failed)"));
        let failed = RunResult {
            id: "3".into(),
            succeeded: false,
            detail: "y".into(),
            parked: false,
            cancelled: false,
        };
        let line = render_result_line(&failed);
        assert!(line.contains("-> failed: y") && !line.contains("parked"));
        let cancelled = RunResult {
            id: "4".into(),
            succeeded: false,
            detail: "cancelled".into(),
            parked: false,
            cancelled: true,
        };
        assert!(render_result_line(&cancelled).contains("-> cancelled"));
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
        assert!(runner.calls.lock().unwrap().is_empty());
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

    #[test]
    fn is_running_stale_detects_dead_runs() {
        let fresh = Task::parse_required(&format!(
            "---\nid: \"1\"\ntitle: t\nai_status: running\nai_run_started: {}\n---\n",
            now().to_rfc3339()
        ))
        .unwrap();
        assert!(!is_running_stale(&fresh, 100, now()));
        let stale = Task::parse_required(
            "---\nid: \"1\"\ntitle: t\nai_status: running\nai_run_started: 2026-07-01T00:00:00Z\n---\n",
        )
        .unwrap();
        assert!(is_running_stale(&stale, 100, now()));
        // A non-running status is never a stale run, however old the timestamp.
        let failed = Task::parse_required(
            "---\nid: \"1\"\ntitle: t\nai_status: failed\nai_run_started: 2026-07-01T00:00:00Z\n---\n",
        )
        .unwrap();
        assert!(!is_running_stale(&failed, 100, now()));
    }

    #[test]
    fn reconcile_stale_clears_ghost_markers_keeping_attempts() {
        let stale = "---\nid: \"001\"\ntitle: t\nstatus: pending\ntags: [ai-runnable]\nai_status: running\nai_run_started: 2026-07-01T00:00:00Z\nai_attempts: 2\n---\n";
        let fresh = format!(
            "---\nid: \"002\"\ntitle: t\nstatus: pending\ntags: [ai-runnable]\nai_status: running\nai_run_started: {}\n---\n",
            now().to_rfc3339()
        );
        let (vault, _) = build_vault(CFG, &[("001-a.md", stale), ("002-b.md", &fresh)]);
        assert_eq!(reconcile_stale(&vault, now()).unwrap(), vec!["001"]);
        let t = reload(&vault, "001");
        assert!(t.get("ai_status").is_none());
        assert!(t.get("ai_run_started").is_none());
        assert_eq!(read_attempts(&t), 2, "cleanup is not a new attempt");
        // The genuinely fresh run is left running.
        assert_eq!(
            reload(&vault, "002")
                .get("ai_status")
                .and_then(|v| v.as_str()),
            Some("running")
        );
    }

    #[test]
    fn run_all_reconciles_ghost_even_at_max_attempts() {
        // Crashed at the attempt cap: not re-run, but its ghost marker is still
        // cleared so it never shows as "running" again.
        let stale = "---\nid: \"001\"\ntitle: t\nstatus: pending\ntags: [ai-runnable]\nai_status: running\nai_run_started: 2026-07-01T00:00:00Z\nai_attempts: 2\n---\n";
        let (vault, _) = build_vault(CFG, &[("001-a.md", stale)]);
        let runner = FakeRunner::new(Mode::SucceedNoComplete);
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert!(report.ran.is_empty());
        assert!(runner.calls.lock().unwrap().is_empty());
        assert!(reload(&vault, "001").get("ai_status").is_none());
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
        let calls = runner.calls.lock().unwrap();
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
        assert!(runner.calls.lock().unwrap().is_empty());
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
            runner.calls.lock().unwrap()[0].working_dir,
            PathBuf::from("/custom/repo")
        );
    }

    // ---- per-run logs (#045) ----

    fn read_records(log_dir: &Path) -> Vec<RunRecord> {
        fs::read_to_string(log_dir.join("runs.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn record(id: &str, log: &str) -> RunRecord {
        RunRecord {
            id: id.into(),
            agent: "claude".into(),
            command: vec!["claude".into()],
            working_dir: "/w".into(),
            started_at: "2026-07-09T12:00:00Z".into(),
            ended_at: "2026-07-09T12:00:00Z".into(),
            duration_s: 0,
            attempt: 1,
            exit_code: None,
            outcome: "completed".into(),
            last_error: None,
            log_file: log.into(),
        }
    }

    #[test]
    fn run_log_filename_is_timestamp_and_id() {
        assert_eq!(run_log_filename(now(), "042"), "20260709T120000Z-042.log");
    }

    #[test]
    fn success_writes_run_log_record_and_output() {
        let (vault, dir) = build_vault(CFG, &[("001-fetch.md", &runnable_task("001"))]);
        let path = dir.join("tasks/001-fetch.md");
        let runner = FakeRunner::new(Mode::Complete(path));
        run_all(&vault, &runner, now(), today()).unwrap();
        // The runner was handed a log path to tee into.
        assert!(runner.calls.lock().unwrap()[0].log_path.is_some());
        let log_dir = dir.join(".karamd/runs");
        let recs = read_records(&log_dir);
        assert_eq!(recs.len(), 1);
        let r = &recs[0];
        assert_eq!(r.id, "001");
        assert_eq!(r.outcome, "completed");
        assert_eq!(r.exit_code, Some(0));
        assert_eq!(r.duration_s, 7);
        assert_eq!(r.attempt, 1);
        assert_eq!(r.agent, "claude");
        assert_eq!(r.command, vec!["claude", "-p", "{prompt}"]);
        assert!(r.last_error.is_none());
        assert_ne!(r.started_at, r.ended_at, "7s duration moves the end time");
        // The per-run .log holds the tee'd bytes.
        let out = fs::read_to_string(log_dir.join(&r.log_file)).unwrap();
        assert_eq!(out, "fake agent output\n");
    }

    #[test]
    fn failure_run_log_records_error() {
        let (vault, dir) = build_vault(CFG, &[("001-a.md", &runnable_task("001"))]);
        let runner = FakeRunner::new(Mode::Fail("boom".into()));
        run_all(&vault, &runner, now(), today()).unwrap();
        let recs = read_records(&dir.join(".karamd/runs"));
        assert_eq!(recs[0].outcome, "failed");
        assert_eq!(recs[0].exit_code, Some(1));
        assert_eq!(recs[0].last_error.as_deref(), Some("boom"));
    }

    #[test]
    fn parked_run_log_records_parked_outcome() {
        let cfg = "run:\n  enabled: true\n  agent: claude\n  max_attempts: 1\n  agents:\n    claude:\n      command: [claude]\n";
        let (vault, dir) = build_vault(cfg, &[("001-a.md", &runnable_task("001"))]);
        let runner = FakeRunner::new(Mode::Fail("nope".into()));
        run_all(&vault, &runner, now(), today()).unwrap();
        assert_eq!(read_records(&dir.join(".karamd/runs"))[0].outcome, "parked");
    }

    #[test]
    fn run_log_write_failure_is_nonfatal() {
        // log_dir points under a regular file, so create_dir_all fails; the run
        // must still be recorded (best-effort logging).
        let dir = tempdir();
        fs::write(dir.join("blocker"), "x").unwrap();
        let cfg = format!(
            "run:\n  enabled: true\n  agent: claude\n  max_attempts: 2\n  log_dir: {}\n  agents:\n    claude:\n      command: [claude]\n",
            dir.join("blocker/sub").display()
        );
        fs::write(dir.join(".taskmd.yaml"), cfg).unwrap();
        fs::write(dir.join("tasks/001-a.md"), runnable_task("001")).unwrap();
        let vault = Vault::open(&dir).unwrap();
        let runner = FakeRunner::new(Mode::Fail("x".into()));
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert_eq!(report.ran.len(), 1);
    }

    #[test]
    fn prune_logs_keeps_recent_and_deletes_orphans() {
        let dir = tempdir();
        let log_dir = dir.join("logs");
        fs::create_dir_all(&log_dir).unwrap();
        let lines = format!(
            "{}\n{}\n{}\n",
            serde_json::to_string(&record("1", "a.log")).unwrap(),
            serde_json::to_string(&record("2", "b.log")).unwrap(),
            serde_json::to_string(&record("3", "c.log")).unwrap(),
        );
        fs::write(log_dir.join("runs.jsonl"), lines).unwrap();
        for f in ["a.log", "b.log", "c.log"] {
            fs::write(log_dir.join(f), "x").unwrap();
        }
        prune_logs(&log_dir, 1).unwrap();
        let idx = fs::read_to_string(log_dir.join("runs.jsonl")).unwrap();
        assert_eq!(idx.lines().count(), 1);
        assert!(idx.contains("c.log"));
        assert!(!log_dir.join("a.log").exists());
        assert!(!log_dir.join("b.log").exists());
        assert!(log_dir.join("c.log").exists());
    }

    #[test]
    fn prune_logs_zero_noop_and_leaves_foreign_files() {
        let dir = tempdir();
        let log_dir = dir.join("logs");
        fs::create_dir_all(&log_dir).unwrap();
        fs::write(
            log_dir.join("runs.jsonl"),
            format!(
                "{}\n",
                serde_json::to_string(&record("1", "a.log")).unwrap()
            ),
        )
        .unwrap();
        // A .log karamd did not record (shared/foreign dir, or a concurrent run
        // whose record is not yet appended).
        fs::write(log_dir.join("foreign.log"), "external").unwrap();
        // keep = 0: no pruning at all.
        prune_logs(&log_dir, 0).unwrap();
        assert!(log_dir.join("foreign.log").exists());
        // keep >= line count: nothing dropped, so nothing deleted; the foreign
        // file is never touched.
        prune_logs(&log_dir, 5).unwrap();
        assert!(log_dir.join("foreign.log").exists());
        // A missing index with keep > 0 surfaces the read error.
        assert!(prune_logs(&dir.join("nope"), 5).is_err());
    }

    #[test]
    fn prune_logs_deletes_only_dropped_and_keeps_referenced() {
        let dir = tempdir();
        let log_dir = dir.join("logs");
        fs::create_dir_all(&log_dir).unwrap();
        // Four lines: a corrupt one, then two pointing at a shared log, then one
        // at c.log. keep = 2 drops the first two.
        let l = |id: &str, log: &str| serde_json::to_string(&record(id, log)).unwrap();
        let lines = format!(
            "{}\n{}\n{}\n{}\n",
            "{not json",
            l("1", "shared.log"),
            l("2", "shared.log"),
            l("3", "c.log"),
        );
        fs::write(log_dir.join("runs.jsonl"), lines).unwrap();
        for f in ["shared.log", "c.log"] {
            fs::write(log_dir.join(f), "x").unwrap();
        }
        prune_logs(&log_dir, 2).unwrap();
        // The corrupt dropped line is skipped (no panic, no delete). shared.log
        // is dropped by record 1 but still referenced by kept record 2, so it
        // survives. The index is trimmed to the last two lines.
        assert!(log_dir.join("shared.log").exists());
        assert!(log_dir.join("c.log").exists());
        assert_eq!(
            fs::read_to_string(log_dir.join("runs.jsonl"))
                .unwrap()
                .lines()
                .count(),
            2
        );
    }

    #[test]
    fn write_run_log_appends_across_runs() {
        let dir = tempdir();
        let log_dir = dir.join("runs"); // does not exist yet
        write_run_log(&log_dir, &record("1", "a.log"), 0).unwrap();
        write_run_log(&log_dir, &record("2", "b.log"), 0).unwrap();
        assert_eq!(read_records(&log_dir).len(), 2);
    }

    #[test]
    fn write_run_log_errors_on_bad_paths() {
        // Parent is a regular file: create_dir_all fails.
        let dir = tempdir();
        fs::write(dir.join("f"), "x").unwrap();
        assert!(write_run_log(&dir.join("f/sub"), &record("1", "a.log"), 0).is_err());
        // Index path is a directory: the append open fails.
        let log_dir = dir.join("runs");
        fs::create_dir_all(&log_dir).unwrap();
        fs::create_dir(log_dir.join("runs.jsonl")).unwrap();
        assert!(write_run_log(&log_dir, &record("1", "a.log"), 0).is_err());
    }

    // ---- re-scan the runnable set after each task (#049) ----

    /// A runner that materialises a second runnable task the first time it runs
    /// (simulating a task appearing mid-invocation), and never completes a task
    /// (so each stays runnable but is guarded by the once-per-invocation set).
    struct SpawningRunner {
        tasks_dir: PathBuf,
        calls: Mutex<u32>,
    }

    impl AgentRunner for SpawningRunner {
        fn run(
            &self,
            _spec: &AgentSpec,
            _prompt: &str,
            _working_dir: &Path,
            _timeout_secs: u64,
            _log_path: Option<&Path>,
        ) -> AgentOutcome {
            let mut n = self.calls.lock().unwrap();
            if *n == 0 {
                fs::write(self.tasks_dir.join("002-b.md"), runnable_task("002")).unwrap();
            }
            *n += 1;
            AgentOutcome {
                success: true,
                ..Default::default()
            }
        }
    }

    #[test]
    fn run_all_rescans_and_drains_tasks_appearing_mid_run() {
        let (vault, dir) = build_vault(CFG, &[("001-a.md", &runnable_task("001"))]);
        let runner = SpawningRunner {
            tasks_dir: dir.join("tasks"),
            calls: Mutex::new(0),
        };
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        let ran: Vec<&str> = report.ran.iter().map(|r| r.id.as_str()).collect();
        // 002 appeared while 001 was running and was drained in the same run.
        assert_eq!(ran, vec!["001", "002"]);
        // Each ran exactly once: a failed-but-under-cap task is NOT retried
        // immediately (it is deferred to the next invocation).
        assert_eq!(*runner.calls.lock().unwrap(), 2);
    }

    #[test]
    fn run_all_caps_tasks_per_invocation() {
        let cfg = "run:\n  enabled: true\n  agent: claude\n  max_attempts: 2\n  max_per_invocation: 1\n  agents:\n    claude:\n      command: [claude]\n";
        let (vault, _) = build_vault(
            cfg,
            &[
                ("001-a.md", &runnable_task("001")),
                ("002-b.md", &runnable_task("002")),
            ],
        );
        let runner = FakeRunner::new(Mode::SucceedNoComplete);
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        // The cap stops after one task even though two were runnable.
        assert_eq!(report.ran.len(), 1);
        assert_eq!(runner.calls.lock().unwrap().len(), 1);
    }

    // ---- concurrency (#042) ----

    #[test]
    fn pick_batch_one_per_dir_up_to_n() {
        let c = vec![
            ("001".to_string(), PathBuf::from("/a")),
            ("002".to_string(), PathBuf::from("/a")), // same dir as 001
            ("003".to_string(), PathBuf::from("/b")),
            ("004".to_string(), PathBuf::from("/c")),
        ];
        // n=3: 002 is skipped (its dir is already taken by 001).
        assert_eq!(pick_batch(&c, 3), vec!["001", "003", "004"]);
        // n=1: just the first candidate.
        assert_eq!(pick_batch(&c, 1), vec!["001"]);
        // n=0 and no candidates both yield an empty batch.
        assert!(pick_batch(&c, 0).is_empty());
        assert!(pick_batch(&[], 5).is_empty());
    }

    fn dir_task(id: &str, dir: &str) -> String {
        format!(
            "---\nid: \"{id}\"\ntitle: t\nstatus: pending\ntags: [ai-runnable]\nai_working_dir: {dir}\n---\n"
        )
    }

    #[test]
    fn run_all_runs_distinct_working_dirs_concurrently() {
        // concurrency 2 + two distinct dirs = one concurrent batch (the threaded
        // run_batch path). Output is sorted by id regardless of finish order.
        let cfg = "run:\n  enabled: true\n  agent: claude\n  max_attempts: 2\n  concurrency: 2\n  agents:\n    claude:\n      command: [claude]\n";
        let (vault, _) = build_vault(
            cfg,
            &[
                ("001-a.md", &dir_task("001", "/tmp/karamd-ta")),
                ("002-b.md", &dir_task("002", "/tmp/karamd-tb")),
            ],
        );
        let runner = FakeRunner::new(Mode::SucceedNoComplete);
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        let ran: Vec<&str> = report.ran.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ran, vec!["001", "002"]);
        assert_eq!(runner.calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn run_all_unlimited_cap_drains_all() {
        // max_per_invocation: 0 = no cap; the loop never breaks on the cap and
        // the per-round budget is just `concurrency`.
        let cfg = "run:\n  enabled: true\n  agent: claude\n  max_attempts: 2\n  max_per_invocation: 0\n  agents:\n    claude:\n      command: [claude]\n";
        let (vault, _) = build_vault(
            cfg,
            &[
                ("001-a.md", &runnable_task("001")),
                ("002-b.md", &runnable_task("002")),
            ],
        );
        let runner = FakeRunner::new(Mode::SucceedNoComplete);
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert_eq!(report.ran.len(), 2);
    }

    #[test]
    fn run_all_serializes_shared_working_dir() {
        // Both tasks resolve to the vault root (no override), so even at
        // concurrency 2 they run one-per-round, never in the same batch.
        let cfg = "run:\n  enabled: true\n  agent: claude\n  max_attempts: 2\n  concurrency: 2\n  agents:\n    claude:\n      command: [claude]\n";
        let (vault, _) = build_vault(
            cfg,
            &[
                ("001-a.md", &runnable_task("001")),
                ("002-b.md", &runnable_task("002")),
            ],
        );
        let runner = FakeRunner::new(Mode::SucceedNoComplete);
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert_eq!(
            report.ran.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["001", "002"]
        );
        assert_eq!(runner.calls.lock().unwrap().len(), 2);
    }

    // ---- ongoing runs, log tail, cancel (#046) ----

    #[test]
    fn ongoing_lists_fresh_uncancelled_running() {
        let fresh = format!(
            "---\nid: \"001\"\ntitle: Live\ntags: [ai-runnable]\nai_status: running\nai_run_started: {}\nai_attempts: 1\n---\n",
            now().to_rfc3339()
        );
        let stale = "---\nid: \"002\"\ntitle: Dead\ntags: [ai-runnable]\nai_status: running\nai_run_started: 2020-01-01T00:00:00Z\n---\n";
        let cancelled = format!(
            "---\nid: \"003\"\ntitle: Cx\ntags: [ai-runnable]\nai_status: running\nai_run_started: {}\nai_cancel: {}\n---\n",
            now().to_rfc3339(),
            now().to_rfc3339()
        );
        let idle = "---\nid: \"004\"\ntitle: Idle\ntags: [ai-runnable]\n---\n";
        let no_start =
            "---\nid: \"005\"\ntitle: NoStart\ntags: [ai-runnable]\nai_status: running\n---\n";
        let bad_start = "---\nid: \"006\"\ntitle: Bad\ntags: [ai-runnable]\nai_status: running\nai_run_started: not-a-date\n---\n";
        // A second fresh running task, filed after 001 so the sort has work to do.
        let fresh2 = format!(
            "---\nid: \"000\"\ntitle: Also\ntags: [ai-runnable]\nai_status: running\nai_run_started: {}\n---\n",
            now().to_rfc3339()
        );
        let (vault, _) = build_vault(
            CFG,
            &[
                ("001.md", &fresh),
                ("002.md", stale),
                ("003.md", &cancelled),
                ("004.md", idle),
                ("005.md", no_start),
                ("006.md", bad_start),
                ("000.md", &fresh2),
            ],
        );
        let runs = ongoing(&vault, now()).unwrap();
        // Only the fresh, uncancelled, parseable-start running tasks qualify,
        // sorted by id.
        assert_eq!(
            runs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["000", "001"]
        );
        let live = runs.iter().find(|r| r.id == "001").unwrap();
        assert_eq!(live.title, "Live");
        assert_eq!(live.attempts, 1);
        assert_eq!(live.log_file, run_log_filename(now(), "001"));
    }

    #[test]
    fn run_log_tail_reads_tail_and_missing() {
        let dir = tempdir();
        fs::write(dir.join("x.log"), "hello world").unwrap();
        assert_eq!(run_log_tail(&dir, "x.log", 5), "world");
        assert_eq!(run_log_tail(&dir, "x.log", 100), "hello world");
        // A missing log file degrades to empty, not an error.
        assert_eq!(run_log_tail(&dir, "missing.log", 100), "");
    }

    #[test]
    fn cancel_run_marks_only_running_tasks() {
        let running = format!(
            "---\nid: \"001\"\ntitle: t\ntags: [ai-runnable]\nai_status: running\nai_run_started: {}\n---\n",
            now().to_rfc3339()
        );
        let idle = "---\nid: \"002\"\ntitle: t\ntags: [ai-runnable]\n---\n";
        let (vault, _) = build_vault(CFG, &[("001-a.md", &running), ("002-b.md", idle)]);
        assert!(cancel_run(&vault, "001", now()).unwrap());
        assert!(reload(&vault, "001").get("ai_cancel").is_some());
        // A not-running task has nothing to cancel.
        assert!(!cancel_run(&vault, "002", now()).unwrap());
        // An unknown id errors.
        assert!(cancel_run(&vault, "999", now()).is_err());
    }

    #[test]
    fn cancel_marker_blocks_selection() {
        let task = format!(
            "---\nid: \"001\"\ntitle: t\nstatus: pending\ntags: [ai-runnable]\nai_cancel: {}\n---\n",
            now().to_rfc3339()
        );
        let (vault, _) = build_vault(CFG, &[("001-a.md", &task)]);
        assert!(plan(&vault, now()).unwrap().is_empty());
    }

    /// A runner that simulates a web cancel arriving mid-run by stamping the
    /// cancel marker on the task while the agent is "working".
    struct CancelingRunner {
        path: PathBuf,
    }

    impl AgentRunner for CancelingRunner {
        fn run(
            &self,
            _spec: &AgentSpec,
            _prompt: &str,
            _working_dir: &Path,
            _timeout_secs: u64,
            _log_path: Option<&Path>,
        ) -> AgentOutcome {
            let mut t = Task::parse_required(&fs::read_to_string(&self.path).unwrap()).unwrap();
            t.set(K_CANCEL, Value::String(now().to_rfc3339()));
            fs::write(&self.path, t.to_markdown()).unwrap();
            AgentOutcome {
                success: true,
                ..Default::default()
            }
        }
    }

    #[test]
    fn run_one_honors_cancel_without_burning_attempt() {
        let (vault, dir) = build_vault(CFG, &[("001-a.md", &runnable_task("001"))]);
        let runner = CancelingRunner {
            path: dir.join("tasks/001-a.md"),
        };
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert_eq!(report.ran.len(), 1);
        assert!(report.ran[0].cancelled);
        assert!(!report.ran[0].succeeded);
        let t = reload(&vault, "001");
        assert!(t.get("ai_status").is_none());
        assert!(t.get("ai_cancel").is_none());
        // The pre-incremented attempt is refunded (back to none).
        assert!(t.get("ai_attempts").is_none());
        // The run is logged with a cancelled outcome.
        let recs = read_records(&dir.join(".karamd/runs"));
        assert_eq!(recs[0].outcome, "cancelled");
        assert!(recs[0].last_error.is_none());
    }

    #[test]
    fn run_one_cancel_refunds_to_a_positive_count() {
        // Starting at 1 attempt: mark_running -> 2, cancel refunds back to 1.
        let task = "---\nid: \"001\"\ntitle: t\nstatus: pending\ntags: [ai-runnable]\nai_attempts: 1\n---\n";
        let (vault, dir) = build_vault(CFG, &[("001-a.md", task)]);
        let runner = CancelingRunner {
            path: dir.join("tasks/001-a.md"),
        };
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert!(report.ran[0].cancelled);
        assert_eq!(
            reload(&vault, "001")
                .get("ai_attempts")
                .and_then(|v| v.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn cancel_log_write_failure_is_nonfatal() {
        // Cancelled runs still log best-effort: an unwritable log dir must not
        // abort the run.
        let dir = tempdir();
        fs::write(dir.join("blocker"), "x").unwrap();
        let cfg = format!(
            "run:\n  enabled: true\n  agent: claude\n  max_attempts: 2\n  log_dir: {}\n  agents:\n    claude:\n      command: [claude]\n",
            dir.join("blocker/sub").display()
        );
        fs::write(dir.join(".taskmd.yaml"), cfg).unwrap();
        fs::write(dir.join("tasks/001-a.md"), runnable_task("001")).unwrap();
        let vault = Vault::open(&dir).unwrap();
        let runner = CancelingRunner {
            path: dir.join("tasks/001-a.md"),
        };
        let report = run_all(&vault, &runner, now(), today()).unwrap();
        assert!(report.ran[0].cancelled);
    }
}
