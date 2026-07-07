//! Pure due-check logic for both triggers. Every function takes `today`
//! explicitly so tests are deterministic and never touch the clock.

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, Weekday};

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

/// Last day of `month` in `year` (the day before the first of the next month).
fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .expect("first of month is always valid")
        .pred_opt()
        .expect("first of month has a predecessor")
        .day()
}

/// Resolve `day` to a concrete date in (`year`, `month`). Days past the end of
/// the month (29-31) clamp to its last day, so a rule for the 31st still fires
/// in 30-day months and February.
pub fn monthly_occurrence(year: i32, month: u32, day: u32) -> NaiveDate {
    let day = day.min(last_day_of_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day).expect("clamped day is valid")
}

/// monthly: if today falls within the `lead_days` window before an occurrence,
/// return that occurrence's `YYYY-MM` (the dedup discriminator). Checks this
/// month and next month, so a window that straddles a month boundary resolves
/// to next month's occurrence.
pub fn monthly_due(today: NaiveDate, day_of_month: u32, lead_days: i64) -> Option<String> {
    monthly_due_every(today, day_of_month, lead_days, 1, None)
}

/// monthly, honouring an `interval` (every Nth month) aligned to `anchor` (see
/// [`on_cadence`]). Only an on-cadence occurrence's month is eligible; an
/// off-cadence month is skipped even if today is inside its lead window.
pub fn monthly_due_every(
    today: NaiveDate,
    day_of_month: u32,
    lead_days: i64,
    interval: u32,
    anchor: Option<NaiveDate>,
) -> Option<String> {
    let (mut year, mut month) = (today.year(), today.month());
    for _ in 0..2 {
        let occ = monthly_occurrence(year, month, day_of_month);
        let delta = (occ - today).num_days();
        if (0..=lead_days).contains(&delta)
            && on_cadence(
                month_index_ym(year, month),
                anchor.map(month_index),
                interval,
            )
        {
            return Some(format!("{year:04}-{month:02}"));
        }
        (year, month) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
    }
    None
}

/// Is `index` on the cadence set by `interval` (every Nth period) and `anchor`?
/// `interval <= 1` is always on cadence (the no-op default). With a larger
/// interval, a period is on cadence when its distance from the anchor period is
/// a multiple of `interval`; a missing anchor aligns to a fixed epoch
/// (CE-day-derived period 0), so behaviour stays deterministic without one.
fn on_cadence(index: i64, anchor_index: Option<i64>, interval: u32) -> bool {
    if interval <= 1 {
        return true;
    }
    (index - anchor_index.unwrap_or(0)).rem_euclid(interval as i64) == 0
}

/// Monotonic month counter (`year*12 + month-1`) for interval maths.
fn month_index_ym(year: i32, month: u32) -> i64 {
    year as i64 * 12 + (month as i64 - 1)
}

/// Month counter of a date (see [`month_index_ym`]).
fn month_index(d: NaiveDate) -> i64 {
    month_index_ym(d.year(), d.month())
}

/// Monotonic ISO-week counter for interval maths: the count of whole weeks from
/// the CE epoch to the Monday of `d`'s ISO week. Consecutive ISO weeks differ by
/// exactly 1, across year boundaries.
fn iso_week_index(d: NaiveDate) -> i64 {
    let iso = d.iso_week();
    let monday = NaiveDate::from_isoywd_opt(iso.year(), iso.week(), Weekday::Mon)
        .expect("an ISO week always has a Monday");
    monday.num_days_from_ce() as i64 / 7
}

/// Parse a canonical lowercase three-letter weekday (`mon`..`sun`). Deliberately
/// strict: only these seven exact forms are accepted so a rules-file typo fails
/// loudly instead of being coerced by a lenient parser.
pub fn parse_weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "mon" => Weekday::Mon,
        "tue" => Weekday::Tue,
        "wed" => Weekday::Wed,
        "thu" => Weekday::Thu,
        "fri" => Weekday::Fri,
        "sat" => Weekday::Sat,
        "sun" => Weekday::Sun,
        _ => return None,
    })
}

