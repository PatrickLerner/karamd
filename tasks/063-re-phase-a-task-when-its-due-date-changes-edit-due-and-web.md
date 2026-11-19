---
id: '063'
title: Re-phase a task when its due date changes (edit --due and web)
status: completed
created_at: 2026-11-19
priority: medium
completed_at: 2026-11-19
---

# Objective

Changing a task's `due` date should move its phase at once, by the reschedule rules. Today only `create --due` and `postpone` do this. `edit --due` and the web UI due edit leave the old phase until the next `karamd reschedule` run (15 min timer on dastyar), so the UI shows a wrong bucket in between.

# Tasks

- [x] `edit --due <date>` without `--phase`: set phase via `reschedule::phase_for_due`, same as `create`
- [x] Explicit `--phase` in the same call wins
- [x] Web: the due-date edit endpoint applies the same rule and returns the new phase, so the list and detail pane update without reload
- [x] Clearing `due` (`--due ""`) keeps the current phase
- [x] No matching rule or no reschedule config: phase unchanged
- [x] Tests for each case; update README, CHANGELOG and the karamd-cli skill

# Acceptance Criteria

- Editing a due date in CLI or web moves the task to the phase `karamd reschedule` would pick, in the same write
- Undated tasks and explicit phase edits behave as before
