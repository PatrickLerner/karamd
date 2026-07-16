---
id: '055'
title: 'web ui: combine status transition actions into a dropdown, like the phase/run switchers'
status: completed
created_at: 2026-07-15
priority: medium
type: feature
tags:
- web
completed_at: 2026-07-15
---

## Description

The detail actions row renders each status transition as its own inline button
(`transitions(status, workflow)` in Detail.tsx: for a pending task that is
Start / Complete / Cancel). With the phase (#054) and run (#047) dropdowns the
row is crowded. Combine the status transitions into one dropdown styled like
those switchers.

Requested order for the pending state: **Complete, Start, Cancel**.

## Design

- Frontend only; reuses the existing `apply(to)` status mutation. No backend.
- Add a `status-split` dropdown mirroring the phase-split control: a trigger
  button labelled with the current status and a caret, opening a menu of the
  available transitions. Menu items are actions (they change status), so use
  `role="menu"` / `role="menuitem"` (no radio/check state), reusing the
  `run-split-menu` look.
- Reorder the `pending` case of `transitions()` to Complete, Start, Cancel.
- When a status has only one transition (completed/cancelled -> Reopen), render
  it as a plain button rather than a one-item dropdown (mirrors run-split only
  showing its caret when there is more than one agent).
- Outside-click + Escape close, disabled while a mutation is in flight, closes
  on success; same a11y wiring as the phase/agent menus.

## Acceptance Criteria

- [ ] A task with 2+ transitions shows a single status dropdown instead of
      separate transition buttons.
- [ ] For a pending task the menu order is Complete, Start, Cancel.
- [ ] Selecting an item performs the transition and the UI reflects the new
      status; the menu closes.
- [ ] A single-transition status (Reopen) renders as a plain button.
- [ ] Keyboard + outside-click behaviour matches the phase/run menus.
- [ ] Frontend builds clean (bun typecheck); Rust gates unaffected.