/// weekly: once per ISO week, pinned to the `target` weekday. Due when today is
/// on or after `target` within the current ISO week, returning that week's
/// `YYYY-Www` discriminator (the dedup marker suffix). Because the run compares
/// weekdays inside one Monday-Sunday ISO week, a `generate` that misses the
/// target day still fires on the following days (Sat/Sun for a Friday rule) to
/// catch up, then resets next week. A fully missed week is never backfilled: the
/// discriminator is always the *current* ISO week, never a past one.
pub fn weekly_due(today: NaiveDate, target: Weekday) -> Option<String> {
    weekly_due_every(today, target, 1, None)
}

/// weekly, honouring an `interval` (every Nth ISO week) aligned to `anchor` (see
/// [`on_cadence`]). An off-cadence week never fires.
pub fn weekly_due_every(
    today: NaiveDate,
    target: Weekday,
    interval: u32,
    anchor: Option<NaiveDate>,
) -> Option<String> {
    if today.weekday().number_from_monday() < target.number_from_monday() {
        return None;
    }
    if !on_cadence(iso_week_index(today), anchor.map(iso_week_index), interval) {
        return None;
    }
    let iso = today.iso_week();
    Some(format!("{:04}-W{:02}", iso.year(), iso.week()))
}

/// Which occurrence of a weekday within a month a `nth_weekday` rule targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeekOrdinal {
    /// The Nth matching weekday (1-4; higher is rejected by validation because a
    /// 5th does not exist in every month).
    Nth(u32),
    /// The final matching weekday of the month (4th or 5th, whichever is last).
    Last,
}

/// The concrete date of the `ord`-th `weekday` in (`year`, `month`). Nth(1-4)
/// and Last always exist in every month, so this is total (no `Option`).
pub fn nth_weekday_occurrence(
    year: i32,
    month: u32,
    weekday: Weekday,
    ord: WeekOrdinal,
) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("first of month is valid");
    // Days from the 1st to the first matching weekday (0-6).
    let offset = (7 + weekday.num_days_from_monday() as i64
        - first.weekday().num_days_from_monday() as i64)
        % 7;
    let first_day = 1 + offset as u32; // day-of-month of the first match (1-7)
    let day = match ord {
        WeekOrdinal::Nth(n) => first_day + 7 * (n - 1),
        WeekOrdinal::Last => {
            let steps = (last_day_of_month(year, month) - first_day) / 7;
            first_day + 7 * steps
        }
    };
    NaiveDate::from_ymd_opt(year, month, day).expect("nth/last weekday is within the month")
}

/// nth_weekday: once per month, on or after the `ord`-th `weekday`. Returns this
/// month's `YYYY-MM` discriminator when today is on or after that date (so a
/// late run catches up), else `None`. Like `weekly`, a fully missed month is not
/// backfilled: the discriminator is always the current month.
pub fn nth_weekday_due(today: NaiveDate, weekday: Weekday, ord: WeekOrdinal) -> Option<String> {
    nth_weekday_due_every(today, weekday, ord, 1, None)
}

