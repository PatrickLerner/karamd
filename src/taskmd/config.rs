//! `.taskmd.yaml` project config: tasks dir, phases, id generation, workflow
//! mode, and scopes. All of it affects what karamd may write, so all of it is
//! parsed — a missing file yields defaults, a malformed one errors loudly
//! (silently falling back could allocate wrong ids or complete tasks in the
//! wrong workflow mode).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// How new task ids are generated (`id.strategy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdStrategy {
    #[default]
    Sequential,
    Prefixed,
    Random,
    Ulid,
}

/// The `id:` section. Defaults per spec: sequential, length 6, padding 3.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct IdConfig {
    pub strategy: IdStrategy,
    /// Required for `prefixed`; emitted as `<prefix><NNN>` (taskmd 0.2.5 puts
    /// no separator between prefix and number: `dr001`).
    pub prefix: String,
    /// Id length for `random` and `ulid`.
    pub length: usize,
    /// Zero-padding width for `sequential` (and the numeric part of
    /// `prefixed`).
    pub padding: usize,
}

impl Default for IdConfig {
    fn default() -> Self {
        IdConfig {
            strategy: IdStrategy::Sequential,
            prefix: String::new(),
            length: 6,
            padding: 3,
        }
    }
}

/// How tasks transition to completion (`workflow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Workflow {
    /// "Mark done" sets `completed` directly.
    #[default]
    Solo,
    /// "Mark done" sets `in-review` (+ a PR url); `completed` happens on merge.
    PrReview,
}

/// One entry of the ordered `phases:` list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Phase {
    /// Stable identifier task `phase:` values reference; falls back to `name`.
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Target date, `YYYY-MM-DD`.
    #[serde(default)]
    pub due: Option<String>,
}

impl Phase {
    /// The key task `phase:` values are matched against.
    pub fn key(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.name)
    }
}

/// Web dashboard settings (`web:`). karamd-specific; taskmd ignores it. Keeps
/// the web's "Today" grouping out of the shared `phases:` entries so a phase
/// rename never silently breaks the tab.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    /// Phase ids merged into the web "Today" tab, in render order. Absent →
    /// [`DEFAULT_TODAY_PHASES`]; present (even empty) → used verbatim.
    pub today: Option<Vec<String>>,
}

/// Fallback set (and order) for the web "Today" tab when `web.today` is unset.
pub const DEFAULT_TODAY_PHASES: [&str; 2] = ["ongoing", "now"];

impl WebConfig {
    /// Phase ids the "Today" tab merges, resolving the default when unset.
    pub fn today_phases(&self) -> Vec<String> {
        self.today
            .clone()
            .unwrap_or_else(|| DEFAULT_TODAY_PHASES.iter().map(|s| s.to_string()).collect())
    }
}

/// How an agent command receives the rendered prompt (`run.agents.<n>.prompt_via`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptVia {
    /// Substitute the prompt into a `{prompt}` token in the command argv.
    #[default]
    Arg,
    /// Pipe the prompt to the command's stdin.
    Stdin,
    /// Write the prompt to a temp file and substitute its path into a
    /// `{prompt_file}` token in the argv.
    File,
}

/// One configured agent command (`run.agents.<name>`). The command is an
/// allowlist: task text never supplies it, only which named agent to use.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AgentSpec {
    /// argv, e.g. `["claude", "-p", "{prompt}", "--permission-mode", "acceptEdits"]`.
    pub command: Vec<String>,
    #[serde(default)]
    pub prompt_via: PromptVia,
}

/// Default prompt template when `run.prompt_template` is unset. Instructs the
/// agent to self-complete so the exit-0-plus-terminal-status success rule holds.
pub const DEFAULT_PROMPT_TEMPLATE: &str = "You are completing a task autonomously and non-interactively. When the work is \
done, run `karamd complete {id}` from the vault so it is marked complete; do not \
ask questions.\n\nTask {id}: {title}\n\n{body}\n";

