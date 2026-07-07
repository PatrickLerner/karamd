---
id: '034'
title: 'interval multiplier: every-N-weeks / every-N-months (biweekly, quarterly)'
status: completed
created_at: 2026-07-07
priority: medium
type: feature
tags:
- generator
completed_at: 2026-07-07
---

## Objective

Express "every N periods": biweekly (every 2 ISO weeks), every-3-weeks, every
2 months, quarterly. Today `weekly` is strictly once per ISO week and `monthly`
once per calendar month; there is no multiplier. This is the most common thing
the current triggers cannot say.

## Rule format

Add an optional `interval` (positive int, default 1) to `weekly` and `monthly`:

```yaml
- key: sprint-retro
  title: "Sprint retro"
  trigger: weekly
  day_of_week: fri
  interval: 2           # every 2nd ISO week
  anchor: 2026-07-10    # a date on the desired cadence (see below)
```

## The anchoring problem (design decision needed)

"Every 2 weeks" is ambiguous without a reference: which weeks are "on"? Options:

- **A. Explicit `anchor` date (recommended).** Fire in a period iff
  `(period_index(today) - period_index(anchor)) % interval == 0`, where
  `period_index` is the ISO-week ordinal (weekly) or the month ordinal
  `year*12+month` (monthly). Fully deterministic, user-controllable, stateless.
  If `anchor` is omitted, default to a fixed epoch (e.g. ISO week of
  1970-01-01 / month 0) so behaviour is still deterministic, and document it.
- **B. Parity on the period number** (even/odd ISO week). Simpler, no anchor,
  but not user-controllable and surprising across year boundaries (W53 -> W01).

Lean A. `interval: 1` (default) must be a no-op so all existing rules are
unaffected. Validate `interval >= 1` and, if `anchor` is present, that it is a
valid `YYYY-MM-DD`.

## Semantics

- Dedup markers unchanged (`key:YYYY-Www` weekly, `key:YYYY-MM` monthly): the
  interval only gates *which* periods are eligible, not the marker shape, so
  idempotency and the open-task guard are untouched.
- A period that is not on-cadence is simply never due; catch-up within an
  on-cadence period still works.

## Design notes

Thread `interval`/`anchor` through `due::weekly_due` and `due::monthly_due`
(or wrap them). Coordinate with #033 (extraneous-field validation): `interval`
and `anchor` are shared-ish but only meaningful for weekly/monthly (and later
`nth_weekday` from #034) - decide their ownership rows. Keep `due.rs` functions
pure and fully covered.

## Tasks

- [ ] `src/rule.rs`: `interval` + `anchor` fields, validation (`>=1`, valid
      anchor date), tests
- [ ] `src/due.rs`: period-index + modular cadence check for weekly and
      monthly; `interval: 1` is a no-op; tests incl. anchor and year-boundary
- [ ] `src/lib.rs`: pass interval/anchor into `decide`; generate tests for
      biweekly on/off weeks
- [ ] `src/task.rs`: extend test `rule()` initialiser
- [ ] Docs: `recurring.example.yml`, README, CLAUDE.md, CHANGELOG,
      `karamd-recurring` skill (document the chosen anchoring)
- [ ] `cargo fmt`/`clippy`/`test` and 100% line coverage

## Acceptance Criteria

- `interval: 2` on a weekly rule fires every other ISO week aligned to `anchor`,
  and never on the off weeks
- `interval: 1` / omitted leaves every existing rule's behaviour identical
- Anchoring is documented and deterministic when `anchor` is omitted
- Malformed rules (`interval: 0`, bad `anchor`) fail loudly
- CI gates green
