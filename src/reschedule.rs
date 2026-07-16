//! `karamd reschedule`: move open tasks between phases by their `due` date,
//! driven by a custom, ordered rule list in `.taskmd.reschedule.yaml`.
//!
//! Like [`crate::generate`] this is idempotent and safe to run unattended: it
//! reads the current task state each run and only writes a task whose phase
//! actually needs to change. Unlike the generator (which only ever *adds*
//! files) it only ever changes the `phase` field.
//!
//! The pure core — [`Window::contains`], [`RescheduleRule`] matching,
//! [`decide`], [`plan`], and validation — takes `today` explicitly and never
//! touches the clock, so every branch is unit-testable. [`run_reschedule`] is
//! the thin I/O shell over a [`Vault`].

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::taskmd::Vault;
use crate::taskmd::model::Task;

/// Default reschedule-rules file, resolved relative to `--vault` when
/// `--config` is omitted. Sits beside `.taskmd.yaml` and
/// `.taskmd.recurring.yaml`.
pub const DEFAULT_RESCHEDULE_CONFIG: &str = ".taskmd.reschedule.yaml";

/// A named due-date window, matched against a task's `due` relative to `today`.
/// The serde spelling is snake_case (`this_week`, `next_month`, ...). Overlaps
/// (today is also "this week") are resolved by rule order — first match wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Window {
    /// Due strictly before today.
    Overdue,
    /// Due exactly today.
    Today,
    /// Due in the same ISO week as today.
    ThisWeek,
    /// Due in the ISO week after today's.
    NextWeek,
    /// Due in the same calendar month as today.
    ThisMonth,
    /// Due in the calendar month after today's (December rolls into January).
    NextMonth,
}

impl Window {
    /// Does `due` fall in this window, given `today`?
    pub fn contains(self, due: NaiveDate, today: NaiveDate) -> bool {
        match self {
            Window::Overdue => due < today,
            Window::Today => due == today,
            Window::ThisWeek => due.iso_week() == today.iso_week(),
            Window::NextWeek => due.iso_week() == (today + Duration::days(7)).iso_week(),
            Window::ThisMonth => due.year() == today.year() && due.month() == today.month(),
            Window::NextMonth => {
                let (y, m) = next_month(today.year(), today.month());
                due.year() == y && due.month() == m
            }
        }
    }
}

/// The calendar month after (`year`, `month`), rolling December into January.
fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

/// One reschedule rule: a window (named or numeric) plus the target phase a
/// matching task is moved to. Exactly one matcher must be set — enforced by
/// [`RescheduleRule::validate`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RescheduleRule {
    /// Target phase id (must be a configured, non-null phase).
    pub phase: String,
    /// Named window (mutually exclusive with `min_days`/`max_days`).
    #[serde(default)]
    pub due: Option<Window>,
    /// Inclusive lower bound on the signed offset `(due - today).days`.
    #[serde(default)]
    pub min_days: Option<i64>,
    /// Inclusive upper bound on the signed offset `(due - today).days`.
    /// `max_days: 0` therefore also catches overdue tasks.
    #[serde(default)]
    pub max_days: Option<i64>,
}

impl RescheduleRule {
    /// Does a task with this `due` date match, given `today`? A named window
    /// takes precedence; otherwise the numeric range applies (an open-ended
    /// side matches everything on that side). Only called on validated rules,
    /// so exactly one matcher is set.
    fn matches(&self, due: NaiveDate, today: NaiveDate) -> bool {
        if let Some(window) = self.due {
            return window.contains(due, today);
        }
        let offset = (due - today).num_days();
        self.min_days.is_none_or(|lo| offset >= lo) && self.max_days.is_none_or(|hi| offset <= hi)
    }

