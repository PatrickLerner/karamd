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
    /// Due on a fixed weekday, once per ISO week (catches up later in the week).
    Weekly,
    /// Due on the Nth (or last) weekday of the month, once per month.
    NthWeekday,
}

/// The `week` field of an `nth_weekday` rule: either a number (`1`-`4`) or the
/// keyword `last`. Untagged so YAML `week: 1` and `week: last` both parse and
/// round-trip to the same shape.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Week {
    Nth(u32),
    Keyword(String),
}

impl Week {
    /// Resolve to a validated [`crate::due::WeekOrdinal`], or `None` if the value
    /// is out of range (`0`, `>4`) or an unknown keyword.
    pub fn ordinal(&self) -> Option<crate::due::WeekOrdinal> {
        match self {
            Week::Nth(n) if (1..=4).contains(n) => Some(crate::due::WeekOrdinal::Nth(*n)),
            Week::Keyword(s) if s == "last" => Some(crate::due::WeekOrdinal::Last),
            _ => None,
        }
    }
}

impl Trigger {
    /// The rules-file spelling of this trigger (matches the `snake_case`
    /// serde form), for error messages.
    fn label(self) -> &'static str {
        match self {
            Trigger::AfterCompletion => "after_completion",
            Trigger::Calendar => "calendar",
            Trigger::Monthly => "monthly",
            Trigger::Weekly => "weekly",
            Trigger::NthWeekday => "nth_weekday",
        }
    }

    /// The trigger-specific fields this trigger legitimately uses. Any *other*
    /// trigger-specific field being set is a rules-file typo (see
    /// [`Rule::reject_foreign_fields`]).
    fn owned_fields(self) -> &'static [&'static str] {
        match self {
            Trigger::AfterCompletion => &["every_days"],
            Trigger::Calendar => &["annual", "lead_days"],
            Trigger::Monthly => &["day_of_month", "lead_days"],
            Trigger::Weekly => &["day_of_week"],
            Trigger::NthWeekday => &["day_of_week", "week"],
        }
    }
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
    /// weekly / nth_weekday: the weekday the task recurs on, one of `mon`,`tue`,
    /// `wed`,`thu`,`fri`,`sat`,`sun`. Any other value is rejected by
    /// [`Rule::validate`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_of_week: Option<String>,
    /// nth_weekday: which occurrence of `day_of_week` in the month (`1`-`4` or
    /// `last`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub week: Option<Week>,
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
        // A field belonging to a different trigger is almost always a typo; the
        // trigger would silently ignore it. Reject it before the per-trigger
        // required-field checks so such mistakes fail loudly.
        self.reject_foreign_fields()?;
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
            Trigger::Weekly => {
                let dow = self.day_of_week.as_deref().with_context(|| {
                    format!("rule `{}`: weekly requires `day_of_week`", self.key)
                })?;
                if crate::due::parse_weekday(dow).is_none() {
                    bail!(
                        "rule `{}`: `day_of_week` must be one of mon,tue,wed,thu,fri,sat,sun",
                        self.key
                    );
                }
            }
            Trigger::NthWeekday => {
                let dow = self.day_of_week.as_deref().with_context(|| {
                    format!("rule `{}`: nth_weekday requires `day_of_week`", self.key)
                })?;
                if crate::due::parse_weekday(dow).is_none() {
                    bail!(
                        "rule `{}`: `day_of_week` must be one of mon,tue,wed,thu,fri,sat,sun",
                        self.key
                    );
                }
                let week = self
                    .week
                    .as_ref()
                    .with_context(|| format!("rule `{}`: nth_weekday requires `week`", self.key))?;
                if week.ordinal().is_none() {
                    bail!("rule `{}`: `week` must be 1-4 or `last`", self.key);
                }
            }
        }
        Ok(())
    }

    /// Reject any trigger-specific field that does not belong to this rule's
    /// trigger. `key`/`title`/`trigger` and the shared cosmetic fields
    /// (`phase`, `priority`, `tags`, `body`) are always allowed; the rest are
    /// owned by exactly one or two triggers (see [`Trigger::owned_fields`]).
    fn reject_foreign_fields(&self) -> Result<()> {
        let owned = self.trigger.owned_fields();
        let present: [(&str, bool); 6] = [
            ("every_days", self.every_days.is_some()),
            ("annual", self.annual.is_some()),
            ("day_of_month", self.day_of_month.is_some()),
            ("day_of_week", self.day_of_week.is_some()),
            ("lead_days", self.lead_days.is_some()),
            ("week", self.week.is_some()),
        ];
        for (name, is_set) in present {
            if is_set && !owned.contains(&name) {
                bail!(
                    "rule `{}`: `{name}` is not valid for a {} trigger",
                    self.key,
                    self.trigger.label()
                );
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
- key: linkedin-weekly
  title: "Evaluate and schedule LinkedIn posts"
  trigger: weekly
  day_of_week: fri
"#;

    #[test]
    fn parses_all_triggers() {
        let rules = load_rules(SAMPLE).unwrap();
        assert_eq!(rules.len(), 4);
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
        assert_eq!(rules[3].trigger, Trigger::Weekly);
        assert_eq!(rules[3].day_of_week.as_deref(), Some("fri"));
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
    fn validate_rejects_weekly_without_day_of_week() {
        let raw = "- key: k\n  title: t\n  trigger: weekly\n";
        let err = load_rules(raw).unwrap()[0].validate().unwrap_err();
        assert!(err.to_string().contains("day_of_week"));
    }

    #[test]
    fn validate_rejects_weekly_invalid_day_of_week() {
        for bad in ["friday", "Fri", "5", "xyz"] {
            let raw = format!("- key: k\n  title: t\n  trigger: weekly\n  day_of_week: {bad}\n");
            let err = load_rules(&raw).unwrap()[0].validate().unwrap_err();
            assert!(err.to_string().contains("day_of_week"));
        }
    }

    #[test]
    fn validate_accepts_weekly_all_weekdays() {
        for dow in ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] {
            let raw = format!("- key: k\n  title: t\n  trigger: weekly\n  day_of_week: {dow}\n");
            load_rules(&raw).unwrap()[0].validate().unwrap();
        }
    }

    #[test]
    fn validate_rejects_foreign_trigger_fields() {
        // Each pairing sets a field owned by a different trigger; validate must
        // reject it, naming the offending field.
        let cases = [
            (
                "after_completion\n  every_days: 3\n  annual: \"01-01\"",
                "annual",
            ),
            (
                "after_completion\n  every_days: 3\n  day_of_month: 5",
                "day_of_month",
            ),
            (
                "after_completion\n  every_days: 3\n  day_of_week: fri",
                "day_of_week",
            ),
            (
                "after_completion\n  every_days: 3\n  lead_days: 2",
                "lead_days",
            ),
            (
                "calendar\n  annual: \"01-01\"\n  lead_days: 2\n  every_days: 3",
                "every_days",
            ),
            (
                "calendar\n  annual: \"01-01\"\n  lead_days: 2\n  day_of_week: fri",
                "day_of_week",
            ),
            (
                "monthly\n  day_of_month: 5\n  lead_days: 2\n  annual: \"01-01\"",
                "annual",
            ),
            (
                "weekly\n  day_of_week: fri\n  day_of_month: 5",
                "day_of_month",
            ),
            ("weekly\n  day_of_week: fri\n  lead_days: 2", "lead_days"),
            ("weekly\n  day_of_week: fri\n  every_days: 3", "every_days"),
            ("weekly\n  day_of_week: fri\n  week: 1", "week"),
            (
                "nth_weekday\n  day_of_week: mon\n  week: 1\n  day_of_month: 5",
                "day_of_month",
            ),
            (
                "monthly\n  day_of_month: 5\n  lead_days: 2\n  week: last",
                "week",
            ),
        ];
        for (spec, field) in cases {
            let raw = format!("- key: k\n  title: t\n  trigger: {spec}\n");
            let err = load_rules(&raw).unwrap()[0].validate().unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains(field), "expected `{field}` in: {msg}");
            assert!(msg.contains("is not valid for"), "wrong message: {msg}");
        }
    }

    #[test]
    fn validate_still_accepts_rules_with_only_owned_fields() {
        // The sample uses only each trigger's own fields, so the foreign-field
        // check must not reject any of them.
        validate_all(&load_rules(SAMPLE).unwrap()).unwrap();
    }

    #[test]
    fn parses_nth_weekday_numeric_and_last() {
        let raw = "- key: a\n  title: t\n  trigger: nth_weekday\n  day_of_week: mon\n  week: 1\n- key: b\n  title: t\n  trigger: nth_weekday\n  day_of_week: fri\n  week: last\n";
        let rules = load_rules(raw).unwrap();
        assert_eq!(rules[0].trigger, Trigger::NthWeekday);
        assert_eq!(rules[0].week, Some(Week::Nth(1)));
        assert_eq!(rules[1].week, Some(Week::Keyword("last".into())));
    }

    #[test]
    fn nth_weekday_round_trips() {
        let raw = "- key: a\n  title: t\n  trigger: nth_weekday\n  day_of_week: mon\n  week: 3\n- key: b\n  title: t\n  trigger: nth_weekday\n  day_of_week: fri\n  week: last\n";
        let rules = load_rules(raw).unwrap();
        let reparsed = load_rules(&dump_rules(&rules).unwrap()).unwrap();
        assert_eq!(reparsed[0].week, Some(Week::Nth(3)));
        assert_eq!(reparsed[1].week, Some(Week::Keyword("last".into())));
    }

    #[test]
    fn validate_rejects_nth_weekday_without_day_of_week() {
        let raw = "- key: k\n  title: t\n  trigger: nth_weekday\n  week: 1\n";
        let err = load_rules(raw).unwrap()[0].validate().unwrap_err();
        assert!(err.to_string().contains("day_of_week"));
    }

    #[test]
    fn validate_rejects_nth_weekday_invalid_day_of_week() {
        let raw =
            "- key: k\n  title: t\n  trigger: nth_weekday\n  day_of_week: friday\n  week: 1\n";
        let err = load_rules(raw).unwrap()[0].validate().unwrap_err();
        assert!(err.to_string().contains("day_of_week"));
    }

    #[test]
    fn validate_rejects_nth_weekday_without_week() {
        let raw = "- key: k\n  title: t\n  trigger: nth_weekday\n  day_of_week: mon\n";
        let err = load_rules(raw).unwrap()[0].validate().unwrap_err();
        assert!(err.to_string().contains("week"));
    }

    #[test]
    fn validate_rejects_nth_weekday_out_of_range_week() {
        for w in ["0", "5", "first"] {
            let raw = format!(
                "- key: k\n  title: t\n  trigger: nth_weekday\n  day_of_week: mon\n  week: {w}\n"
            );
            let err = load_rules(&raw).unwrap()[0].validate().unwrap_err();
            assert!(err.to_string().contains("week"), "for week={w}");
        }
    }

    #[test]
    fn validate_accepts_nth_weekday_bounds() {
        for w in ["1", "4", "last"] {
            let raw = format!(
                "- key: k\n  title: t\n  trigger: nth_weekday\n  day_of_week: mon\n  week: {w}\n"
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
        assert_eq!(reparsed.len(), 4);
        assert_eq!(reparsed[0].every_days, Some(18));
        assert_eq!(reparsed[1].annual.as_deref(), Some("07-20"));
        assert_eq!(reparsed[2].day_of_month, Some(12));
        assert_eq!(reparsed[3].day_of_week.as_deref(), Some("fri"));
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
        assert_eq!(reparsed.len(), 4);
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