/// Autonomous task-execution settings (`run:`, #039). karamd-specific; taskmd
/// ignores it. Disabled by default: the whole feature is off unless
/// `run.enabled` is explicitly true (the top-level safety lock).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RunConfig {
    /// Master switch. Absent/false: `karamd run` does nothing and spawns nothing.
    pub enabled: bool,
    /// Default agent name; a task may override via `ai_agent` frontmatter.
    pub agent: String,
    /// Named agent commands. A task's agent must resolve to one of these.
    pub agents: BTreeMap<String, AgentSpec>,
    /// Default working dir for spawned agents; a task may override via
    /// `ai_working_dir`. Absent: the vault root.
    pub working_dir: Option<String>,
    /// Hard per-run timeout (seconds).
    pub timeout_secs: u64,
    /// After this many failed attempts a task is parked (`ai-failed` tag) and
    /// no longer selected.
    pub max_attempts: u32,
    /// Prompt template; `{id}`, `{title}`, `{body}`, `{path}` are interpolated.
    pub prompt_template: String,
    /// Directory for per-run logs (#045). Absent resolves to
    /// `<vault>/.karamd/runs`. Each execution appends a JSON record to
    /// `runs.jsonl` and tees the agent's output to a per-run `.log` file.
    pub log_dir: Option<String>,
    /// How many most-recent run records to keep; older records and their
    /// `.log` files are pruned. `0` keeps everything (no prune).
    pub log_retention: usize,
}

impl RunConfig {
    /// Absolute per-run log directory: `run.log_dir` if set, else
    /// `<vault>/.karamd/runs`.
    pub fn resolve_log_dir(&self, vault_root: &Path) -> PathBuf {
        match &self.log_dir {
            Some(dir) => PathBuf::from(dir),
            None => vault_root.join(".karamd").join("runs"),
        }
    }
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig {
            enabled: false,
            agent: "claude".into(),
            agents: BTreeMap::new(),
            working_dir: None,
            timeout_secs: 900,
            max_attempts: 3,
            prompt_template: DEFAULT_PROMPT_TEMPLATE.into(),
            log_dir: None,
            log_retention: 1000,
        }
    }
}

/// One entry of the `scopes:` map, backing task `touches` values.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Scope {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
}

/// The parsed `.taskmd.yaml`. Unknown keys are ignored (other tools may add
/// their own; karamd itself keeps its rules in a separate file).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Tasks directory relative to the vault root.
    pub dir: String,
    pub phases: Vec<Phase>,
    pub id: IdConfig,
    pub workflow: Workflow,
    pub scopes: BTreeMap<String, Scope>,
    /// Web dashboard settings (karamd-specific; ignored by taskmd).
    pub web: WebConfig,
    /// Autonomous task-execution settings (karamd-specific; ignored by taskmd).
    pub run: RunConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            dir: "tasks".into(),
            phases: Vec::new(),
            id: IdConfig::default(),
            workflow: Workflow::default(),
            scopes: BTreeMap::new(),
            web: WebConfig::default(),
            run: RunConfig::default(),
        }
    }
}

impl Config {
    /// Load `<vault>/.taskmd.yaml`. Missing file: defaults. Unreadable or
    /// malformed: an error, never a silent fallback.
    pub fn load(vault: &Path) -> Result<Config> {
        let path = vault.join(".taskmd.yaml");
        if !path.exists() {
            return Ok(Config::default());
        }
        let raw =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_norway::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }

    /// Absolute tasks dir for a vault root.
    pub fn tasks_dir(&self, vault: &Path) -> PathBuf {
        vault.join(self.dir.trim_start_matches("./"))
    }

    /// Position of a task's `phase` value in the configured phase order, for
    /// sorting. `None` when the phase is not configured (or no phases are).
    pub fn phase_index(&self, phase: &str) -> Option<usize> {
        self.phases.iter().position(|p| p.key() == phase)
    }
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
        let base = std::env::temp_dir().join(format!("karamd-cfg-{uniq}"));
        fs::create_dir_all(&base).unwrap();
        base
    }

    const FULL: &str = r#"
dir: ./work/tasks
workflow: pr-review
id:
  strategy: prefixed
  prefix: dr
  length: 8
  padding: 4
phases:
  - id: core-cli
    name: "Core CLI"
    description: "Core CLI features"
    due: 2026-04-01
  - name: "Web Dashboard"
web:
  today:
    - core-cli
scopes:
  cli/graph:
    description: "Graph rendering"
    paths:
      - "src/graph.rs"
  cli/output:
    paths: []
