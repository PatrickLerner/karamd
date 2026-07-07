---
id: '035'
title: 'Web Rules editor: support weekly + nth_weekday + interval fields'
status: completed
created_at: 2026-07-07
priority: medium
type: feature
tags:
- web
- generator
dependencies:
- '033'
- '034'
completed_at: 2026-07-07
---

## Objective

The web Rules editor (`web/src/views/Rules.tsx`) only understands the three
original triggers. It has no input for `day_of_week` (shipped in #031), nor for
the `nth_weekday`/`week` (#034) or `interval`/`anchor` (#035-interval) fields.
So a weekly rule cannot be created or edited in the UI, its summary line does
not render, and `emptyRule`/client-side validation ignore the new fields. Bring
the web editor to parity with the backend rule format.

## Scope

Cover every trigger and field the backend now supports:

- `weekly`: `day_of_week` select (mon..sun)
- `nth_weekday`: `day_of_week` + `week` (1..4 / last)
- `interval` + `anchor` on the triggers that accept them

## Tasks

- [ ] `web/src/types.ts`: extend the `Rule` type with `day_of_week`, `week`,
      `interval`, `anchor` and the new trigger literals
- [ ] `web/src/views/Rules.tsx`: trigger dropdown includes the new triggers;
      per-trigger field inputs; summary line renders each; `emptyRule` seeds
      sensible defaults
- [ ] `web/mock.ts`: mock validation accepts/rejects the new triggers to match
      the server (`generate_from_rules` dry-run via the API)
- [ ] Verify end to end against the real API (`karamd web`): create a weekly and
      an nth_weekday rule, save, confirm the generated file is spec-valid
- [ ] `bun run build` clean; no type errors

## Acceptance Criteria

- Every backend trigger is creatable and editable in the web Rules view
- The summary line renders correctly for each trigger
- Saving a rule round-trips through the API and produces a spec-valid task on
  generate
- Frontend build is clean

## Notes

Depends on #034 and the interval task so the web editor covers all new fields in
one pass instead of lagging each format change. Weekly (#031) support is the
baseline and can land even if the others slip.
