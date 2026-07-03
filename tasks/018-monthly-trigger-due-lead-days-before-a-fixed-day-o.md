---
id: "018"
title: "Monthly trigger: due lead_days before a fixed day of the month"
status: completed
priority: medium
dependencies: []
tags: ["generator"]
created_at: 2026-07-02
completed_at: 2026-07-02
---

# Monthly trigger: due lead_days before a fixed day of the month

## Objective

Add a third trigger kind, `monthly`: the task materialises `lead_days` before a
fixed `day_of_month`, once per month. Use case: a bill or top-up due on the
12th of every month, surfaced a week early. Neither existing trigger covers
this: `calendar` fires once per year, `after_completion` drifts off the fixed
date.

Rule format:

```yaml
- key: monthly-topup
  title: "Top up account"
  trigger: monthly
  day_of_month: 12   # 1-31; 29-31 clamp to the month's last day
  lead_days: 7       # 0-27; 28+ would overlap the previous window (February)
```

Semantics (mirror `calendar`):

- Due when `0 <= (occurrence - today) <= lead_days`, checking this month and
  next month so a window straddling a month boundary resolves forward.
- Dedup marker `key:YYYY-MM` (e.g. `monthly-topup:2026-07`): once per month,
  early completion inside the window must not re-trigger.
- `day_of_month` 29-31 clamps to the month's last day (like the `02-29` leap
  rule for `calendar`), so `31` still fires in 30-day months and February.
- `Rule::validate` rejects: missing `day_of_month`/`lead_days`,
  `day_of_month` outside 1-31, `lead_days` outside 0-27.

## State: partially implemented in the working tree

Uncommitted changes from a prior session already in place — review, keep or
redo, then finish:

- `src/due.rs`: `monthly_occurrence` (clamping), `monthly_due` (returns the
  `YYYY-MM` discriminator), plus unit tests.
- `src/rule.rs`: `Trigger::Monthly`, `day_of_month` field, validation, tests.
- `src/lib.rs`: module doc, `marker_belongs` (via `is_year_month` helper),
  `decide` arm, `MONTHLY` test fixture, marker + generate tests, and
  `day_of_month: None` added to `bare_rule`.

Known missing:

- `src/lib.rs`: `decide_monthly_missing_day_of_month_errors` /
  `decide_monthly_missing_lead_days_errors` tests (edit was not applied).
- `src/task.rs`: test `rule()` initialiser lacks the new `day_of_month` field —
  the test build fails until it is added.
- Docs: `recurring.example.yml` (monthly example), README "Two trigger kinds"
  section, CLAUDE.md ("Two triggers" and design decisions), CHANGELOG entry.

## Tasks

- [ ] Review/finish `due.rs`, `rule.rs`, `lib.rs` changes listed above
- [ ] Fix `src/task.rs` test initialiser (`day_of_month: None`)
- [ ] Add the missing `decide` error-path tests
- [ ] Update `recurring.example.yml`, README, CLAUDE.md, CHANGELOG
- [ ] `cargo fmt --all -- --check`, `cargo clippy --all-targets
      --all-features`, `cargo test`
- [ ] `cargo llvm-cov --ignore-filename-regex 'src/main.rs'` back at 100% lines

## Acceptance Criteria

- A `monthly` rule generates exactly one task per month, `lead_days` before
  `day_of_month`, idempotent across reruns and early completion
- `day_of_month: 31` fires in February and 30-day months (clamped)
- Malformed rules fail loudly in `validate_all`, never silently skip
- All four CI gates green (fmt, clippy, test, 100% line coverage)