    /// Reject a rule that has no matcher, both matcher kinds, an inverted
    /// numeric range, an empty phase, or a phase that is not configured.
    fn validate(&self, idx: usize, valid_phases: &HashSet<&str>) -> Result<()> {
        let n = idx + 1;
        let has_named = self.due.is_some();
        let has_range = self.min_days.is_some() || self.max_days.is_some();
        if has_named && has_range {
            bail!("reschedule rule #{n}: cannot combine a `due` window with `min_days`/`max_days`");
        }
        if !has_named && !has_range {
            bail!("reschedule rule #{n}: needs a `due` window or a `min_days`/`max_days` range");
        }
        if let (Some(lo), Some(hi)) = (self.min_days, self.max_days)
            && lo > hi
        {
            bail!("reschedule rule #{n}: `min_days` ({lo}) must be <= `max_days` ({hi})");
        }
        if self.phase.trim().is_empty() {
            bail!("reschedule rule #{n}: `phase` must not be empty");
        }
        if !valid_phases.contains(self.phase.as_str()) {
            bail!(
                "reschedule rule #{n}: `phase` `{}` is not a configured phase",
                self.phase
            );
        }
        Ok(())
    }
}

/// The reschedule-rules file: an ordered rule list plus an optional `enabled`
/// switch (default true, so the file's presence is enough; set `enabled: false`
/// to pause without deleting it).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RescheduleConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<RescheduleRule>,
}

fn default_enabled() -> bool {
    true
}

impl RescheduleConfig {
    /// Validate every rule against the set of configured phase keys.
    pub fn validate(&self, valid_phases: &HashSet<&str>) -> Result<()> {
        for (idx, rule) in self.rules.iter().enumerate() {
            rule.validate(idx, valid_phases)?;
        }
        Ok(())
    }
}

/// Parse a reschedule-rules file's contents.
pub fn load_reschedule_config(raw: &str) -> Result<RescheduleConfig> {
    Ok(serde_norway::from_str(raw)?)
}

/// The target phase for a task with this `due` date: the first matching rule's
/// phase, or `None` when no rule matches.
pub fn decide(due: NaiveDate, today: NaiveDate, rules: &[RescheduleRule]) -> Option<&str> {
    rules
        .iter()
        .find(|r| r.matches(due, today))
        .map(|r| r.phase.as_str())
}

/// One phase move `plan` decided on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhaseMove {
    pub id: String,
    /// The task's phase before the move (`None` = unphased).
    pub from: Option<String>,
    pub to: String,
}

/// Compute the moves for a scanned task set: every open (non-terminal) task
/// with a `due` date whose first matching rule names a phase different from its
/// current one. Terminal tasks, undated tasks, tasks matching no rule, and
/// tasks already in the target phase are left out.
pub fn plan(tasks: &[Task], rules: &[RescheduleRule], today: NaiveDate) -> Vec<PhaseMove> {
    let mut moves = Vec::new();
    for task in tasks {
        if task.effective_status().is_terminal() {
            continue;
        }
        let Some(due) = task.due() else { continue };
        let Some(target) = decide(due, today, rules) else {
            continue;
        };
        let from = task.phase();
        if from.as_deref() == Some(target) {
            continue;
        }
        moves.push(PhaseMove {
            id: task.id(),
            from,
            to: target.to_string(),
        });
    }
    moves
}

/// Why a run did not evaluate rules, or that it did.
#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Rules were evaluated (see `moved`).
    #[default]
    Ran,
    /// No config file at the resolved path.
    NoConfig,
    /// Config present but `enabled: false`.
    Disabled,
}

/// What a `reschedule` run did, for printing and testing.
#[derive(Debug, Default, PartialEq, Serialize)]
pub struct RescheduleReport {
    pub state: State,
    pub moved: Vec<PhaseMove>,
    /// Open tasks with a due date that were considered.
    pub considered: usize,
}

