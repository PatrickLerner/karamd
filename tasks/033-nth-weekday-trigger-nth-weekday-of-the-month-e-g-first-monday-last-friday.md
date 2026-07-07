---
id: '033'
title: 'nth_weekday trigger: Nth weekday of the month (e.g. first Monday, last Friday)'
status: completed
created_at: 2026-07-07
priority: medium
type: feature
tags:
- generator
completed_at: 2026-07-07
---

## Objective

Support "first Monday", "last Friday" style recurrence: a task pinned to the Nth
occurrence of a weekday within a month, once per month. Common for standing
meetings and monthly reviews. None of the existing triggers express it: `monthly`
is a fixed day-of-month, `weekly` is every ISO week.

## Rule format

```yaml
- key: ops-review
  title: "Monthly ops review"
  trigger: nth_weekday
  day_of_week: mon      # mon|tue|wed|thu|fri|sat|sun (reuse weekly's parser)
  week: 1               # 1..4, or "last" for the final such weekday in the month
```

## Semantics

- **Once per month.** Dedup marker `key:YYYY-MM`, mirroring `monthly`.
- **Self-healing, on-or-after.** Due when today is on or after the resolved date
  within the current month (so a late run catches up), and no occurrence exists
  for this month's marker. Mirrors `weekly`'s within-week catch-up, scoped to the
  month. A fully missed month is not backfilled.
- **Open-task guard.** An open task for the key blocks a second (never two).
- **`week: last`** resolves to the last matching weekday of the month (4th or
  5th depending on the month), so "last Friday" always lands.
- Optional `lead_days`? Lean simple: on-or-after the day, no lead, matching
  `weekly`. Revisit only if a use case appears.
- `Rule::validate` rejects: missing/invalid `day_of_week`, missing/invalid
  `week` (must be 1-4 or `last`).

## Design notes

Add `src/due.rs::nth_weekday_occurrence(year, month, weekday, week) -> Option<NaiveDate>`
(pure, `None` when e.g. a 5th occurrence does not exist and `week` is numeric)
and `nth_weekday_due(today, weekday, week) -> Option<String>` returning the
`YYYY-MM` discriminator. Reuse `parse_weekday`. Extend `marker_belongs`
(`is_year_month` already fits) and the `decide` arm (open-task guard + marker
check, like `weekly`). Coordinate with #033 (extraneous-field validation): the
new `week` field joins the ownership map.

## Tasks

- [ ] `src/rule.rs`: `Trigger::NthWeekday`, `week` field (int or `last`),
      parsing, validation, tests
- [ ] `src/due.rs`: `nth_weekday_occurrence` + `nth_weekday_due`, tests
      (incl. 5th-week non-existence and `last` resolving to 4th vs 5th)
- [ ] `src/lib.rs`: `decide` arm + fixtures + generate/error-path tests
- [ ] `src/task.rs`: extend test `rule()` initialiser
- [ ] Docs: `recurring.example.yml`, README, CLAUDE.md, CHANGELOG,
      `karamd-recurring` skill
- [ ] `cargo fmt`/`clippy`/`test` and 100% line coverage

## Acceptance Criteria

- A `nth_weekday` rule with `day_of_week: mon`, `week: 1` generates exactly one
  task on/after the first Monday each month, idempotent across reruns and early
  completion
- `week: last` always fires on the final matching weekday
- Open task blocks a second; late run catches up; missed month not backfilled
- Malformed rules fail loudly in `validate_all`
- CI gates green
