---
id: '032'
title: Reject extraneous trigger fields in rule validation
status: completed
created_at: 2026-07-07
priority: medium
type: improvement
tags:
- generator
completed_at: 2026-07-07
---

## Objective

`Rule::validate` today only rejects *missing* required fields for a trigger. It
does not reject *extraneous* ones: `trigger: weekly` with `day_of_month: 12`
parses cleanly and silently ignores `day_of_month`. That contradicts the
"fail loudly on a rules-file typo" contract. Reject any field that belongs to a
different trigger.

## Design

The `Rule` struct is flat: each trigger owns a subset of the optional fields
(`every_days`, `annual`, `day_of_month`, `day_of_week`, `lead_days`). After the
existing required-field checks, assert that fields *not* owned by the active
trigger are `None`.

Ownership map:

- `after_completion`: `every_days`
- `calendar`: `annual`, `lead_days`
- `monthly`: `day_of_month`, `lead_days`
- `weekly`: `day_of_week`

`body`, `phase`, `priority`, `tags` are shared and always allowed. Emit a
specific error naming the offending field and the trigger, e.g.
``rule `k`: `day_of_month` is not valid for a weekly trigger``.

Consider a small table-driven helper so adding a trigger later is one row, not a
new hand-written block (keeps this from rotting as triggers grow, per the
scaling note in #031).

## Tasks

- [ ] `src/rule.rs`: extend `Rule::validate` to reject fields owned by other
      triggers; one clear error per stray field
- [ ] Tests: each trigger rejects each foreign field; valid rules with only
      their own fields still pass
- [ ] Docs: note the stricter validation in the `karamd-recurring` skill and
      README/CHANGELOG
- [ ] `cargo fmt`/`clippy`/`test` and 100% line coverage

## Acceptance Criteria

- A rule mixing a trigger with a foreign field fails `validate_all` with a
  message naming the field and trigger
- All existing valid rules still validate
- CI gates green
