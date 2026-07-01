//! karamd — recurring-task generator for a taskmd markdown vault.
//!
//! taskmd has no recurrence. karamd is the layer that adds it: it reads a rules
//! file, inspects the existing task files, and materialises the next occurrence
//! of a rule only when it is due. Re-running on the same day never duplicates
//! (idempotency), because every generated task carries a `recurring:` marker
//! that the next run reads back.
//!
//! Two triggers, both defined in [`due`]:
//!   - `after_completion`: due N days after the last occurrence was *completed*.
//!   - `calendar`: due `lead_days` before a fixed annual date, once per year.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate};
use clap::{Parser, Subcommand};

pub mod due;
pub mod rule;
pub mod task;

/// Rules file used when `--config` is omitted, resolved relative to `--vault`.
pub const DEFAULT_CONFIG: &str = ".taskmd.recurring.yaml";

use rule::{Rule, Trigger};
use task::ExistingTask;

#[derive(Parser)]
#[command(name = "karamd", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate any due recurring tasks into the target vault.
    Generate {
        /// Path to the taskmd project root (the dir holding .taskmd.yaml).
        #[arg(long)]
        vault: PathBuf,
        /// Path to the recurring-rules YAML file. Defaults to
        /// `<vault>/.taskmd.recurring.yaml`.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Print what would be created without writing files.
        #[arg(long)]
        dry_run: bool,
        /// Pretend today is this date (YYYY-MM-DD) for backfill or testing;
        /// defaults to the system date.
        #[arg(long)]
        today: Option<NaiveDate>,
    },
}

/// One task karamd decided to create this run.
#[derive(Debug, Clone, PartialEq)]
pub struct Created {
    pub filename: String,
    pub marker: String,
}

/// What a `generate` run did, for logging and testing.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub created: Vec<Created>,
}

/// Does an existing task's `recurring` marker belong to this rule? For
/// after_completion the marker is exactly the key; for calendar it is
/// `key:year`, so we match the `key:` prefix.
fn marker_belongs(marker: &str, rule: &Rule) -> bool {
    match rule.trigger {
        Trigger::AfterCompletion => marker == rule.key,
        Trigger::Calendar => marker
            .strip_prefix(&rule.key)
            .and_then(|rest| rest.strip_prefix(':'))
            .is_some_and(|year| year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit())),
    }
}

/// Decide whether `rule` is due today given the tasks already in the vault.
/// Returns the `recurring` marker to stamp on the new task, or `None` to skip.
///
/// This is where each trigger's dedup contract lives:
///   - after_completion: an open task blocks; otherwise due by interval since
///     the most recent completion.
///   - calendar: due inside the lead window, unless a task for that target year
///     already exists (which is why early completion can't re-trigger).
fn decide(rule: &Rule, existing: &[ExistingTask], today: NaiveDate) -> Result<Option<String>> {
    let mine: Vec<&ExistingTask> = existing
        .iter()
        .filter(|t| {
            t.recurring
                .as_deref()
                .is_some_and(|m| marker_belongs(m, rule))
        })
        .collect();

    match rule.trigger {
        Trigger::AfterCompletion => {
            if mine.iter().any(|t| t.is_open()) {
                return Ok(None);
            }
            // All matching tasks are terminal here; use each one's conclusion
            // date (completed or cancelled, else created) so a cancelled or
            // undated occurrence still anchors the interval instead of looking
            // like "never ran".
            let last = mine.iter().filter_map(|t| t.occurrence_date()).max();
            let every = rule
                .every_days
                .context("after_completion needs every_days")?;
            Ok(due::after_completion_due(today, every, last).then(|| rule.key.clone()))
        }
        Trigger::Calendar => {
            let annual = rule.annual.as_deref().context("calendar needs annual")?;
            let lead = rule.lead_days.context("calendar needs lead_days")?;
            match due::calendar_due(today, annual, lead)? {
                Some(year) => {
                    let marker = format!("{}:{year}", rule.key);
                    let exists = mine.iter().any(|t| t.recurring.as_deref() == Some(&marker));
                    Ok((!exists).then_some(marker))
                }
                None => Ok(None),
            }
        }
    }
}

