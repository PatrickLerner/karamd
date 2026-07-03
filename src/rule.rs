//! Recurring-rule model and rules-file parsing.
//!
//! A rules file is a YAML list of [`Rule`]s. Each rule describes one recurring
//! task and how its due-ness is decided (see [`Trigger`]).

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Which due-check governs a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// Due `every_days` after the last occurrence was *completed*.
    AfterCompletion,
    /// Due `lead_days` before a fixed annual `MM-DD` date, once per year.
    Calendar,
    /// Due `lead_days` before a fixed day of the month, once per month.
    Monthly,
}

/// One recurring-task definition from the rules file. Serializes back to the
/// same YAML shape (absent optionals and empty tags omitted) so the web UI can
/// round-trip the rules file without churning it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    /// Stable dedup marker. Written as `recurring: <key>` (after_completion) or
    /// `recurring: "<key>:<year>"` (calendar) frontmatter on generated tasks.
    pub key: String,
    /// Task title (also drives the filename slug).
    pub title: String,
    pub trigger: Trigger,
    /// after_completion: days after last completion before the next is due.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every_days: Option<i64>,
    /// calendar: fixed annual date as `MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annual: Option<String>,
    /// monthly: day of the month the task is due (1-31; 29-31 clamp to the
    /// month's last day, so `31` still fires in 30-day months and February).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_of_month: Option<u32>,
    /// calendar/monthly: how many days before the occurrence the task should
    /// appear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_days: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Optional markdown body for the generated task. When present it replaces
    /// the default `TODO` stub (see [`crate::task::render_task`]); when absent
    /// the stub is used so existing rules keep their output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

impl Rule {
    /// Reject rules missing the fields their trigger needs, so a typo in the
    /// rules file fails loudly instead of silently generating nothing.
    pub fn validate(&self) -> Result<()> {
        // A body, if given, must carry text: an empty/whitespace-only body would
        // emit a task with no content, worse than the fallback stub.
        if let Some(body) = &self.body
            && body.trim().is_empty()
        {
            bail!("rule `{}`: `body` must not be empty", self.key);
        }
        match self.trigger {
            Trigger::AfterCompletion => {
                if self.every_days.is_none() {
                    bail!(
                        "rule `{}`: after_completion requires `every_days`",
                        self.key
                    );
                }
            }
            Trigger::Calendar => {
                let annual = self
                    .annual
                    .as_deref()
                    .with_context(|| format!("rule `{}`: calendar requires `annual`", self.key))?;
                if self.lead_days.is_none() {
                    bail!("rule `{}`: calendar requires `lead_days`", self.key);
                }
                // Reject a malformed `annual` here (leap year so `02-29` is
                // accepted) instead of failing deep inside a run.
                crate::due::calendar_occurrence(2000, annual).with_context(|| {
                    format!("rule `{}`: invalid `annual` (need MM-DD)", self.key)
                })?;
            }
            Trigger::Monthly => {
                let day = self.day_of_month.with_context(|| {
                    format!("rule `{}`: monthly requires `day_of_month`", self.key)
                })?;
                if !(1..=31).contains(&day) {
                    bail!("rule `{}`: `day_of_month` must be 1-31", self.key);
                }
                let lead = self.lead_days.with_context(|| {
                    format!("rule `{}`: monthly requires `lead_days`", self.key)
                })?;
                // A lead of 28+ days would overlap the previous occurrence's
                // window (February), making the rule due every single day.
                if !(0..=27).contains(&lead) {
                    bail!("rule `{}`: monthly `lead_days` must be 0-27", self.key);
                }
            }
        }
        Ok(())
    }
}

/// Parse a rules file's contents into a list of [`Rule`]s.
pub fn load_rules(raw: &str) -> Result<Vec<Rule>> {
    Ok(serde_norway::from_str(raw)?)
}

/// Serialize rules to the YAML rules-file form (a plain list).
pub fn dump_rules(rules: &[Rule]) -> Result<String> {
    Ok(serde_norway::to_string(rules)?)
}

