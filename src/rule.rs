//! Recurring-rule model and rules-file parsing.
//!
//! A rules file is a YAML list of [`Rule`]s. Each rule describes one recurring
//! task and how its due-ness is decided (see [`Trigger`]).

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Which due-check governs a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// Due `every_days` after the last occurrence was *completed*.
    AfterCompletion,
    /// Due `lead_days` before a fixed annual `MM-DD` date, once per year.
    Calendar,
}

/// One recurring-task definition from the rules file.
#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    /// Stable dedup marker. Written as `recurring: <key>` (after_completion) or
    /// `recurring: "<key>:<year>"` (calendar) frontmatter on generated tasks.
    pub key: String,
    /// Task title (also drives the filename slug).
    pub title: String,
    pub trigger: Trigger,
    /// after_completion: days after last completion before the next is due.
    #[serde(default)]
    pub every_days: Option<i64>,
    /// calendar: fixed annual date as `MM-DD`.
    #[serde(default)]
    pub annual: Option<String>,
    /// calendar: how many days before `annual` the task should appear.
    #[serde(default)]
    pub lead_days: Option<i64>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Rule {
    /// Reject rules missing the fields their trigger needs, so a typo in the
    /// rules file fails loudly instead of silently generating nothing.
    pub fn validate(&self) -> Result<()> {
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
        }
        Ok(())
    }
}

/// Parse a rules file's contents into a list of [`Rule`]s.
pub fn load_rules(raw: &str) -> Result<Vec<Rule>> {
    Ok(serde_norway::from_str(raw)?)
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
"#;

    #[test]
    fn parses_both_triggers() {
        let rules = load_rules(SAMPLE).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].trigger, Trigger::AfterCompletion);
        assert_eq!(rules[0].every_days, Some(18));
        assert_eq!(rules[0].phase.as_deref(), Some("next"));
        assert_eq!(rules[0].tags, vec!["personal"]);
        assert_eq!(rules[1].trigger, Trigger::Calendar);
        assert_eq!(rules[1].annual.as_deref(), Some("07-20"));
        assert_eq!(rules[1].lead_days, Some(10));
        assert!(rules[1].tags.is_empty());
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
}