/// Read rules and existing tasks, create every due task, return a [`Report`].
/// With `dry_run` the report is identical but no files are written.
pub fn generate(vault: &Path, config: &Path, today: NaiveDate, dry_run: bool) -> Result<Report> {
    let raw = fs::read_to_string(config)
        .with_context(|| format!("reading rules file {}", config.display()))?;
    let rules = rule::load_rules(&raw)?;
    rule::validate_all(&rules)?;

    let tasks_dir = task::tasks_dir(vault);
    let existing = task::scan_dir(&tasks_dir)?;
    let mut next = task::next_id(&existing);

    let mut report = Report::default();
    for rule in &rules {
        let Some(marker) = decide(rule, &existing, today)? else {
            continue;
        };
        let id = format!("{next:03}");
        let filename = format!("{id}-{}.md", task::slugify(&rule.title));
        if !dry_run {
            fs::create_dir_all(&tasks_dir)?;
            let body = task::render_task(rule, &id, &marker, today);
            fs::write(tasks_dir.join(&filename), body)?;
        }
        report.created.push(Created { filename, marker });
        next += 1;
    }
    Ok(report)
}

/// CLI entry point. Parses `args`, resolves today's date, and runs `generate`.
pub fn run<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    match cli.command {
        Commands::Generate {
            vault,
            config,
            dry_run,
            today,
        } => {
            let today = today.unwrap_or_else(|| Local::now().date_naive());
            let config = config.unwrap_or_else(|| vault.join(DEFAULT_CONFIG));
            let report = generate(&vault, &config, today, dry_run)?;
            if report.created.is_empty() {
                println!("karamd: nothing due");
            } else {
                let verb = if dry_run { "would create" } else { "created" };
                for c in &report.created {
                    println!("karamd: {verb} {} (recurring: {})", c.filename, c.marker);
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn tempdir() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("karamd-lib-{uniq}"));
        fs::create_dir_all(&base).unwrap();
        base
    }

    /// A vault dir with a tasks/ subdir and a rules file; returns (vault, config).
    fn vault_with_rules(rules_yaml: &str) -> (PathBuf, PathBuf) {
        let vault = tempdir();
        fs::create_dir_all(vault.join("tasks")).unwrap();
        let config = vault.join("recurring.yml");
        fs::write(&config, rules_yaml).unwrap();
        (vault, config)
    }

    const AFTER: &str =
        "- key: checkin\n  title: Reach out\n  trigger: after_completion\n  every_days: 18\n";
    const CAL: &str = "- key: bday\n  title: Birthday\n  trigger: calendar\n  annual: \"07-20\"\n  lead_days: 10\n";

    #[test]
    fn marker_belongs_matches_correctly() {
        let after = &rule::load_rules(AFTER).unwrap()[0];
        let cal = &rule::load_rules(CAL).unwrap()[0];
        assert!(marker_belongs("checkin", after));
        assert!(!marker_belongs("checkin:2026", after));
        assert!(marker_belongs("bday:2026", cal));
        assert!(!marker_belongs("bday", cal));
        assert!(!marker_belongs("bday:20xx", cal));
        assert!(!marker_belongs("other:2026", cal));
    }

    #[test]
    fn generate_first_run_creates_after_completion() {
        let (vault, config) = vault_with_rules(AFTER);
        let report = generate(&vault, &config, day(2026, 7, 1), false).unwrap();
        assert_eq!(report.created.len(), 1);
        assert_eq!(report.created[0].marker, "checkin");
        assert!(
            vault
                .join("tasks")
                .join(&report.created[0].filename)
                .exists()
        );
    }

    #[test]
    fn generate_is_idempotent_same_day() {
        let (vault, config) = vault_with_rules(AFTER);
        let first = generate(&vault, &config, day(2026, 7, 1), false).unwrap();
        assert_eq!(first.created.len(), 1);
        // Second run: the open task blocks re-creation.
        let second = generate(&vault, &config, day(2026, 7, 1), false).unwrap();
        assert_eq!(second, Report::default());
        let files: Vec<_> = fs::read_dir(vault.join("tasks")).unwrap().collect();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn crlf_open_task_blocks_recreation() {
        // The idempotency contract must hold for a CRLF-encoded existing task:
        // its recurring marker must be read so the open task blocks a duplicate.
        let (vault, config) = vault_with_rules(AFTER);
        let open =
            "---\r\nid: \"001\"\r\nstatus: pending\r\nrecurring: \"checkin\"\r\n---\r\n\r\n# x\r\n";
        fs::write(vault.join("tasks/001-reach-out.md"), open).unwrap();
        assert_eq!(
            generate(&vault, &config, day(2026, 7, 1), false).unwrap(),
            Report::default()
        );
    }

    #[test]
    fn after_completion_recreates_only_once_interval_elapsed() {
        let (vault, config) = vault_with_rules(AFTER);
        // Existing completed task 18+ days ago, no open task.
        let done = "---\nid: \"001\"\nstatus: completed\nrecurring: \"checkin\"\ncompleted_at: 2026-06-01\n---\n";
        fs::write(vault.join("tasks/001-reach-out.md"), done).unwrap();

        // Before interval elapses: nothing.
        assert_eq!(
            generate(&vault, &config, day(2026, 6, 10), false).unwrap(),
            Report::default()
        );
        // After interval: exactly one, numbered past the existing id.
        let r = generate(&vault, &config, day(2026, 6, 20), false).unwrap();
        assert_eq!(r.created.len(), 1);
        assert!(r.created[0].filename.starts_with("002-"));
    }

    #[test]
    fn after_completion_cancelled_reschedules_after_interval_not_immediately() {
        let (vault, config) = vault_with_rules(AFTER);
        // A cancelled occurrence: no completed_at, only cancelled_at.
        let cancelled = "---\nid: \"001\"\nstatus: cancelled\nrecurring: \"checkin\"\ncancelled_at: 2026-06-01\ncreated_at: 2026-05-14\n---\n";
        fs::write(vault.join("tasks/001-reach-out.md"), cancelled).unwrap();

        // Right after cancelling: must NOT re-fire immediately.
        assert_eq!(
            generate(&vault, &config, day(2026, 6, 2), false).unwrap(),
            Report::default()
        );
        // Before the interval since cancellation: still nothing.
        assert_eq!(
            generate(&vault, &config, day(2026, 6, 10), false).unwrap(),
            Report::default()
        );
        // every_days after the cancellation: the series continues.
        let r = generate(&vault, &config, day(2026, 6, 19), false).unwrap();
        assert_eq!(r.created.len(), 1);
    }

    #[test]
    fn after_completion_undated_completion_uses_created_at_not_refire_forever() {
        let (vault, config) = vault_with_rules(AFTER);
        // Completed but missing completed_at: must fall back to created_at, not
        // be treated as "never ran" (which would re-fire on every run).
        let undated = "---\nid: \"001\"\nstatus: completed\nrecurring: \"checkin\"\ncreated_at: 2026-06-01\n---\n";
        fs::write(vault.join("tasks/001-reach-out.md"), undated).unwrap();

        assert_eq!(
            generate(&vault, &config, day(2026, 6, 5), false).unwrap(),
            Report::default()
        );
        let r = generate(&vault, &config, day(2026, 6, 20), false).unwrap();
        assert_eq!(r.created.len(), 1);
    }

    #[test]
    fn calendar_creates_once_per_year_and_survives_early_completion() {
        let (vault, config) = vault_with_rules(CAL);
        // Inside the 10-day window.
        let r = generate(&vault, &config, day(2026, 7, 12), false).unwrap();
        assert_eq!(r.created.len(), 1);
        assert_eq!(r.created[0].marker, "bday:2026");

        // Simulate completing it early (still same window).
        let path = vault.join("tasks").join(&r.created[0].filename);
        let content = fs::read_to_string(&path)
            .unwrap()
            .replace("status: pending", "status: completed");
        fs::write(&path, content).unwrap();

        // Re-run inside the window: the year marker blocks re-creation.
        assert_eq!(
            generate(&vault, &config, day(2026, 7, 15), false).unwrap(),
            Report::default()
        );
    }

    #[test]
    fn calendar_next_year_window_creates_again() {
        let (vault, config) = vault_with_rules(CAL);
        // Last year's task already present and completed.
        let old = "---\nid: \"001\"\nstatus: completed\nrecurring: \"bday:2026\"\ncompleted_at: 2026-07-18\n---\n";
        fs::write(vault.join("tasks/001-birthday.md"), old).unwrap();
        // 2027 window opens.
        let r = generate(&vault, &config, day(2027, 7, 12), false).unwrap();
        assert_eq!(r.created.len(), 1);
        assert_eq!(r.created[0].marker, "bday:2027");
    }

    #[test]
    fn calendar_outside_window_creates_nothing() {
        let (vault, config) = vault_with_rules(CAL);
        assert_eq!(
            generate(&vault, &config, day(2026, 1, 1), false).unwrap(),
            Report::default()
        );
    }

    #[test]
    fn dry_run_writes_nothing_but_reports() {
        let (vault, config) = vault_with_rules(AFTER);
        let r = generate(&vault, &config, day(2026, 7, 1), true).unwrap();
        assert_eq!(r.created.len(), 1);
        assert!(fs::read_dir(vault.join("tasks")).unwrap().next().is_none());
    }

    #[test]
    fn generate_creates_tasks_dir_if_absent() {
        let vault = tempdir();
        let config = vault.join("recurring.yml");
        fs::write(&config, AFTER).unwrap();
        // No tasks/ dir yet.
        let r = generate(&vault, &config, day(2026, 7, 1), false).unwrap();
        assert_eq!(r.created.len(), 1);
        assert!(vault.join("tasks").is_dir());
    }

    #[test]
    fn generate_missing_config_errors() {
        let vault = tempdir();
        let err = generate(&vault, &vault.join("nope.yml"), day(2026, 7, 1), false).unwrap_err();
        assert!(err.to_string().contains("reading rules file"));
    }

    #[test]
    fn generate_invalid_rule_errors() {
        let (vault, config) =
            vault_with_rules("- key: k\n  title: t\n  trigger: after_completion\n");
        assert!(generate(&vault, &config, day(2026, 7, 1), false).is_err());
    }

    #[test]
    fn generate_malformed_rules_yaml_errors() {
        let (vault, config) = vault_with_rules("key: : : not a list");
        assert!(generate(&vault, &config, day(2026, 7, 1), false).is_err());
    }

    #[test]
    fn generate_scan_error_when_tasks_dir_is_a_file() {
        let vault = tempdir();
        fs::write(vault.join("tasks"), "i am a file, not a dir").unwrap();
        let config = vault.join("recurring.yml");
        fs::write(&config, AFTER).unwrap();
        assert!(generate(&vault, &config, day(2026, 7, 1), false).is_err());
    }

    #[test]
    fn run_end_to_end() {
        let (vault, config) = vault_with_rules(AFTER);
        run([
            "karamd".into(),
            "generate".into(),
            "--vault".into(),
            vault.clone().into_os_string(),
            "--config".into(),
            config.into_os_string(),
        ])
        .unwrap();
        assert_eq!(fs::read_dir(vault.join("tasks")).unwrap().count(), 1);
    }

    #[test]
    fn run_defaults_config_to_taskmd_recurring_yaml() {
        // Omitting --config resolves to <vault>/.taskmd.recurring.yaml.
        let vault = tempdir();
        fs::create_dir_all(vault.join("tasks")).unwrap();
        fs::write(vault.join(DEFAULT_CONFIG), AFTER).unwrap();
        run([
            "karamd".into(),
            "generate".into(),
            "--vault".into(),
            vault.clone().into_os_string(),
        ])
        .unwrap();
        assert_eq!(fs::read_dir(vault.join("tasks")).unwrap().count(), 1);
    }

    #[test]
    fn run_today_override_drives_the_date() {
        // A far-future --today the real clock can never match: the calendar rule
        // fires only because the override put us inside its 2099 window.
        let (vault, config) = vault_with_rules(CAL);
        run([
            "karamd".into(),
            "generate".into(),
            "--vault".into(),
            vault.clone().into_os_string(),
            "--config".into(),
            config.into_os_string(),
            "--today".into(),
            "2099-07-15".into(),
        ])
        .unwrap();
        let files: Vec<_> = fs::read_dir(vault.join("tasks"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        assert!(
            fs::read_to_string(&files[0])
                .unwrap()
                .contains("recurring: \"bday:2099\"")
        );
    }

    #[test]
    fn cli_rejects_invalid_today() {
        let parsed = Cli::try_parse_from([
            "karamd",
            "generate",
            "--vault",
            "v",
            "--config",
            "c",
            "--today",
            "not-a-date",
        ]);
        assert!(parsed.is_err());
    }

    fn bare_rule(trigger: Trigger) -> Rule {
        Rule {
            key: "k".into(),
            title: "t".into(),
            trigger,
            every_days: None,
            annual: None,
            lead_days: None,
            phase: None,
            priority: None,
            tags: vec![],
        }
    }

    #[test]
    fn decide_after_completion_missing_every_days_errors() {
        let r = bare_rule(Trigger::AfterCompletion);
        assert!(decide(&r, &[], day(2026, 7, 1)).is_err());
    }

    #[test]
    fn decide_calendar_missing_annual_errors() {
        let mut r = bare_rule(Trigger::Calendar);
        r.lead_days = Some(10);
        assert!(decide(&r, &[], day(2026, 7, 1)).is_err());
    }

    #[test]
    fn decide_calendar_missing_lead_days_errors() {
        let mut r = bare_rule(Trigger::Calendar);
        r.annual = Some("07-20".into());
        assert!(decide(&r, &[], day(2026, 7, 1)).is_err());
    }

    #[test]
    fn decide_calendar_bad_annual_errors() {
        let mut r = bare_rule(Trigger::Calendar);
        r.annual = Some("not-a-date".into());
        r.lead_days = Some(10);
        assert!(decide(&r, &[], day(2026, 7, 1)).is_err());
    }

    #[test]
    fn generate_rejects_malformed_annual() {
        // A present-but-malformed annual is rejected by validate_all up front.
        let yaml =
            "- key: k\n  title: t\n  trigger: calendar\n  annual: \"99-99\"\n  lead_days: 5\n";
        let (vault, config) = vault_with_rules(yaml);
        assert!(generate(&vault, &config, day(2026, 7, 1), false).is_err());
    }

    #[test]
    fn generate_rejects_duplicate_keys() {
        let yaml = "- key: k\n  title: a\n  trigger: after_completion\n  every_days: 3\n- key: k\n  title: b\n  trigger: after_completion\n  every_days: 5\n";
        let (vault, config) = vault_with_rules(yaml);
        assert!(generate(&vault, &config, day(2026, 7, 1), false).is_err());
    }

    #[test]
    fn generate_create_dir_all_error() {
        // tasks dir would be <vault>/blocker/tasks, but `blocker` is a file.
        let vault = tempdir();
        fs::write(vault.join(".taskmd.yaml"), "dir: ./blocker/tasks\n").unwrap();
        fs::write(vault.join("blocker"), "i am a file").unwrap();
        let config = vault.join("recurring.yml");
        fs::write(&config, AFTER).unwrap();
        assert!(generate(&vault, &config, day(2026, 7, 1), false).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn generate_write_error_on_readonly_dir() {
        use std::os::unix::fs::PermissionsExt;
        let (vault, config) = vault_with_rules(AFTER);
        let tasks = vault.join("tasks");
        fs::set_permissions(&tasks, fs::Permissions::from_mode(0o555)).unwrap();
        let result = generate(&vault, &config, day(2026, 7, 1), false);
        // Restore perms so the temp dir can be cleaned up.
        fs::set_permissions(&tasks, fs::Permissions::from_mode(0o755)).ok();
        assert!(result.is_err());
    }

    #[test]
    fn run_propagates_error() {
        let vault = tempdir();
        let err = run([
            "karamd".into(),
            "generate".into(),
            "--vault".into(),
            vault.clone().into_os_string(),
            "--config".into(),
            vault.join("missing.yml").into_os_string(),
        ]);
        assert!(err.is_err());
    }

    #[test]
    fn run_dry_run_reports_would_create() {
        // dry-run that IS due -> exercises the "would create" log branch.
        let (vault, config) = vault_with_rules(AFTER);
        run([
            "karamd".into(),
            "generate".into(),
            "--vault".into(),
            vault.clone().into_os_string(),
            "--config".into(),
            config.into_os_string(),
            "--dry-run".into(),
        ])
        .unwrap();
        assert!(fs::read_dir(vault.join("tasks")).unwrap().next().is_none());
    }

    #[test]
    fn run_dry_run_reports_nothing_when_not_due() {
        let (vault, config) = vault_with_rules(CAL);
        // Outside window -> "nothing due" branch, dry-run path.
        run([
            "karamd".into(),
            "generate".into(),
            "--vault".into(),
            vault.clone().into_os_string(),
            "--config".into(),
            config.into_os_string(),
            "--dry-run".into(),
        ])
        .unwrap();
        assert!(fs::read_dir(vault.join("tasks")).unwrap().next().is_none());
    }
}
