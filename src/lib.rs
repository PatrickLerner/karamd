//! karamd — recurring-task generator for a taskmd markdown vault.
//!
//! taskmd has no recurrence. karamd is the layer that adds it: it reads a rules
//! file, inspects the existing task files, and materialises the next occurrence
//! of a rule only when it is due. Re-running on the same day never duplicates
//! (idempotency), because every generated task carries a `recurring:` marker
//! that the next run reads back.
//!
//! Three triggers, all defined in [`due`]:
//!   - `after_completion`: due N days after the last occurrence was *completed*.
//!   - `calendar`: due `lead_days` before a fixed annual date, once per year.
//!   - `monthly`: due `lead_days` before a fixed day of the month, once per month.

use std::ffi::OsString;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate};
use clap::{Parser, Subcommand};

pub mod analyze;
pub mod due;
pub mod next;
pub mod output;
pub mod query;
pub mod rule;
pub mod task;
pub mod taskmd;
pub mod terminal;
pub mod validate;
pub mod verbs;
pub mod web;
pub mod web_terminal;

/// Rules file used when `--config` is omitted, resolved relative to `--vault`.
pub const DEFAULT_CONFIG: &str = ".taskmd.recurring.yaml";

use output::Format;
use rule::{Rule, Trigger};
use task::ExistingTask;
use taskmd::Status;

#[derive(Parser)]
#[command(name = "karamd", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Flags shared by every command that reads or writes a vault interactively.
/// (`generate` keeps its own required `--vault`: the unattended cron run must
/// never silently target the current directory.)
#[derive(clap::Args)]
struct VaultArg {
    /// Path to the taskmd project root (the dir holding .taskmd.yaml).
    #[arg(long, default_value = ".")]
    vault: PathBuf,
}

#[derive(clap::Args)]
struct FormatArgs {
    /// Machine-readable JSON output.
    #[arg(long, conflicts_with = "yaml")]
    json: bool,
    /// Machine-readable YAML output.
    #[arg(long)]
    yaml: bool,
}

impl FormatArgs {
    fn format(&self) -> Format {
        Format::from_flags(self.json, self.yaml)
    }
}

#[derive(clap::Args)]
struct TodayArg {
    /// Pretend today is this date (YYYY-MM-DD) for backfill or testing;
    /// defaults to the system date.
    #[arg(long)]
    today: Option<NaiveDate>,
}

