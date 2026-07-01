//! Pure due-check logic for both triggers. Every function takes `today`
//! explicitly so tests are deterministic and never touch the clock.

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};

/// after_completion: due when at least `every_days` have elapsed since the last
/// completion. A rule that has never produced a completed task (`None`) is due
/// on first run. Because it keys off the *last* completion (not a schedule), a
/// missed run catches up with a single task rather than a backlog.
pub fn after_completion_due(
    today: NaiveDate,
    every_days: i64,
    last_completion: Option<NaiveDate>,
) -> bool {
    match last_completion {
        None => true,
        Some(done) => (today - done).num_days() >= every_days,
    }
}

/// Resolve `MM-DD` to a concrete date in `year`. `02-29` in a non-leap year
/// clamps to `02-28` so the rule still fires every year.
pub fn calendar_occurrence(year: i32, mm_dd: &str) -> Result<NaiveDate> {
    let (m, d) = mm_dd.split_once('-').context("annual date must be MM-DD")?;
    let month: u32 = m.parse().context("invalid month in annual date")?;
    let day: u32 = d.parse().context("invalid day in annual date")?;
    // Feb 29 in a common year clamps to Feb 28 so the rule still fires yearly.
    let day = if month == 2 && day == 29 && NaiveDate::from_ymd_opt(year, 2, 29).is_none() {
        28
    } else {
        day
    };
    NaiveDate::from_ymd_opt(year, month, day)
        .with_context(|| format!("invalid annual date `{mm_dd}`"))
}

/// calendar: if today falls within the `lead_days` window before an occurrence,
/// return the year of that occurrence (the dedup discriminator). Checks this
/// year and next year, so a window that straddles Dec 31 / Jan 1 resolves to
/// next year's occurrence.
pub fn calendar_due(today: NaiveDate, mm_dd: &str, lead_days: i64) -> Result<Option<i32>> {
    for year in [today.year(), today.year() + 1] {
        let occ = calendar_occurrence(year, mm_dd)?;
        let delta = (occ - today).num_days();
        if (0..=lead_days).contains(&delta) {
            return Ok(Some(year));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn after_completion_never_run_is_due() {
        assert!(after_completion_due(d(2026, 7, 1), 18, None));
    }

    #[test]
    fn after_completion_before_interval_not_due() {
        let last = d(2026, 6, 20);
        assert!(!after_completion_due(d(2026, 7, 1), 18, Some(last))); // 11 days
    }

    #[test]
    fn after_completion_exactly_due() {
        let last = d(2026, 6, 13);
        assert!(after_completion_due(d(2026, 7, 1), 18, Some(last))); // 18 days
    }

    #[test]
    fn after_completion_overdue() {
        let last = d(2026, 5, 1);
        assert!(after_completion_due(d(2026, 7, 1), 18, Some(last)));
    }

    #[test]
    fn occurrence_normal_date() {
        assert_eq!(calendar_occurrence(2026, "07-20").unwrap(), d(2026, 7, 20));
    }

    #[test]
    fn occurrence_leap_day_clamps_in_common_year() {
        assert_eq!(calendar_occurrence(2026, "02-29").unwrap(), d(2026, 2, 28));
    }

    #[test]
    fn occurrence_leap_day_stays_in_leap_year() {
        assert_eq!(calendar_occurrence(2028, "02-29").unwrap(), d(2028, 2, 29));
    }

    #[test]
    fn occurrence_rejects_bad_format() {
        assert!(calendar_occurrence(2026, "0720").is_err()); // no separator
        assert!(calendar_occurrence(2026, "aa-bb").is_err()); // month unparseable
        assert!(calendar_occurrence(2026, "12-zz").is_err()); // day unparseable
        assert!(calendar_occurrence(2026, "13-40").is_err()); // out of range
    }

    #[test]
    fn calendar_outside_window_not_due() {
        // 20 days out, lead is 10
        assert_eq!(calendar_due(d(2026, 6, 30), "07-20", 10).unwrap(), None);
    }

    #[test]
    fn calendar_inside_window_returns_year() {
        assert_eq!(
            calendar_due(d(2026, 7, 12), "07-20", 10).unwrap(),
            Some(2026)
        );
    }

    #[test]
    fn calendar_on_the_day_is_due() {
        assert_eq!(
            calendar_due(d(2026, 7, 20), "07-20", 10).unwrap(),
            Some(2026)
        );
    }

    #[test]
    fn calendar_after_the_day_not_due() {
        assert_eq!(calendar_due(d(2026, 7, 21), "07-20", 10).unwrap(), None);
    }

    #[test]
    fn calendar_year_rollover_returns_next_year() {
        // today Dec 28 2026, annual Jan 02, lead 10 -> next year's occurrence
        assert_eq!(
            calendar_due(d(2026, 12, 28), "01-02", 10).unwrap(),
            Some(2027)
        );
    }

    #[test]
    fn calendar_propagates_parse_error() {
        assert!(calendar_due(d(2026, 7, 1), "nope", 10).is_err());
    }
}
