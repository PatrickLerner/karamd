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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            dir: "tasks".into(),
            phases: Vec::new(),
            id: IdConfig::default(),
            workflow: Workflow::default(),
            scopes: BTreeMap::new(),
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