impl TodayArg {
    fn resolve(&self) -> NaiveDate {
        self.today.unwrap_or_else(|| Local::now().date_naive())
    }
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
        #[command(flatten)]
        today: TodayArg,
    },
    /// Create a new task.
    Create {
        /// Task title.
        title: String,
        #[command(flatten)]
        vault: VaultArg,
        /// Priority: low, medium, high, critical.
        #[arg(long)]
        priority: Option<String>,
        /// Effort: small, medium, large.
        #[arg(long)]
        effort: Option<String>,
        /// Type: feature, bug, improvement, chore, docs.
        #[arg(long = "type")]
        task_type: Option<String>,
        /// Phase name (should match a configured phase id).
        #[arg(long)]
        phase: Option<String>,
        /// Tag (repeatable).
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Dependencies as comma-separated task ids (e.g. 008,011).
        #[arg(long, value_delimiter = ',')]
        depends_on: Vec<String>,
        /// Template: feature, bug, chore, or a custom
        /// .taskmd/templates/<name>.md.
        #[arg(long)]
        template: Option<String>,
        /// Markdown body (replaces the template/default body).
        #[arg(long)]
        body: Option<String>,
        #[command(flatten)]
        today: TodayArg,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// List tasks, optionally filtered by a query
    /// (e.g. 'status:pending AND priority>=high').
    List {
        /// Query expression; all tasks when omitted.
        query: Option<String>,
        #[command(flatten)]
        vault: VaultArg,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// Show one task in full (including its body).
    Show {
        /// Task id.
        id: String,
        #[command(flatten)]
        vault: VaultArg,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// Mark a task done (workflow-aware: solo -> completed,
    /// pr-review -> in-review).
    Complete {
        /// Task id.
        id: String,
        /// PR URL to record (pr-review workflow).
        #[arg(long)]
        pr: Option<String>,
        #[command(flatten)]
        vault: VaultArg,
        #[command(flatten)]
        today: TodayArg,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// Cancel a task (will not be completed).
    Cancel {
        /// Task id.
        id: String,
        #[command(flatten)]
        vault: VaultArg,
        #[command(flatten)]
        today: TodayArg,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// Reopen a task (back to pending; clears terminal timestamps).
    Reopen {
        /// Task id.
        id: String,
        #[command(flatten)]
        vault: VaultArg,
        #[command(flatten)]
        today: TodayArg,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// Set an explicit status (full enum, e.g. in-progress, blocked).
    Status {
        /// Task id.
        id: String,
        /// New status: pending, in-progress, in-review, completed, blocked,
        /// cancelled.
        status: String,
        #[command(flatten)]
        vault: VaultArg,
        #[command(flatten)]
        today: TodayArg,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// Recommend the next task(s) to work on (taskmd-compatible ranking).
    Next {
        #[command(flatten)]
        vault: VaultArg,
        /// Maximum number of recommendations.
        #[arg(long, default_value_t = 5)]
        limit: usize,
        /// Only small-effort tasks.
        #[arg(long)]
        quick_wins: bool,
        /// Only tasks on the critical path.
        #[arg(long)]
        critical: bool,
        /// Only tasks in this phase.
        #[arg(long)]
        phase: Option<String>,
        /// Order by configured phase before score.
        #[arg(long)]
        strict_phases: bool,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// Lint the vault against the taskmd spec (exit 1 on errors, 2 on
    /// warnings with --strict).
    Validate {
        #[command(flatten)]
        vault: VaultArg,
        /// Treat warnings as failures (exit code 2).
        #[arg(long)]
        strict: bool,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// Full-text search across task titles and bodies.
    Search {
        /// Text to search for (case-insensitive substring).
        text: String,
        #[command(flatten)]
        vault: VaultArg,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// Export the dependency graph (human = Graphviz DOT).
    Graph {
        #[command(flatten)]
        vault: VaultArg,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// Computed vault metrics (counts by status/priority/phase, ready/blocked).
    Stats {
        #[command(flatten)]
        vault: VaultArg,
        #[command(flatten)]
        format: FormatArgs,
    },
    /// Serve the web UI (React SPA + JSON API) over the vault.
    Web {
        #[command(flatten)]
        vault: VaultArg,
        /// Address to bind. Default is loopback; opt in to a Tailscale IP or
        /// 0.0.0.0 for remote access (the tailnet is the security boundary).
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: SocketAddr,
        /// Directory holding the pre-built SPA bundle (`bun build` output).
        #[arg(long, env = "KARAMD_WEB_DIR", default_value = "dist")]
        web_dir: PathBuf,
        /// Command a task's "run" session spawns in a PTY (cwd = vault).
        #[arg(long, env = "KARAMD_RUN_COMMAND", default_value = "claude")]
        run_command: String,
    },
}

/// Render a single task result for the mutating/detail verbs.
fn print_task(format: Format, view: &output::TaskView, human: String) -> Result<()> {
    match format {
        Format::Human => println!("{human}"),
        Format::Json => println!("{}", output::to_json(view)?),
        Format::Yaml => println!("{}", output::to_yaml(view)?),
    }
    Ok(())
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
/// `key:year` and for monthly `key:year-month`, so we match the `key:` prefix
/// plus the trigger's discriminator shape.
fn marker_belongs(marker: &str, rule: &Rule) -> bool {
    let discriminator = marker
        .strip_prefix(&rule.key)
        .and_then(|rest| rest.strip_prefix(':'));
    match rule.trigger {
        Trigger::AfterCompletion => marker == rule.key,
        Trigger::Calendar => discriminator
            .is_some_and(|year| year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit())),
        Trigger::Monthly => discriminator.is_some_and(is_year_month),
    }
}

/// Is `s` a `YYYY-MM` discriminator (as produced by [`due::monthly_due`])?
fn is_year_month(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..].iter().all(u8::is_ascii_digit)
}

/// Decide whether `rule` is due today given the tasks already in the vault.
/// Returns the `recurring` marker to stamp on the new task, or `None` to skip.
///
/// This is where each trigger's dedup contract lives:
///   - after_completion: an open task blocks; otherwise due by interval since
///     the most recent completion.
///   - calendar: due inside the lead window, unless a task for that target year
///     already exists (which is why early completion can't re-trigger).
///   - monthly: like calendar, with a `year-month` discriminator instead of a
///     year.
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
        Trigger::Monthly => {
            let day = rule.day_of_month.context("monthly needs day_of_month")?;
            let lead = rule.lead_days.context("monthly needs lead_days")?;
            match due::monthly_due(today, day, lead) {
                Some(ym) => {
                    let marker = format!("{}:{ym}", rule.key);
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
    generate_from_rules(vault, &rules, today, dry_run)
}

/// The core of [`generate`] over already-loaded rules: validate, scan existing
/// tasks, and materialize every due one. Exposed so the web UI (#013) can
/// dry-run a proposed rule set without going through a file.
pub fn generate_from_rules(
    vault: &Path,
    rules: &[Rule],
    today: NaiveDate,
    dry_run: bool,
) -> Result<Report> {
    rule::validate_all(rules)?;

    let tasks_dir = task::tasks_dir(vault);
    let existing = task::scan_dir(&tasks_dir)?;
    let mut next = task::next_id(&existing);

    let mut report = Report::default();
    for rule in rules {
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

/// CLI entry point: parse `args` and dispatch to the subcommand.
pub fn run<I, T>(args: I) -> Result<ExitCode>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    // Parse in the generic shim, dispatch in a plain fn: `run` is
    // monomorphized per caller and a closure inside it would count as a dead
    // function copy in every instantiation that skips it.
    dispatch(Cli::parse_from(args))
}

fn dispatch(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Commands::Generate {
            vault,
            config,
            dry_run,
            today,
        } => {
            let today = today.resolve();
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
            Ok(ExitCode::SUCCESS)
        }
        Commands::Create {
            title,
            vault,
            priority,
            effort,
            task_type,
            phase,
            tags,
            depends_on,
            template,
            body,
            today,
            format,
        } => {
            let spec = verbs::CreateSpec {
                title,
                priority,
                effort,
                task_type,
                phase,
                tags,
                dependencies: depends_on,
                template,
                body,
            };
            let view = verbs::create(
                &vault.vault,
                &spec,
                today.resolve(),
                &mut taskmd::SystemEntropy::default(),
            )?;
            let human = format!(
                "karamd: created {} ({})",
                view.id,
                view.file.as_deref().unwrap_or("?")
            );
            print_task(format.format(), &view, human)?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::List {
            query,
            vault,
            format,
        } => {
            let (views, invalid) = verbs::list(&vault.vault, query.as_deref())?;
            match format.format() {
                Format::Human => println!("{}", output::task_table(&views)),
                Format::Json => println!("{}", output::to_json(&views)?),
                Format::Yaml => println!("{}", output::to_yaml(&views)?),
            }
            if invalid > 0 {
                eprintln!("karamd: warning: {invalid} broken task file(s); run `karamd validate`");
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Show { id, vault, format } => {
            let view = verbs::show(&vault.vault, &id)?;
            let human = format!(
                "{} {} [{} / {}]\n\n{}",
                view.id,
                view.title,
                view.status,
                view.priority,
                view.body.as_deref().unwrap_or("")
            );
            print_task(format.format(), &view, human)?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Complete {
            id,
            pr,
            vault,
            today,
            format,
        } => {
            let view = verbs::complete(&vault.vault, &id, pr.as_deref(), today.resolve())?;
            let human = format!("karamd: {} -> {}", view.id, view.status);
            print_task(format.format(), &view, human)?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Cancel {
            id,
            vault,
            today,
            format,
        } => {
            let view = verbs::set_status(&vault.vault, &id, Status::Cancelled, today.resolve())?;
            let human = format!("karamd: {} -> cancelled", view.id);
            print_task(format.format(), &view, human)?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Reopen {
            id,
            vault,
            today,
            format,
        } => {
            let view = verbs::set_status(&vault.vault, &id, Status::Pending, today.resolve())?;
            let human = format!("karamd: {} -> pending", view.id);
            print_task(format.format(), &view, human)?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Status {
            id,
            status,
            vault,
            today,
            format,
        } => {
            let status = Status::parse(&status).with_context(|| {
                format!(
                    "invalid status `{status}` (pending, in-progress, in-review, completed, \
                     blocked, cancelled)"
                )
            })?;
            let view = verbs::set_status(&vault.vault, &id, status, today.resolve())?;
            let human = format!("karamd: {} -> {}", view.id, view.status);
            print_task(format.format(), &view, human)?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Next {
            vault,
            limit,
            quick_wins,
            critical,
            phase,
            strict_phases,
            format,
        } => {
            let v = taskmd::Vault::open(&vault.vault)?;
            let scan = v.scan()?;
            let phase_order: Vec<String> = v
                .config
                .phases
                .iter()
                .map(|p| p.key().to_string())
                .collect();
            let opts = next::Options {
                limit,
                quick_wins,
                critical,
                phase,
                strict_phases,
            };
            let report = next::recommend(&scan.tasks, &phase_order, &opts);
            match format.format() {
                Format::Human => println!("{}", next::render_human(&report)),
                // JSON/YAML print only the recommendations array, matching
                // `taskmd next --format json` for parity diffing.
                Format::Json => println!("{}", output::to_json(&report.recommendations)?),
                Format::Yaml => println!("{}", output::to_yaml(&report.recommendations)?),
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Validate {
            vault,
            strict,
            format,
        } => {
            let report = validate::validate(&vault.vault)?;
            match format.format() {
                Format::Human => println!("{}", validate::render_human(&report)),
                Format::Json => println!("{}", output::to_json(&report)?),
                Format::Yaml => println!("{}", output::to_yaml(&report)?),
            }
            Ok(ExitCode::from(report.exit_code(strict)))
        }
        Commands::Search {
            text,
            vault,
            format,
        } => {
            let (views, invalid) = verbs::search(&vault.vault, &text)?;
            match format.format() {
                Format::Human => println!("{}", output::task_table(&views)),
                Format::Json => println!("{}", output::to_json(&views)?),
                Format::Yaml => println!("{}", output::to_yaml(&views)?),
            }
            if invalid > 0 {
                eprintln!("karamd: warning: {invalid} broken task file(s); run `karamd validate`");
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Graph { vault, format } => {
            let v = taskmd::Vault::open(&vault.vault)?;
            let scan = v.scan()?;
            let graph = taskmd::Graph::build(&scan.tasks);
            let view = analyze::GraphView::build(&scan.tasks, &graph);
            match format.format() {
                Format::Human => println!("{}", analyze::to_dot(&view)),
                Format::Json => println!("{}", output::to_json(&view)?),
                Format::Yaml => println!("{}", output::to_yaml(&view)?),
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Stats { vault, format } => {
            let v = taskmd::Vault::open(&vault.vault)?;
            let scan = v.scan()?;
            let graph = taskmd::Graph::build(&scan.tasks);
            let view = analyze::StatsView::build(&scan.tasks, &graph, scan.invalid.len());
            match format.format() {
                Format::Human => println!("{}", analyze::render_stats(&view)),
                Format::Json => println!("{}", output::to_json(&view)?),
                Format::Yaml => println!("{}", output::to_yaml(&view)?),
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Web {
            vault,
            bind,
            web_dir,
            run_command,
        } => web::serve_blocking(bind, vault.vault, web_dir, run_command),
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
    const MONTHLY: &str =
        "- key: topup\n  title: Top up\n  trigger: monthly\n  day_of_month: 12\n  lead_days: 7\n";

    #[test]
    fn marker_belongs_matches_correctly() {
        let after = &rule::load_rules(AFTER).unwrap()[0];
        let cal = &rule::load_rules(CAL).unwrap()[0];
        let monthly = &rule::load_rules(MONTHLY).unwrap()[0];
        assert!(marker_belongs("checkin", after));
        assert!(!marker_belongs("checkin:2026", after));
        assert!(marker_belongs("bday:2026", cal));
        assert!(!marker_belongs("bday", cal));
        assert!(!marker_belongs("bday:20xx", cal));
        assert!(!marker_belongs("other:2026", cal));
        assert!(!marker_belongs("bday:2026-07", cal));
        assert!(marker_belongs("topup:2026-07", monthly));
        assert!(!marker_belongs("topup", monthly));
        assert!(!marker_belongs("topup:2026", monthly));
        assert!(!marker_belongs("topup:2026-7x", monthly));
        assert!(!marker_belongs("topup:20x6-07", monthly));
        assert!(!marker_belongs("topup:2026_07", monthly));
        assert!(!marker_belongs("other:2026-07", monthly));
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
    fn monthly_creates_once_per_month_and_survives_early_completion() {
        let (vault, config) = vault_with_rules(MONTHLY);
        // Inside the 7-day window before Jul 12.
        let r = generate(&vault, &config, day(2026, 7, 6), false).unwrap();
        assert_eq!(r.created.len(), 1);
        assert_eq!(r.created[0].marker, "topup:2026-07");

        // Complete it early, still inside the window.
        let path = vault.join("tasks").join(&r.created[0].filename);
        let content = fs::read_to_string(&path)
            .unwrap()
            .replace("status: pending", "status: completed");
        fs::write(&path, content).unwrap();

        // Re-run inside the window: the year-month marker blocks re-creation.
        assert_eq!(
            generate(&vault, &config, day(2026, 7, 10), false).unwrap(),
            Report::default()
        );

        // Next month's window opens: a new occurrence appears.
        let next = generate(&vault, &config, day(2026, 8, 5), false).unwrap();
        assert_eq!(next.created.len(), 1);
        assert_eq!(next.created[0].marker, "topup:2026-08");
    }

    #[test]
    fn monthly_outside_window_creates_nothing() {
        let (vault, config) = vault_with_rules(MONTHLY);
        assert_eq!(
            generate(&vault, &config, day(2026, 7, 1), false).unwrap(),
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
            day_of_month: None,
            phase: None,
            priority: None,
            tags: vec![],
            body: None,
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
    fn decide_monthly_missing_day_of_month_errors() {
        let mut r = bare_rule(Trigger::Monthly);
        r.lead_days = Some(7);
        assert!(decide(&r, &[], day(2026, 7, 1)).is_err());
    }

    #[test]
    fn decide_monthly_missing_lead_days_errors() {
        let mut r = bare_rule(Trigger::Monthly);
        r.day_of_month = Some(12);
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

    /// Run karamd with string args against a vault (prepends the binary name).
    fn run_in(vault: &Path, args: &[&str]) -> Result<ExitCode> {
        let mut full: Vec<OsString> = vec!["karamd".into()];
        for a in args {
            full.push((*a).into());
        }
        // Every command that takes --vault gets it appended.
        full.push("--vault".into());
        full.push(vault.as_os_str().to_owned());
        run(full)
    }

    #[test]
    fn run_create_and_show_and_list() {
        let vault = tempdir();
        fs::create_dir_all(vault.join("tasks")).unwrap();
        run_in(
            &vault,
            &[
                "create",
                "My first task",
                "--priority",
                "high",
                "--tag",
                "core",
                "--today",
                "2026-07-02",
            ],
        )
        .unwrap();
        assert!(vault.join("tasks/001-my-first-task.md").exists());
        // Machine formats and the human table all render.
        run_in(&vault, &["list"]).unwrap();
        run_in(&vault, &["list", "status:pending", "--json"]).unwrap();
        run_in(&vault, &["list", "priority>=high", "--yaml"]).unwrap();
        run_in(&vault, &["show", "001"]).unwrap();
        run_in(&vault, &["show", "001", "--json"]).unwrap();
    }

    #[test]
    fn run_create_with_template_and_machine_output() {
        let vault = tempdir();
        run_in(
            &vault,
            &["create", "Bug hunt", "--template", "bug", "--json"],
        )
        .unwrap();
        let raw = fs::read_to_string(vault.join("tasks/001-bug-hunt.md")).unwrap();
        assert!(raw.contains("type: bug"));
        assert!(raw.contains("## Steps to Reproduce"));
    }

    #[test]
    fn run_status_transitions_e2e() {
        let vault = tempdir();
        fs::create_dir_all(vault.join("tasks")).unwrap();
        fs::write(
            vault.join("tasks/001-a.md"),
            "---\nid: \"001\"\ntitle: A\nstatus: pending\n---\n",
        )
        .unwrap();
        let read = || fs::read_to_string(vault.join("tasks/001-a.md")).unwrap();

        run_in(&vault, &["status", "001", "in-progress"]).unwrap();
        assert!(read().contains("status: in-progress"));

        run_in(&vault, &["complete", "001", "--today", "2026-07-02"]).unwrap();
        assert!(read().contains("status: completed"));
        assert!(read().contains("completed_at: 2026-07-02"));

        run_in(&vault, &["reopen", "001", "--yaml"]).unwrap();
        assert!(read().contains("status: pending"));
        assert!(!read().contains("completed_at"));

        run_in(
            &vault,
            &["cancel", "001", "--today", "2026-07-03", "--json"],
        )
        .unwrap();
        assert!(read().contains("status: cancelled"));
        assert!(read().contains("cancelled_at: 2026-07-03"));
    }

    #[test]
    fn run_complete_respects_pr_review_workflow() {
        let vault = tempdir();
        fs::write(vault.join(".taskmd.yaml"), "workflow: pr-review\n").unwrap();
        fs::create_dir_all(vault.join("tasks")).unwrap();
        fs::write(
            vault.join("tasks/001-a.md"),
            "---\nid: \"001\"\ntitle: A\nstatus: in-progress\n---\n",
        )
        .unwrap();
        run_in(&vault, &["complete", "001", "--pr", "https://x/pull/9"]).unwrap();
        let raw = fs::read_to_string(vault.join("tasks/001-a.md")).unwrap();
        assert!(raw.contains("status: in-review"));
        assert!(raw.contains("https://x/pull/9"));
    }

    #[test]
    fn run_invalid_status_and_bad_query_error() {
        let vault = tempdir();
        fs::create_dir_all(vault.join("tasks")).unwrap();
        fs::write(
            vault.join("tasks/001-a.md"),
            "---\nid: \"001\"\ntitle: A\n---\n",
        )
        .unwrap();
        let err = run_in(&vault, &["status", "001", "done"]).unwrap_err();
        assert!(err.to_string().contains("invalid status `done`"));
        assert!(run_in(&vault, &["list", "bogus:x"]).is_err());
        assert!(run_in(&vault, &["show", "404"]).is_err());
        // A failing create propagates through the CLI arm too.
        assert!(run_in(&vault, &["create", "X", "--priority", "urgent"]).is_err());
    }

    #[test]
    fn run_web_arm_builds_runtime_and_serves() {
        // The web arm spins up a runtime and serves; binding to TEST-NET-1
        // (never local) fails immediately, so block_on returns and the arm's
        // lines are exercised without a server that runs forever.
        let vault = tempdir();
        fs::create_dir_all(vault.join("tasks")).unwrap();
        let err = run_in(
            &vault,
            &["web", "--bind", "192.0.2.1:8787", "--web-dir", "dist"],
        )
        .unwrap_err();
        assert!(err.to_string().contains("binding"));
    }

    #[test]
    fn run_search_graph_stats_all_formats() {
        let vault = tempdir();
        fs::create_dir_all(vault.join("tasks")).unwrap();
        fs::write(
            vault.join("tasks/001-a.md"),
            "---\nid: \"001\"\ntitle: Login fix\nstatus: completed\n---\n\n# Login fix\n\ndone\n",
        )
        .unwrap();
        fs::write(
            vault.join("tasks/002-b.md"),
            "---\nid: \"002\"\ntitle: Other\nstatus: pending\ndependencies: [\"001\"]\n---\n\n# Other\n\nlogin in body\n",
        )
        .unwrap();
        // A broken file drives the warning path in search.
        fs::write(
            vault.join("tasks/003-broken.md"),
            "---\nid: \"003\"\nstatus: pending\n---\n",
        )
        .unwrap();

        run_in(&vault, &["search", "login"]).unwrap();
        run_in(&vault, &["search", "login", "--json"]).unwrap();
        run_in(&vault, &["search", "login", "--yaml"]).unwrap();
        run_in(&vault, &["graph"]).unwrap();
        run_in(&vault, &["graph", "--json"]).unwrap();
        run_in(&vault, &["graph", "--yaml"]).unwrap();
        run_in(&vault, &["stats"]).unwrap();
        run_in(&vault, &["stats", "--json"]).unwrap();
        run_in(&vault, &["stats", "--yaml"]).unwrap();

        // Vault-open failure propagates through graph/stats too.
        fs::write(vault.join(".taskmd.yaml"), "dir: [unclosed\n").unwrap();
        assert!(run_in(&vault, &["graph"]).is_err());
        assert!(run_in(&vault, &["stats"]).is_err());
        assert!(run_in(&vault, &["search", "x"]).is_err());
    }

    #[test]
    fn run_next_all_formats_and_flags() {
        let vault = tempdir();
        fs::write(
            vault.join(".taskmd.yaml"),
            "phases:\n  - id: v1\n    name: V1\n",
        )
        .unwrap();
        fs::create_dir_all(vault.join("tasks")).unwrap();
        fs::write(
            vault.join("tasks/001-a.md"),
            "---\nid: \"001\"\ntitle: A\nstatus: pending\npriority: high\nphase: v1\neffort: small\n---\n",
        )
        .unwrap();
        fs::write(
            vault.join("tasks/002-b.md"),
            "---\nid: \"002\"\ntitle: B\nstatus: pending\npriority: critical\ndependencies: [\"001\"]\n---\n",
        )
        .unwrap();
        run_in(&vault, &["next"]).unwrap();
        run_in(&vault, &["next", "--json", "--limit", "1"]).unwrap();
        run_in(&vault, &["next", "--yaml"]).unwrap();
        run_in(&vault, &["next", "--quick-wins", "--critical"]).unwrap();
        run_in(&vault, &["next", "--phase", "v1", "--strict-phases"]).unwrap();
        // Vault open failure propagates.
        fs::write(vault.join(".taskmd.yaml"), "dir: [broken\n").unwrap();
        assert!(run_in(&vault, &["next"]).is_err());
    }

    #[test]
    fn run_list_warns_about_invalid_files() {
        let vault = tempdir();
        fs::create_dir_all(vault.join("tasks")).unwrap();
        fs::write(
            vault.join("tasks/001-broken.md"),
            "---\nid: \"001\"\nstatus: pending\n---\n",
        )
        .unwrap();
        // Exercises the warning branch (output goes to stderr).
        run_in(&vault, &["list"]).unwrap();
    }

    #[test]
    fn run_validate_exit_codes() {
        let vault = tempdir();
        fs::create_dir_all(vault.join("tasks")).unwrap();
        // Clean vault -> success in every format.
        fs::write(
            vault.join("tasks/001-a.md"),
            "---\nid: \"001\"\ntitle: A\ncreated_at: 2026-07-01\n---\n",
        )
        .unwrap();
        assert_eq!(run_in(&vault, &["validate"]).unwrap(), ExitCode::SUCCESS);
        assert_eq!(
            run_in(&vault, &["validate", "--json"]).unwrap(),
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_in(&vault, &["validate", "--yaml"]).unwrap(),
            ExitCode::SUCCESS
        );

        // A warning (missing created_at): 0 normally, 2 under --strict.
        fs::write(
            vault.join("tasks/002-b.md"),
            "---\nid: \"002\"\ntitle: B\n---\n",
        )
        .unwrap();
        assert_eq!(run_in(&vault, &["validate"]).unwrap(), ExitCode::SUCCESS);
        assert_eq!(
            run_in(&vault, &["validate", "--strict"]).unwrap(),
            ExitCode::from(2)
        );

        // An error (bad enum): exit 1 regardless.
        fs::write(
            vault.join("tasks/003-c.md"),
            "---\nid: \"003\"\ntitle: C\nstatus: done\ncreated_at: 2026-07-01\n---\n",
        )
        .unwrap();
        assert_eq!(run_in(&vault, &["validate"]).unwrap(), ExitCode::from(1));

        // Vault open failure is an error, not an exit code.
        fs::write(vault.join(".taskmd.yaml"), "dir: [broken\n").unwrap();
        assert!(run_in(&vault, &["validate"]).is_err());
    }
}
