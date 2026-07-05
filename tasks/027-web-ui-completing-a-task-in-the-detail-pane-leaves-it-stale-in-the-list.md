---
id: '027'
title: 'Web UI: completing a task in the detail pane leaves it stale in the list'
status: completed
created_at: 2026-07-05
type: bug
tags:
- web
- ui
completed_at: 2026-07-05
---

## Steps to Reproduce

1. Open `karamd web`, open a task in the detail pane, click **Complete**.
2. The detail pane updates, but the task stays in the list and the sidebar
   counts are unchanged until a manual navigation/reload.

## Cause

`main.tsx` held the task list in local state and refetched it only when the
route (`routeKey`) changed. An in-place status change in the detail pane does
not change the URL, so the shared list never refetched. Edit/Create hid the bug
because they `navigate()` afterward.

## Fix

Adopt TanStack Query for server state:

- `main.tsx` wraps the app in `QueryClientProvider`; `config`/`tasks`/`next`/
  `sessions` become `useQuery` (sessions via `refetchInterval`).
- `Detail` reads the task via `useQuery` and changes status via `useMutation`
  whose `onSuccess` invalidates `["tasks"]` and `["next"]` (and writes back
  `["task", id]`), so a completed task drops out of the list immediately.
- `TaskForm` create/patch become a `useMutation` with the same invalidation.
- Rules keeps its local-first editing model (its save does not change the task
  list, so no invalidation needed).

## Acceptance Criteria

- Completing/cancelling a task in the detail pane removes it from the open list
  and updates the sidebar counts with no manual reload
- `bun run typecheck` and `bun run build` pass; lockfile updated for CI
