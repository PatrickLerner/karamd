---
id: '057'
title: 'reschedule: move tasks between phases by due date via custom rules (karamd reschedule)'
status: completed
created_at: 2026-07-15
priority: high
type: feature
tags:
- cli
completed_at: 2026-07-15
---

## Description

A new `karamd reschedule` subcommand that moves open tasks between phases based
on their `due` date, driven by a fully custom, ordered rule list. Idempotent and
safe to run unattended on cron, like `generate` (which only ever *adds* files;
this one only ever changes `phase`).

Motivating example:

```yaml
# .taskmd.reschedule.yaml
enabled: true
rules:
  - { due: today,     phase: now }
  - { due: this_week, phase: next }
  - { due: next_week, phase: soon }
```

## Confirmed design decisions

- **Config**: a separate `.taskmd.reschedule.yaml`, resolved to
  `<vault>/.taskmd.reschedule.yaml` by default (const `DEFAULT_RESCHEDULE_CONFIG`),
  overridable with `--config`. Top-level `rules:` list plus optional
  `enabled:` (default true; `enabled: false` pauses without deleting the file).
- **Windows: support both** styles per rule (exactly one matcher per rule):
  - Named `due:` keyword, a membership test on the due date vs `today`:
    - `overdue` = due < today
    - `today` = due == today
    - `this_week` = due in the same ISO week as today
    - `next_week` = due in the ISO week after today's
    - `this_month` = due in the same calendar month/year
    - `next_month` = due in the next calendar month (Dec->Jan rolls the year)
  - Numeric range: `min_days` / `max_days` (either or both), inclusive, over the
    signed offset `(due - today).days`. `max_days: 0` therefore also catches
    overdue; open-ended on the missing side.
- **First match wins**: rules evaluated top to bottom; the first whose window
  contains the task's due date sets the target phase.
- **Authoritative, both directions**: any open (non-terminal) task with a due
  date that matches a rule gets that rule's phase, even if that moves it *later*
  (e.g. now -> soon when the due date is far out). Overrides manual phase.
- **Scope / no-ops**: tasks with no `due`, terminal tasks, and tasks matching no
  rule are left untouched. No write when the task is already in the target phase.
- **Flags**: `--vault`, `--config`, `--dry-run`, `--today YYYY-MM-DD` (test /
  backfill), `--json` / `--yaml`. Mirrors `generate`.

## Layout (match repo conventions)

- `src/reschedule.rs` - pure, unit-tested core: the config model
  (`RescheduleConfig`, `RescheduleRule`, `Window`), `load_reschedule_config`,
  `RescheduleConfig::validate` (phase ids exist in `.taskmd.yaml`; exactly one
  matcher; known keyword; `min_days <= max_days`; non-empty phase), a pure
  `Window::contains(due, today)` and `decide(due, phase, today, &rules) ->
  Option<&phase>`, and `plan(&[Task], &rules, today) -> Vec<Move>`. Every fn
  takes `today: NaiveDate`; no clock access.
- CLI wiring + orchestration in `src/lib.rs` (`Reschedule` subcommand): load both
  configs, scan the `Vault`, compute the plan, apply via `Vault` update
  (re-read-before-mutate, atomic) unless `--dry-run`; build a `RescheduleReport`
  (moved `(id, from, to)`, considered / skipped counts) rendered human/JSON/YAML.
- `.taskmd.reschedule.example.yaml` - format reference (like
  `recurring.example.yml`).
- Update the CLAUDE.md Layout section to list `src/reschedule.rs`.

## Acceptance Criteria

- [ ] `karamd reschedule` moves open, due-dated tasks to the phase dictated by
      the first matching rule; the example config yields today->now,
      this_week->next, next_week->soon.
- [ ] Named windows (overdue/today/this_week/next_week/this_month/next_month) and
      numeric `min_days`/`max_days` ranges both work; first match wins.
- [ ] Authoritative both directions; no write when already in target; tasks with
      no due date / no match / terminal status untouched.
- [ ] `--dry-run` reports moves without writing; `--today` overrides the date;
      `--json`/`--yaml` emit the report.
- [ ] `enabled: false` (or missing file handling) is a clean no-op.
- [ ] `validate` rejects unknown windows, missing/dual matchers, unknown phase
      ids, and `min_days > max_days`.
- [ ] fmt/clippy clean; `cargo test` green; 100% line coverage maintained
      (reschedule.rs + its lib.rs wiring fully covered).
