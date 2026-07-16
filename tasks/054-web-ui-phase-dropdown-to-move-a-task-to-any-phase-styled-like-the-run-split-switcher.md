---
id: '054'
title: 'web ui: phase dropdown to move a task to any phase, styled like the run-split switcher'
status: completed
created_at: 2026-07-15
priority: medium
type: feature
tags:
- web
completed_at: 2026-07-15
---

## Description

#052 added one-click phase **buttons** on the Detail view, but only when a task
has *no* phase (`canAssignPhase` requires `task.phase === null`, Detail.tsx). To
change the phase of an already-phased task you still have to open Edit, pick the
phase, and save.

Preferred fix: a **phase dropdown** on the task actions row that lets you move a
task to any configured phase in one interaction, without the edit round-trip. It
should work for phased *and* unphased tasks.

Model the control on the existing split-button run switcher (`run-split` /
`run-split-toggle` / `run-split-menu` in Detail.tsx + styles.css): a compact
action button showing the current phase, with a caret that opens a small menu of
the configured phases, the current one check-marked. This is the same task-action
dropdown pattern Patrick called out as working well.

## Design

- Frontend only. `PATCH /api/tasks/{id}` already accepts `phase`
  (`patchTask` in api.ts, `PatchBody.phase` in src/web.rs) and `phaseMutation`
  already exists in Detail.tsx. No backend change.
- Replace the `phase-assign` button group (Detail.tsx ~331-349) with a
  split/menu control reusing the `run-split` markup + a11y pattern
  (`aria-haspopup="menu"`, `aria-expanded`, `role="menuitemradio"`,
  `aria-checked`, outside-click + Escape close, mirroring the `agentMenuOpen`
  handling).
- Show for any non-terminal task with >=1 configured phase (drop the
  `task.phase === null` gate). Label the trigger with the current phase name, or
  "No phase" when unset. Menu lists all `assignablePhases`; the active phase is
  check-marked and selecting it is a no-op / disabled.
- On select: `phaseMutation.mutate(phaseId)`, invalidate the `tasks`/`next`
  queries, close the menu, disable while in flight, surface failure via the
  existing error path.
- Keep the unphased quick-triage affordance usable (the dropdown covers it; a
  first-run empty-phase state should still be one obvious click).

## Acceptance Criteria

- [ ] A non-terminal task shows a phase dropdown in the actions row regardless of
      whether it currently has a phase.
- [ ] The trigger shows the current phase (or "No phase"); the menu lists all
      configured phases with the active one check-marked.
- [ ] Selecting a phase PATCHes it in one interaction (no edit view) and the UI
      reflects the new phase; selecting the current phase does nothing.
- [ ] Keyboard + outside-click behaviour matches the run-split menu; buttons
      don't trigger the row/detail navigation.
- [ ] Frontend builds clean (bun typecheck/lint); Rust gates unaffected.