"#;

    #[test]
    fn parses_full_config() {
        let c: Config = serde_norway::from_str(FULL).unwrap();
        assert_eq!(c.dir, "./work/tasks");
        assert_eq!(c.workflow, Workflow::PrReview);
        assert_eq!(c.id.strategy, IdStrategy::Prefixed);
        assert_eq!(c.id.prefix, "dr");
        assert_eq!(c.id.length, 8);
        assert_eq!(c.id.padding, 4);
        assert_eq!(c.phases.len(), 2);
        assert_eq!(c.phases[0].key(), "core-cli");
        assert_eq!(c.phases[0].name, "Core CLI");
        assert_eq!(c.phases[0].due.as_deref(), Some("2026-04-01"));
        // Phase without id falls back to name as its key.
        assert_eq!(c.phases[1].key(), "Web Dashboard");
        assert_eq!(c.web.today_phases(), vec!["core-cli"]);
        assert_eq!(c.scopes.len(), 2);
        assert_eq!(
            c.scopes["cli/graph"].description.as_deref(),
            Some("Graph rendering")
        );
        assert_eq!(c.scopes["cli/graph"].paths, vec!["src/graph.rs"]);
        assert!(c.scopes["cli/output"].paths.is_empty());
    }

    #[test]
    fn defaults_match_spec() {
        let c = Config::default();
        assert_eq!(c.dir, "tasks");
        assert_eq!(c.workflow, Workflow::Solo);
        assert_eq!(c.id.strategy, IdStrategy::Sequential);
        assert_eq!(c.id.length, 6);
        assert_eq!(c.id.padding, 3);
        assert!(c.phases.is_empty());
        assert!(c.scopes.is_empty());
    }

    #[test]
    fn partial_id_section_fills_defaults() {
        let c: Config = serde_norway::from_str("id:\n  strategy: random\n").unwrap();
        assert_eq!(c.id.strategy, IdStrategy::Random);
        assert_eq!(c.id.length, 6);
        assert_eq!(c.id.padding, 3);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let c: Config = serde_norway::from_str("dir: t\nfuture_key: whatever\n").unwrap();
        assert_eq!(c.dir, "t");
    }

    #[test]
    fn load_missing_file_is_default() {
        let vault = tempdir();
        assert_eq!(Config::load(&vault).unwrap(), Config::default());
    }

    #[test]
    fn load_reads_file() {
        let vault = tempdir();
        fs::write(vault.join(".taskmd.yaml"), "dir: ./mytasks\n").unwrap();
        let c = Config::load(&vault).unwrap();
        assert_eq!(c.dir, "./mytasks");
        assert_eq!(c.tasks_dir(&vault), vault.join("mytasks"));
    }

    #[test]
    fn load_malformed_errors_loudly() {
        let vault = tempdir();
        fs::write(vault.join(".taskmd.yaml"), "dir: [unclosed\n").unwrap();
        let err = Config::load(&vault).unwrap_err();
        assert!(err.to_string().contains("parsing"));
    }

    #[test]
    fn load_unreadable_errors() {
        let vault = tempdir();
        // A directory named like the config file: read_to_string fails.
        fs::create_dir(vault.join(".taskmd.yaml")).unwrap();
        let err = Config::load(&vault).unwrap_err();
        assert!(err.to_string().contains("reading"));
    }

    #[test]
    fn tasks_dir_default() {
        let c = Config::default();
        assert_eq!(c.tasks_dir(Path::new("/v")), PathBuf::from("/v/tasks"));
    }

    #[test]
    fn phase_index_orders_by_config() {
        let c: Config = serde_norway::from_str(FULL).unwrap();
        assert_eq!(c.phase_index("core-cli"), Some(0));
        assert_eq!(c.phase_index("Web Dashboard"), Some(1));
        assert_eq!(c.phase_index("nope"), None);
        assert_eq!(Config::default().phase_index("anything"), None);
    }

    #[test]
    fn web_today_defaults_when_absent() {
        // No `web:` section at all: the resolved Today set is the default.
        let c = Config::default();
        assert_eq!(c.web.today, None);
        assert_eq!(c.web.today_phases(), vec!["ongoing", "now"]);
        // A `web:` section without `today:` behaves the same.
        let c: Config = serde_norway::from_str("web: {}\n").unwrap();
        assert_eq!(c.web.today_phases(), vec!["ongoing", "now"]);
    }

    #[test]
    fn web_today_parsed_in_order() {
        let c: Config =
            serde_norway::from_str("web:\n  today:\n    - now\n    - ongoing\n    - triage\n")
                .unwrap();
        assert_eq!(
            c.web.today_phases(),
            vec!["now", "ongoing", "triage"],
            "order is preserved verbatim, not sorted"
        );
    }

    #[test]
    fn web_today_empty_list_is_respected() {
        // An explicit empty list means "merge no named phases" (only unphased
        // open tasks fall into Today) and must not fall back to the default.
        let c: Config = serde_norway::from_str("web:\n  today: []\n").unwrap();
        assert_eq!(c.web.today, Some(Vec::new()));
        assert!(c.web.today_phases().is_empty());
    }

    #[test]
    fn run_config_defaults_are_off() {
        let c = Config::default();
        assert!(!c.run.enabled);
        assert_eq!(c.run.agent, "claude");
        assert!(c.run.agents.is_empty());
        assert_eq!(c.run.working_dir, None);
        assert_eq!(c.run.timeout_secs, 900);
        assert_eq!(c.run.max_attempts, 3);
        assert!(c.run.prompt_template.contains("{id}"));
        // Per-run log defaults (#045): no explicit dir, keep 1000 records.
        assert_eq!(c.run.log_dir, None);
        assert_eq!(c.run.log_retention, 1000);
        assert_eq!(
            c.run.resolve_log_dir(Path::new("/v")),
            PathBuf::from("/v/.karamd/runs")
        );
        // Absent `run:` section behaves as the default (off).
        let c: Config = serde_norway::from_str("dir: t\n").unwrap();
        assert!(!c.run.enabled);
    }

    #[test]
    fn run_config_parses_log_knobs() {
        let raw = "run:\n  enabled: true\n  log_dir: /var/log/karamd\n  log_retention: 50\n";
        let c: Config = serde_norway::from_str(raw).unwrap();
        assert_eq!(c.run.log_dir.as_deref(), Some("/var/log/karamd"));
        assert_eq!(c.run.log_retention, 50);
        // An explicit dir wins over the vault-relative default.
        assert_eq!(
            c.run.resolve_log_dir(Path::new("/v")),
            PathBuf::from("/var/log/karamd")
        );
    }

    #[test]
    fn run_config_parses_agents_and_prompt_via() {
        let raw = "run:\n  enabled: true\n  agent: opencode\n  working_dir: /repo\n  timeout_secs: 60\n  max_attempts: 2\n  prompt_template: \"do {id}\"\n  agents:\n    claude:\n      command: [claude, -p, \"{prompt}\"]\n    opencode:\n      command: [opencode, run]\n      prompt_via: stdin\n    filed:\n      command: [tool, \"{prompt_file}\"]\n      prompt_via: file\n";
        let c: Config = serde_norway::from_str(raw).unwrap();
        assert!(c.run.enabled);
        assert_eq!(c.run.agent, "opencode");
        assert_eq!(c.run.working_dir.as_deref(), Some("/repo"));
        assert_eq!(c.run.timeout_secs, 60);
        assert_eq!(c.run.max_attempts, 2);
        assert_eq!(c.run.prompt_template, "do {id}");
        assert_eq!(c.run.agents.len(), 3);
        assert_eq!(c.run.agents["claude"].prompt_via, PromptVia::Arg);
        assert_eq!(
            c.run.agents["claude"].command,
            vec!["claude", "-p", "{prompt}"]
        );
        assert_eq!(c.run.agents["opencode"].prompt_via, PromptVia::Stdin);
        assert_eq!(c.run.agents["filed"].prompt_via, PromptVia::File);
    }

    #[test]
    fn run_config_rejects_agent_without_command() {
        assert!(
            serde_norway::from_str::<Config>("run:\n  agents:\n    x:\n      prompt_via: arg\n")
                .is_err()
        );
        // Unknown prompt_via is a loud error, not a silent default.
        assert!(
            serde_norway::from_str::<Config>(
                "run:\n  agents:\n    x:\n      command: [c]\n      prompt_via: telepathy\n"
            )
            .is_err()
        );
    }

    #[test]
    fn workflow_and_strategy_enum_spellings() {
        let c: Config = serde_norway::from_str("workflow: solo\n").unwrap();
        assert_eq!(c.workflow, Workflow::Solo);
        for (raw, want) in [
            ("sequential", IdStrategy::Sequential),
            ("prefixed", IdStrategy::Prefixed),
            ("random", IdStrategy::Random),
            ("ulid", IdStrategy::Ulid),
        ] {
            let c: Config = serde_norway::from_str(&format!("id:\n  strategy: {raw}\n")).unwrap();
            assert_eq!(c.id.strategy, want);
        }
        // An unknown enum value is a loud parse error, not a silent default.
        assert!(serde_norway::from_str::<Config>("workflow: chaotic\n").is_err());
        assert!(serde_norway::from_str::<Config>("id:\n  strategy: guid\n").is_err());
    }
}