/// Read the config, evaluate the rules against the vault, and (unless
/// `dry_run`) apply each move. A missing config file or `enabled: false` is a
/// clean no-op, reported via [`RescheduleReport::state`].
pub fn run_reschedule(
    vault: &Vault,
    config_path: &Path,
    today: NaiveDate,
    dry_run: bool,
) -> Result<RescheduleReport> {
    if !config_path.exists() {
        return Ok(RescheduleReport {
            state: State::NoConfig,
            ..Default::default()
        });
    }
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading reschedule config {}", config_path.display()))?;
    let cfg = load_reschedule_config(&raw)?;
    if !cfg.enabled {
        return Ok(RescheduleReport {
            state: State::Disabled,
            ..Default::default()
        });
    }
    let valid_phases: HashSet<&str> = vault.config.phases.iter().map(|p| p.key()).collect();
    cfg.validate(&valid_phases)?;

    let scan = vault.scan()?;
    let considered = scan
        .tasks
        .iter()
        .filter(|t| !t.effective_status().is_terminal() && t.due().is_some())
        .count();
    let moved = plan(&scan.tasks, &cfg.rules, today);
    if !dry_run {
        for m in &moved {
            let to = m.to.clone();
            let mut set_phase = |t: &mut Task| {
                t.set_phase(Some(&to));
                Ok(())
            };
            vault.update(&m.id, &mut set_phase)?;
        }
    }
    Ok(RescheduleReport {
        state: State::Ran,
        moved,
        considered,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn rule(
        phase: &str,
        due: Option<Window>,
        min: Option<i64>,
        max: Option<i64>,
    ) -> RescheduleRule {
        RescheduleRule {
            phase: phase.into(),
            due,
            min_days: min,
            max_days: max,
        }
    }

    fn task(frontmatter: &str) -> Task {
        Task::parse_required(&format!("---\n{frontmatter}---\n\n# t\n")).unwrap()
    }

    // ---- Window::contains ----

    #[test]
    fn window_overdue_and_today() {
        let today = d(2026, 7, 15);
        assert!(Window::Overdue.contains(d(2026, 7, 14), today));
        assert!(!Window::Overdue.contains(today, today));
        assert!(Window::Today.contains(today, today));
        assert!(!Window::Today.contains(d(2026, 7, 16), today));
    }

    #[test]
    fn window_this_and_next_week() {
        // 2026-07-15 is a Wednesday (ISO week 29). Sunday 07-19 is still this
        // week; Monday 07-20 is next week.
        let today = d(2026, 7, 15);
        assert!(Window::ThisWeek.contains(d(2026, 7, 19), today));
        assert!(!Window::ThisWeek.contains(d(2026, 7, 20), today));
        assert!(Window::NextWeek.contains(d(2026, 7, 20), today));
        assert!(!Window::NextWeek.contains(d(2026, 7, 19), today));
        assert!(!Window::NextWeek.contains(d(2026, 7, 27), today));
    }

    #[test]
    fn window_this_and_next_month() {
        let today = d(2026, 7, 15);
        assert!(Window::ThisMonth.contains(d(2026, 7, 1), today));
        assert!(!Window::ThisMonth.contains(d(2026, 8, 1), today));
        assert!(!Window::ThisMonth.contains(d(2025, 7, 1), today));
        assert!(Window::NextMonth.contains(d(2026, 8, 31), today));
        assert!(!Window::NextMonth.contains(d(2026, 9, 1), today));
    }

    #[test]
    fn window_next_month_rolls_over_year() {
        let today = d(2026, 12, 15);
        assert!(Window::NextMonth.contains(d(2027, 1, 5), today));
        assert!(!Window::NextMonth.contains(d(2026, 12, 31), today));
    }

    // ---- matching / decide ----

    #[test]
    fn matches_named_window() {
        let r = rule("now", Some(Window::Today), None, None);
        assert!(r.matches(d(2026, 7, 15), d(2026, 7, 15)));
        assert!(!r.matches(d(2026, 7, 16), d(2026, 7, 15)));
    }

    #[test]
    fn matches_numeric_ranges() {
        let today = d(2026, 7, 15);
        // Both bounds: 1..=7 days out.
        let both = rule("next", None, Some(1), Some(7));
        assert!(both.matches(d(2026, 7, 20), today));
        assert!(!both.matches(today, today));
        assert!(!both.matches(d(2026, 7, 23), today));
        // max only (includes overdue).
        let max = rule("now", None, None, Some(0));
        assert!(max.matches(d(2026, 7, 10), today));
        assert!(max.matches(today, today));
        assert!(!max.matches(d(2026, 7, 16), today));
        // min only (unbounded above).
        let min = rule("soon", None, Some(8), None);
        assert!(min.matches(d(2026, 7, 23), today));
        assert!(!min.matches(d(2026, 7, 20), today));
    }

    #[test]
    fn decide_first_match_wins() {
        let today = d(2026, 7, 15);
        let rules = vec![
            rule("now", Some(Window::Today), None, None),
            rule("next", Some(Window::ThisWeek), None, None),
            rule("soon", Some(Window::NextWeek), None, None),
        ];
        assert_eq!(decide(today, today, &rules), Some("now"));
        assert_eq!(decide(d(2026, 7, 17), today, &rules), Some("next"));
        assert_eq!(decide(d(2026, 7, 21), today, &rules), Some("soon"));
        // A far-future due matches nothing.
        assert_eq!(decide(d(2026, 9, 1), today, &rules), None);
    }

    // ---- rule validation ----

    fn phases() -> HashSet<&'static str> {
        ["now", "next", "soon"].into_iter().collect()
    }

    #[test]
    fn validate_accepts_named_and_numeric() {
        rule("now", Some(Window::Today), None, None)
            .validate(0, &phases())
            .unwrap();
        rule("next", None, Some(1), Some(7))
            .validate(0, &phases())
            .unwrap();
    }

    #[test]
    fn validate_rejects_both_matchers() {
        let err = rule("now", Some(Window::Today), Some(0), None)
            .validate(2, &phases())
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("rule #3"), "{msg}");
        assert!(msg.contains("cannot combine"), "{msg}");
    }

    #[test]
    fn validate_rejects_no_matcher() {
        let err = rule("now", None, None, None)
            .validate(0, &phases())
            .unwrap_err();
        assert!(err.to_string().contains("needs a `due` window"));
    }

    #[test]
    fn validate_rejects_inverted_range() {
        let err = rule("now", None, Some(5), Some(1))
            .validate(0, &phases())
            .unwrap_err();
        assert!(err.to_string().contains("must be <= `max_days`"));
    }

    #[test]
    fn validate_rejects_empty_phase() {
        let err = rule("  ", Some(Window::Today), None, None)
            .validate(0, &phases())
            .unwrap_err();
        assert!(err.to_string().contains("`phase` must not be empty"));
    }

    #[test]
    fn validate_rejects_unknown_phase() {
        let err = rule("backlog", Some(Window::Today), None, None)
            .validate(0, &phases())
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not a configured phase"), "{msg}");
        assert!(msg.contains("backlog"), "{msg}");
    }

    #[test]
    fn config_validate_walks_all_rules() {
        let cfg = RescheduleConfig {
            enabled: true,
            rules: vec![
                rule("now", Some(Window::Today), None, None),
                rule("nope", Some(Window::ThisWeek), None, None),
            ],
        };
        assert!(
            cfg.validate(&phases())
                .unwrap_err()
                .to_string()
                .contains("rule #2")
        );
    }

    // ---- config loading ----

    #[test]
    fn load_parses_example_shape() {
        let raw = "enabled: true\nrules:\n  - { due: today, phase: now }\n  - { due: this_week, phase: next }\n  - { min_days: 8, max_days: 14, phase: soon }\n";
        let cfg = load_reschedule_config(raw).unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.rules.len(), 3);
        assert_eq!(cfg.rules[0].due, Some(Window::Today));
        assert_eq!(cfg.rules[0].phase, "now");
        assert_eq!(cfg.rules[2].min_days, Some(8));
        assert_eq!(cfg.rules[2].max_days, Some(14));
    }

    #[test]
    fn load_defaults_enabled_true_and_empty_rules() {
        let cfg = load_reschedule_config("rules: []\n").unwrap();
        assert!(cfg.enabled);
        assert!(cfg.rules.is_empty());
    }

    #[test]
    fn load_honours_enabled_false() {
        let cfg = load_reschedule_config("enabled: false\nrules: []\n").unwrap();
        assert!(!cfg.enabled);
    }

    #[test]
    fn load_rejects_unknown_window() {
        let err = load_reschedule_config("rules:\n  - { due: someday, phase: now }\n").unwrap_err();
        assert!(err.to_string().contains("unknown variant"), "{err}");
    }

    // ---- plan over tasks ----

    #[test]
    fn plan_moves_open_dated_tasks_and_skips_the_rest() {
        let today = d(2026, 7, 15);
        let rules = vec![
            rule("now", Some(Window::Today), None, None),
            rule("soon", Some(Window::NextWeek), None, None),
        ];
        let tasks = vec![
            // due today, currently later -> move to now.
            task("id: \"001\"\ntitle: a\nstatus: pending\nphase: later\ndue: 2026-07-15\n"),
            // due today, unphased -> move to now (from None).
            task("id: \"002\"\ntitle: b\nstatus: pending\ndue: 2026-07-15\n"),
            // due today, already in now -> skip (idempotent).
            task("id: \"003\"\ntitle: c\nstatus: pending\nphase: now\ndue: 2026-07-15\n"),
            // no due date -> skip.
            task("id: \"004\"\ntitle: d\nstatus: pending\nphase: later\n"),
            // due far out, matches no rule -> skip.
            task("id: \"005\"\ntitle: e\nstatus: pending\ndue: 2026-09-01\n"),
            // terminal -> skip even though due today.
            task("id: \"006\"\ntitle: f\nstatus: completed\ndue: 2026-07-15\n"),
        ];
        let moves = plan(&tasks, &rules, today);
        assert_eq!(
            moves,
            vec![
                PhaseMove {
                    id: "001".into(),
                    from: Some("later".into()),
                    to: "now".into()
                },
                PhaseMove {
                    id: "002".into(),
                    from: None,
                    to: "now".into()
                },
            ]
        );
    }

    #[test]
    fn plan_moves_both_directions() {
        // Authoritative: a task in `now` whose due is next week is relaxed to
        // `soon`, not just pulled forward.
        let today = d(2026, 7, 15);
        let rules = vec![rule("soon", Some(Window::NextWeek), None, None)];
        let tasks = vec![task(
            "id: \"001\"\ntitle: a\nstatus: pending\nphase: now\ndue: 2026-07-21\n",
        )];
        let moves = plan(&tasks, &rules, today);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].to, "soon");
        assert_eq!(moves[0].from.as_deref(), Some("now"));
    }

    // ---- run_reschedule (I/O over a temp vault) ----

    fn tempdir() -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("karamd-resched-{uniq}"));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    /// A vault with phases now/next/soon and the given tasks (each a `(name,
    /// frontmatter)` pair). Returns the vault root.
    fn vault_with(tasks: &[(&str, &str)]) -> std::path::PathBuf {
        let root = tempdir();
        std::fs::write(
            root.join(".taskmd.yaml"),
            "dir: tasks\nphases:\n  - { id: now, name: Now }\n  - { id: next, name: Next }\n  - { id: soon, name: Soon }\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        for (name, fm) in tasks {
            std::fs::write(
                root.join("tasks").join(name),
                format!("---\n{fm}---\n\n# t\n"),
            )
            .unwrap();
        }
        root
    }

    const EXAMPLE_RULES: &str = "rules:\n  - { due: today, phase: now }\n  - { due: this_week, phase: next }\n  - { due: next_week, phase: soon }\n";

    #[test]
    fn run_applies_moves_and_is_idempotent() {
        let root = vault_with(&[(
            "001-a.md",
            "id: \"001\"\ntitle: a\nstatus: pending\nphase: later\ndue: 2026-07-15\n",
        )]);
        let cfg = root.join(".taskmd.reschedule.yaml");
        std::fs::write(&cfg, EXAMPLE_RULES).unwrap();
        let vault = Vault::open(&root).unwrap();

        let report = run_reschedule(&vault, &cfg, d(2026, 7, 15), false).unwrap();
        assert_eq!(report.state, State::Ran);
        assert_eq!(report.considered, 1);
        assert_eq!(report.moved.len(), 1);
        assert_eq!(report.moved[0].to, "now");
        // The file now carries phase: now.
        let body = std::fs::read_to_string(root.join("tasks/001-a.md")).unwrap();
        assert!(body.contains("phase: now"), "{body}");

        // Re-running is a no-op: the task is already in now.
        let again = run_reschedule(&vault, &cfg, d(2026, 7, 15), false).unwrap();
        assert!(again.moved.is_empty());
    }

    #[test]
    fn run_dry_run_reports_without_writing() {
        let root = vault_with(&[(
            "001-a.md",
            "id: \"001\"\ntitle: a\nstatus: pending\nphase: later\ndue: 2026-07-15\n",
        )]);
        let cfg = root.join(".taskmd.reschedule.yaml");
        std::fs::write(&cfg, EXAMPLE_RULES).unwrap();
        let vault = Vault::open(&root).unwrap();

        let report = run_reschedule(&vault, &cfg, d(2026, 7, 15), true).unwrap();
        assert_eq!(report.moved.len(), 1);
        // Unchanged on disk.
        let body = std::fs::read_to_string(root.join("tasks/001-a.md")).unwrap();
        assert!(body.contains("phase: later"), "{body}");
    }

    #[test]
    fn run_missing_config_is_no_op() {
        let root = vault_with(&[]);
        let vault = Vault::open(&root).unwrap();
        let report =
            run_reschedule(&vault, &root.join("absent.yaml"), d(2026, 7, 15), false).unwrap();
        assert_eq!(report.state, State::NoConfig);
        assert!(report.moved.is_empty());
    }

    #[test]
    fn run_disabled_config_is_no_op() {
        let root = vault_with(&[(
            "001-a.md",
            "id: \"001\"\ntitle: a\nstatus: pending\ndue: 2026-07-15\n",
        )]);
        let cfg = root.join(".taskmd.reschedule.yaml");
        std::fs::write(&cfg, format!("enabled: false\n{EXAMPLE_RULES}")).unwrap();
        let vault = Vault::open(&root).unwrap();
        let report = run_reschedule(&vault, &cfg, d(2026, 7, 15), false).unwrap();
        assert_eq!(report.state, State::Disabled);
    }

    #[test]
    fn run_propagates_validation_error() {
        let root = vault_with(&[]);
        let cfg = root.join(".taskmd.reschedule.yaml");
        std::fs::write(&cfg, "rules:\n  - { due: today, phase: nope }\n").unwrap();
        let vault = Vault::open(&root).unwrap();
        let err = run_reschedule(&vault, &cfg, d(2026, 7, 15), false).unwrap_err();
        assert!(err.to_string().contains("not a configured phase"));
    }

    #[test]
    fn report_serializes_every_state_and_unphased_move() {
        for (state, expected) in [
            (State::Ran, "ran"),
            (State::NoConfig, "no_config"),
            (State::Disabled, "disabled"),
        ] {
            let report = RescheduleReport {
                state,
                moved: vec![PhaseMove {
                    id: "001".into(),
                    from: None,
                    to: "now".into(),
                }],
                considered: 1,
            };
            let json = crate::output::to_json(&report).unwrap();
            assert!(json.contains(expected), "{json}");
            assert!(json.contains("\"from\": null"), "{json}");
        }
    }

    #[test]
    fn run_surfaces_a_read_error() {
        // The config path exists but is a directory, so the read fails after the
        // existence check.
        let root = vault_with(&[]);
        let cfg = root.join(".taskmd.reschedule.yaml");
        std::fs::create_dir_all(&cfg).unwrap();
        let vault = Vault::open(&root).unwrap();
        let err = run_reschedule(&vault, &cfg, d(2026, 7, 15), false).unwrap_err();
        assert!(err.to_string().contains("reading reschedule config"));
    }
}
