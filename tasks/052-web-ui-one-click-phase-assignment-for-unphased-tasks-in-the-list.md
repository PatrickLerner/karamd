---
id: '052'
title: 'web ui: one-click phase assignment for unphased tasks in the list'
status: completed
created_at: 2026-07-15
priority: medium
type: feature
tags:
- web
completed_at: 2026-07-15
---

## Description

Tasks with no phase surface in the Today tab (`taskInTab` puts `phase === null`
open tasks there). Assigning one of them to a phase (now / next / later / …)
currently means: open the task, open edit, pick the phase in a dropdown, save.
Too many clicks for triage.

Add a **one-click quick-assign**: on an unphased task row in the list, show a
small set of phase buttons (one per configured phase). Clicking one PATCHes the
task's `phase` and the row moves to that phase's tab. No edit round-trip.

## Design

- Frontend only: the `PATCH /api/tasks/{id}` endpoint already accepts `phase`
  (`web/src/api.ts` `patchTask`, `src/web.rs` `PatchBody.phase`). No backend change.
- In `web/src/views/List.tsx`, render quick-assign controls only for rows whose
  `task.phase === null` (and not terminal). Offer the configured phases
  (`config.phases`, excluding the null "no phase" entry), labelled by name.
- Restructure `TaskRow` so the buttons are not nested inside the row `<a>`
  (invalid HTML): make the row a flex container holding the task link plus a
  `phase-quick` button group. Preserve the current row look (id / title / chips,
  ledger rule, hover).
- On click: `api.patchTask(id, { phase })`, then invalidate the `tasks` (and
  `next`) query so the list regroups. Disable the buttons while the mutation is
  in flight; surface a failure via the existing error banner path.

## Acceptance Criteria

- [ ] An unphased, non-terminal task in the list shows a button per configured
      phase.
- [ ] Clicking a phase button assigns that phase in one click (no edit view) and
      the task regroups/moves out of Today.
- [ ] Phased tasks show no quick-assign controls.
- [ ] The row link to the task detail still works; buttons don't trigger it.
- [ ] Frontend builds clean (bun typecheck/lint); Rust gates unaffected.
