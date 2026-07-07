---
id: '031'
title: 'Weekly trigger: recur on a fixed day of the week (e.g. every Friday)'
status: completed
created_at: 2026-07-07
priority: medium
type: feature
tags:
- generator
completed_at: 2026-07-07
---

## Objective

Add a fourth trigger kind, `weekly`: the task recurs once per ISO week, pinned to
a fixed weekday. Motivating case: a weekly LinkedIn review I always want to land
on a **Friday**. None of the existing triggers express this:

- `after_completion` (`every_days: 7`) drifts off the weekday — it counts 7 days
  from *completion*, so a late completion pushes every future occurrence off
  Friday.
- `calendar` is once per year; `monthly` is once per month.

Rule format:

```yaml
- key: linkedin-weekly
  title: "Evaluate and schedule LinkedIn posts"
  trigger: weekly
  day_of_week: fri   # mon|tue|wed|thu|fri|sat|sun
```

## Semantics

- **Once per ISO week.** Dedup marker `key:YYYY-Www` (e.g. `linkedin-weekly:2026-W28`),
  mirroring `monthly`'s `key:YYYY-MM`. Completing early in the week must not
  re-trigger the same week.
- **Open-task guard.** If an open (non-terminal) task already exists for the key,
  do NOT create a second one — same as `after_completion` already enforces. This
  is the explicit requirement: always want one on Friday, never two.
- **Self-healing.** Due when today is on or after `day_of_week` within the current
  ISO week and no occurrence exists for this week's marker. So if `generate` does
  not run exactly on Friday, it still fires Sat/Sun to catch up, then resets next
  week. A fully missed week is not backfilled.
- `Rule::validate` rejects: missing/invalid `day_of_week`.

## Open questions

- Accept both string weekdays (`fri`) and integers? Pick one canonical form,
  document it, reject the rest.
- Optional `lead_days` to surface it a day early, or keep weekly strictly
  on-the-day (simpler)? Lean simple unless a use case appears.

## Tasks

- [ ] `src/rule.rs`: `Trigger::Weekly`, `day_of_week` field, parsing, validation, tests
- [ ] `src/due.rs`: `weekly_due` returning the `YYYY-Www` discriminator, ISO-week math, tests
- [ ] `src/lib.rs`: `marker_belongs` (is-iso-week helper), `decide` arm, fixtures, generate + error-path tests
- [ ] `src/task.rs`: extend test `rule()` initialiser with `day_of_week: None`
- [ ] Docs: `recurring.example.yml`, README trigger section, CLAUDE.md, CHANGELOG
- [ ] `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features`, `cargo test`
- [ ] `cargo llvm-cov --ignore-filename-regex 'src/main.rs'` back at 100% lines

## Acceptance Criteria

- A `weekly` rule with `day_of_week: fri` generates exactly one task per ISO week,
  on Friday, idempotent across reruns and early completion
- An already-open task for the key blocks a second creation
- If generate runs a day or two late, the week's task still appears; a fully
  missed week is not backfilled
- Malformed rules fail loudly in `validate_all`, never silently skip
- All four CI gates green (fmt, clippy, test, 100% line coverage)