/// nth_weekday, honouring an `interval` (every Nth month) aligned to `anchor`
/// (see [`on_cadence`]). An off-cadence month never fires.
pub fn nth_weekday_due_every(
    today: NaiveDate,
    weekday: Weekday,
    ord: WeekOrdinal,
    interval: u32,
    anchor: Option<NaiveDate>,
) -> Option<String> {
    let occ = nth_weekday_occurrence(today.year(), today.month(), weekday, ord);
    if today < occ {
        return None;
    }
    if !on_cadence(month_index(today), anchor.map(month_index), interval) {
        return None;
    }
    Some(format!("{:04}-{:02}", today.year(), today.month()))
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

    #[test]
    fn monthly_occurrence_normal_day() {
        assert_eq!(monthly_occurrence(2026, 7, 12), d(2026, 7, 12));
    }

    #[test]
    fn monthly_occurrence_clamps_to_short_month() {
        assert_eq!(monthly_occurrence(2026, 6, 31), d(2026, 6, 30));
        assert_eq!(monthly_occurrence(2026, 2, 30), d(2026, 2, 28));
        assert_eq!(monthly_occurrence(2028, 2, 31), d(2028, 2, 29)); // leap year
    }

    #[test]
    fn monthly_occurrence_december() {
        // Exercises the year rollover inside last_day_of_month.
        assert_eq!(monthly_occurrence(2026, 12, 31), d(2026, 12, 31));
    }

    #[test]
    fn monthly_inside_window_returns_year_month() {
        assert_eq!(
            monthly_due(d(2026, 7, 5), 12, 7),
            Some("2026-07".to_string())
        );
    }

    #[test]
    fn monthly_on_the_day_is_due() {
        assert_eq!(
            monthly_due(d(2026, 7, 12), 12, 7),
            Some("2026-07".to_string())
        );
    }

    #[test]
    fn monthly_before_window_not_due() {
        assert_eq!(monthly_due(d(2026, 7, 4), 12, 7), None);
    }

    #[test]
    fn monthly_after_the_day_not_due_until_next_window() {
        assert_eq!(monthly_due(d(2026, 7, 13), 12, 7), None);
        assert_eq!(
            monthly_due(d(2026, 8, 5), 12, 7),
            Some("2026-08".to_string())
        );
    }

    #[test]
    fn monthly_window_straddles_month_boundary() {
        // today Jul 28, day 2, lead 7 -> Aug 2 is 5 days out.
        assert_eq!(
            monthly_due(d(2026, 7, 28), 2, 7),
            Some("2026-08".to_string())
        );
    }

    #[test]
    fn monthly_window_straddles_year_boundary() {
        assert_eq!(
            monthly_due(d(2026, 12, 28), 2, 7),
            Some("2027-01".to_string())
        );
    }

    #[test]
    fn monthly_zero_lead_only_on_the_day() {
        assert_eq!(monthly_due(d(2026, 7, 11), 12, 0), None);
        assert_eq!(
            monthly_due(d(2026, 7, 12), 12, 0),
            Some("2026-07".to_string())
        );
    }

    #[test]
    fn monthly_clamped_day_fires_in_short_month() {
        // day 31 in June clamps to Jun 30.
        assert_eq!(
            monthly_due(d(2026, 6, 28), 31, 7),
            Some("2026-06".to_string())
        );
    }

    #[test]
    fn parse_weekday_accepts_all_seven() {
        assert_eq!(parse_weekday("mon"), Some(Weekday::Mon));
        assert_eq!(parse_weekday("tue"), Some(Weekday::Tue));
        assert_eq!(parse_weekday("wed"), Some(Weekday::Wed));
        assert_eq!(parse_weekday("thu"), Some(Weekday::Thu));
        assert_eq!(parse_weekday("fri"), Some(Weekday::Fri));
        assert_eq!(parse_weekday("sat"), Some(Weekday::Sat));
        assert_eq!(parse_weekday("sun"), Some(Weekday::Sun));
    }

    #[test]
    fn parse_weekday_rejects_other_forms() {
        // Long names, capitalisation, and integers are all rejected: the rules
        // file must use the one canonical form.
        for bad in ["Fri", "friday", "5", "", "mo", "fri "] {
            assert_eq!(parse_weekday(bad), None);
        }
    }

    #[test]
    fn weekly_before_target_day_not_due() {
        // Thu 2026-07-09, rule pinned to Friday: not yet due this ISO week.
        assert_eq!(weekly_due(d(2026, 7, 9), Weekday::Fri), None);
    }

    #[test]
    fn weekly_on_target_day_is_due() {
        // Fri 2026-07-10 is ISO 2026-W28.
        assert_eq!(
            weekly_due(d(2026, 7, 10), Weekday::Fri),
            Some("2026-W28".to_string())
        );
    }

    #[test]
    fn weekly_after_target_day_catches_up_same_week() {
        // Sat and Sun of the same ISO week still fire (self-healing) with the
        // same discriminator, so a late run is not missed.
        assert_eq!(
            weekly_due(d(2026, 7, 11), Weekday::Fri),
            Some("2026-W28".to_string())
        );
        assert_eq!(
            weekly_due(d(2026, 7, 12), Weekday::Fri),
            Some("2026-W28".to_string())
        );
    }

    #[test]
    fn weekly_monday_target_is_due_every_day() {
        // A Monday rule is due Monday and every later day of the week.
        assert_eq!(
            weekly_due(d(2026, 7, 6), Weekday::Mon),
            Some("2026-W28".to_string())
        );
        assert_eq!(
            weekly_due(d(2026, 7, 9), Weekday::Mon),
            Some("2026-W28".to_string())
        );
    }

    #[test]
    fn weekly_sunday_target_only_on_sunday() {
        // Sat 2026-07-11 is before Sunday; Sun 2026-07-12 is due.
        assert_eq!(weekly_due(d(2026, 7, 11), Weekday::Sun), None);
        assert_eq!(
            weekly_due(d(2026, 7, 12), Weekday::Sun),
            Some("2026-W28".to_string())
        );
    }

    #[test]
    fn weekly_discriminator_uses_iso_year_not_calendar_year() {
        // Fri 2027-01-01 belongs to ISO week 2026-W53: the discriminator tracks
        // the ISO year, so a New-Year Friday is not mislabelled 2027-W01.
        assert_eq!(
            weekly_due(d(2027, 1, 1), Weekday::Fri),
            Some("2026-W53".to_string())
        );
    }

    #[test]
    fn nth_weekday_occurrence_first_and_fourth() {
        assert_eq!(
            nth_weekday_occurrence(2026, 7, Weekday::Mon, WeekOrdinal::Nth(1)),
            d(2026, 7, 6)
        );
        assert_eq!(
            nth_weekday_occurrence(2026, 7, Weekday::Mon, WeekOrdinal::Nth(4)),
            d(2026, 7, 27)
        );
    }

    #[test]
    fn nth_weekday_occurrence_last_is_fifth_or_fourth() {
        // July 2026 has five Fridays; last is the 5th.
        assert_eq!(
            nth_weekday_occurrence(2026, 7, Weekday::Fri, WeekOrdinal::Last),
            d(2026, 7, 31)
        );
        // February 2026 has four Wednesdays; last is the 4th.
        assert_eq!(
            nth_weekday_occurrence(2026, 2, Weekday::Wed, WeekOrdinal::Last),
            d(2026, 2, 25)
        );
        // Last Saturday of August 2026.
        assert_eq!(
            nth_weekday_occurrence(2026, 8, Weekday::Sat, WeekOrdinal::Last),
            d(2026, 8, 29)
        );
    }

    #[test]
    fn nth_weekday_before_occurrence_not_due() {
        // First Monday is Jul 6; the 5th is before it.
        assert_eq!(
            nth_weekday_due(d(2026, 7, 5), Weekday::Mon, WeekOrdinal::Nth(1)),
            None
        );
    }

    #[test]
    fn nth_weekday_on_and_after_occurrence_due_same_month() {
        assert_eq!(
            nth_weekday_due(d(2026, 7, 6), Weekday::Mon, WeekOrdinal::Nth(1)),
            Some("2026-07".to_string())
        );
        // Catch-up later in the month keeps the same discriminator.
        assert_eq!(
            nth_weekday_due(d(2026, 7, 20), Weekday::Mon, WeekOrdinal::Nth(1)),
            Some("2026-07".to_string())
        );
    }

    #[test]
    fn nth_weekday_last_friday_due() {
        assert_eq!(
            nth_weekday_due(d(2026, 7, 30), Weekday::Fri, WeekOrdinal::Last),
            None
        );
        assert_eq!(
            nth_weekday_due(d(2026, 7, 31), Weekday::Fri, WeekOrdinal::Last),
            Some("2026-07".to_string())
        );
    }

    #[test]
    fn interval_one_matches_the_plain_helpers() {
        // The interval-aware variants with interval 1 / no anchor reproduce the
        // plain functions exactly.
        assert_eq!(
            weekly_due_every(d(2026, 7, 10), Weekday::Fri, 1, None),
            weekly_due(d(2026, 7, 10), Weekday::Fri)
        );
        assert_eq!(
            monthly_due_every(d(2026, 7, 6), 12, 7, 1, None),
            monthly_due(d(2026, 7, 6), 12, 7)
        );
        assert_eq!(
            nth_weekday_due_every(d(2026, 7, 6), Weekday::Mon, WeekOrdinal::Nth(1), 1, None),
            nth_weekday_due(d(2026, 7, 6), Weekday::Mon, WeekOrdinal::Nth(1))
        );
    }

    #[test]
    fn weekly_biweekly_with_anchor_fires_on_alternating_weeks() {
        let anchor = Some(d(2026, 7, 10)); // Fri of W28
        assert_eq!(
            weekly_due_every(d(2026, 7, 10), Weekday::Fri, 2, anchor),
            Some("2026-W28".to_string())
        );
        // W29 is one week off the anchor -> skipped.
        assert_eq!(
            weekly_due_every(d(2026, 7, 17), Weekday::Fri, 2, anchor),
            None
        );
        // W30 is two weeks off -> fires again.
        assert_eq!(
            weekly_due_every(d(2026, 7, 24), Weekday::Fri, 2, anchor),
            Some("2026-W30".to_string())
        );
    }

    #[test]
    fn weekly_biweekly_without_anchor_is_deterministic_alternation() {
        // With no anchor the cadence aligns to a fixed epoch; consecutive weeks
        // must alternate (exactly one of two adjacent weeks fires).
        let a = weekly_due_every(d(2026, 7, 10), Weekday::Fri, 2, None);
        let b = weekly_due_every(d(2026, 7, 17), Weekday::Fri, 2, None);
        assert!(
            a.is_some() ^ b.is_some(),
            "adjacent biweekly weeks must alternate"
        );
    }

    #[test]
    fn monthly_interval_with_anchor_skips_off_months() {
        let anchor = Some(d(2026, 7, 12));
        assert_eq!(
            monthly_due_every(d(2026, 7, 6), 12, 7, 2, anchor),
            Some("2026-07".to_string())
        );
        assert_eq!(monthly_due_every(d(2026, 8, 6), 12, 7, 2, anchor), None);
        assert_eq!(
            monthly_due_every(d(2026, 9, 6), 12, 7, 2, anchor),
            Some("2026-09".to_string())
        );
    }

    #[test]
    fn nth_weekday_interval_with_anchor_skips_off_months() {
        let anchor = Some(d(2026, 7, 6)); // first Monday of July
        assert_eq!(
            nth_weekday_due_every(d(2026, 7, 6), Weekday::Mon, WeekOrdinal::Nth(1), 2, anchor),
            Some("2026-07".to_string())
        );
        // First Monday of August is off cadence.
        assert_eq!(
            nth_weekday_due_every(d(2026, 8, 3), Weekday::Mon, WeekOrdinal::Nth(1), 2, anchor),
            None
        );
        // First Monday of September is back on cadence.
        assert_eq!(
            nth_weekday_due_every(d(2026, 9, 7), Weekday::Mon, WeekOrdinal::Nth(1), 2, anchor),
            Some("2026-09".to_string())
        );
    }
}