/// Write a rules file atomically (temp file in the same dir + rename), so a
/// concurrent sync or reader never sees a half-written file. The parent dir
/// (the vault root) is expected to exist; a missing/unwritable one surfaces as
/// a loud write error. Callers should [`validate_all`] first — this only
/// serializes and writes.
pub fn write_rules(path: &Path, rules: &[Rule]) -> Result<()> {
    let body = dump_rules(rules)?;
    let tmp = path.with_extension("karamd.tmp");
    std::fs::write(&tmp, &body).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

/// Validate a whole rules file: keys must be unique (a shared key would make two
/// rules fight over one dedup marker), and every rule must pass [`Rule::validate`].
pub fn validate_all(rules: &[Rule]) -> Result<()> {
    let mut seen = HashSet::new();
    for rule in rules {
        if !seen.insert(rule.key.as_str()) {
            bail!("duplicate rule key `{}`", rule.key);
        }
        rule.validate()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
- key: periodic-checkin
  title: "Reach out to [[Someone]]"
  trigger: after_completion
  every_days: 18
  phase: next
  priority: medium
  tags: [personal]
- key: annual-birthday
  title: "Birthday"
  trigger: calendar
  annual: "07-20"
  lead_days: 10
  priority: high
- key: monthly-topup
  title: "Top up account"
  trigger: monthly
  day_of_month: 12
  lead_days: 7
"#;

    #[test]
    fn parses_all_triggers() {
        let rules = load_rules(SAMPLE).unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].trigger, Trigger::AfterCompletion);
        assert_eq!(rules[0].every_days, Some(18));
        assert_eq!(rules[0].phase.as_deref(), Some("next"));
        assert_eq!(rules[0].tags, vec!["personal"]);
        assert_eq!(rules[1].trigger, Trigger::Calendar);
        assert_eq!(rules[1].annual.as_deref(), Some("07-20"));
        assert_eq!(rules[1].lead_days, Some(10));
        assert!(rules[1].tags.is_empty());
        assert_eq!(rules[2].trigger, Trigger::Monthly);
        assert_eq!(rules[2].day_of_month, Some(12));
        assert_eq!(rules[2].lead_days, Some(7));
    }

    #[test]
    fn rejects_malformed_yaml() {
        assert!(load_rules("key: : :").is_err());
    }

    #[test]
    fn validate_accepts_complete_rules() {
        for rule in load_rules(SAMPLE).unwrap() {
            rule.validate().unwrap();
        }
    }

    #[test]
    fn validate_rejects_after_completion_without_every_days() {
        let raw = "- key: k\n  title: t\n  trigger: after_completion\n";
        let err = load_rules(raw).unwrap()[0].validate().unwrap_err();
        assert!(err.to_string().contains("every_days"));
    }

    #[test]
    fn validate_rejects_calendar_without_annual() {
        let raw = "- key: k\n  title: t\n  trigger: calendar\n  lead_days: 5\n";
        let err = load_rules(raw).unwrap()[0].validate().unwrap_err();
        assert!(err.to_string().contains("annual"));
    }

    #[test]
    fn validate_rejects_calendar_without_lead_days() {
        let raw = "- key: k\n  title: t\n  trigger: calendar\n  annual: \"01-01\"\n";
        let err = load_rules(raw).unwrap()[0].validate().unwrap_err();
        assert!(err.to_string().contains("lead_days"));
    }

    #[test]
    fn validate_rejects_malformed_annual() {
        let raw =
            "- key: k\n  title: t\n  trigger: calendar\n  annual: \"99-99\"\n  lead_days: 5\n";
        let err = load_rules(raw).unwrap()[0].validate().unwrap_err();
        assert!(err.to_string().contains("annual"));
    }

    #[test]
    fn validate_rejects_monthly_without_day_of_month() {
        let raw = "- key: k\n  title: t\n  trigger: monthly\n  lead_days: 7\n";
        let err = load_rules(raw).unwrap()[0].validate().unwrap_err();
        assert!(err.to_string().contains("day_of_month"));
    }

    #[test]
    fn validate_rejects_monthly_without_lead_days() {
        let raw = "- key: k\n  title: t\n  trigger: monthly\n  day_of_month: 12\n";
        let err = load_rules(raw).unwrap()[0].validate().unwrap_err();
        assert!(err.to_string().contains("lead_days"));
    }

    #[test]
    fn validate_rejects_monthly_day_out_of_range() {
        for day in ["0", "32"] {
            let raw = format!(
                "- key: k\n  title: t\n  trigger: monthly\n  day_of_month: {day}\n  lead_days: 7\n"
            );
            let err = load_rules(&raw).unwrap()[0].validate().unwrap_err();
            assert!(err.to_string().contains("day_of_month"));
        }
    }

    #[test]
    fn validate_rejects_monthly_lead_out_of_range() {
        for lead in ["-1", "28"] {
            let raw = format!(
                "- key: k\n  title: t\n  trigger: monthly\n  day_of_month: 12\n  lead_days: {lead}\n"
            );
            let err = load_rules(&raw).unwrap()[0].validate().unwrap_err();
            assert!(err.to_string().contains("lead_days"));
        }
    }

    #[test]
    fn validate_accepts_monthly_bounds() {
        for (day, lead) in [("1", "0"), ("31", "27")] {
            let raw = format!(
                "- key: k\n  title: t\n  trigger: monthly\n  day_of_month: {day}\n  lead_days: {lead}\n"
            );
            load_rules(&raw).unwrap()[0].validate().unwrap();
        }
    }

    #[test]
    fn validate_accepts_leap_day_annual() {
        let raw =
            "- key: k\n  title: t\n  trigger: calendar\n  annual: \"02-29\"\n  lead_days: 5\n";
        load_rules(raw).unwrap()[0].validate().unwrap();
    }

    #[test]
    fn validate_all_rejects_duplicate_keys() {
        let raw = "- key: dup\n  title: a\n  trigger: after_completion\n  every_days: 3\n- key: dup\n  title: b\n  trigger: after_completion\n  every_days: 5\n";
        let rules = load_rules(raw).unwrap();
        let err = validate_all(&rules).unwrap_err();
        assert!(err.to_string().contains("duplicate rule key `dup`"));
    }

    #[test]
    fn validate_all_accepts_unique_valid_rules() {
        validate_all(&load_rules(SAMPLE).unwrap()).unwrap();
    }

    #[test]
    fn parses_optional_body() {
        let raw = "- key: k\n  title: t\n  trigger: after_completion\n  every_days: 3\n  body: |\n    ## Objective\n\n    Do the thing.\n";
        let rules = load_rules(raw).unwrap();
        assert_eq!(
            rules[0].body.as_deref(),
            Some("## Objective\n\nDo the thing.\n")
        );
    }

    #[test]
    fn body_defaults_to_none() {
        assert!(load_rules(SAMPLE).unwrap()[0].body.is_none());
    }

    #[test]
    fn validate_rejects_empty_body() {
        let raw =
            "- key: k\n  title: t\n  trigger: after_completion\n  every_days: 3\n  body: \"   \"\n";
        let err = load_rules(raw).unwrap()[0].validate().unwrap_err();
        assert!(err.to_string().contains("body"));
    }

    #[test]
    fn validate_accepts_non_empty_body() {
        let raw =
            "- key: k\n  title: t\n  trigger: after_completion\n  every_days: 3\n  body: real\n";
        load_rules(raw).unwrap()[0].validate().unwrap();
    }

    #[test]
    fn dump_then_load_round_trips_all_triggers() {
        let rules = load_rules(SAMPLE).unwrap();
        let yaml = dump_rules(&rules).unwrap();
        // Absent optionals are omitted, not emitted as null.
        assert!(!yaml.contains("null"));
        assert!(yaml.contains("trigger: after_completion"));
        let reparsed = load_rules(&yaml).unwrap();
        assert_eq!(reparsed.len(), 3);
        assert_eq!(reparsed[0].every_days, Some(18));
        assert_eq!(reparsed[1].annual.as_deref(), Some("07-20"));
        assert_eq!(reparsed[2].day_of_month, Some(12));
    }

    #[test]
    fn write_rules_is_atomic_and_reparseable() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("karamd-rules-{uniq}"));
        let parent = dir.join("nested");
        std::fs::create_dir_all(&parent).unwrap();
        let path = parent.join(".taskmd.recurring.yaml");
        let rules = load_rules(SAMPLE).unwrap();
        write_rules(&path, &rules).unwrap();
        // The temp file was renamed away; only the target remains.
        let entries: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec![".taskmd.recurring.yaml".to_string()]);
        let reparsed = load_rules(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(reparsed.len(), 3);
        // Overwriting an existing file works too.
        write_rules(&path, &rules[..1]).unwrap();
        assert_eq!(
            load_rules(&std::fs::read_to_string(&path).unwrap())
                .unwrap()
                .len(),
            1
        );
    }

    fn rules_tempdir(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("karamd-rules-{tag}-{uniq}"));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    #[cfg(unix)]
    fn write_rules_tmp_write_failure_errors() {
        use std::os::unix::fs::PermissionsExt;
        let base = rules_tempdir("ro");
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = write_rules(&base.join("rules.yaml"), &[]);
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755)).ok();
        assert!(result.unwrap_err().to_string().contains("writing"));
    }

    #[test]
    fn write_rules_rename_onto_directory_errors() {
        // The target path is occupied by a directory, so the final rename fails
        // after the temp write succeeded.
        let base = rules_tempdir("rename");
        let occupied = base.join("rules.yaml");
        std::fs::create_dir_all(occupied.join("inner")).unwrap();
        let err = write_rules(&occupied, &[]).unwrap_err();
        assert!(err.to_string().contains("renaming"));
    }
}
