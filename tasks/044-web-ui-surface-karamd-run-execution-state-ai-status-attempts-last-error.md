---
id: '044'
title: 'web-ui: surface karamd run execution state (ai_status/attempts/last_error)'
status: completed
created_at: 2026-07-10
priority: medium
type: improvement
tags:
- run
- web-ui
dependencies:
- '039'
completed_at: 2026-07-10
---

## Context

`karamd run` (#039) tracks per-task execution state in frontmatter: `ai_status`
(running/failed), `ai_attempts`, `ai_run_started`, `ai_last_error`, plus the
`ai-failed` tag when a task is parked at `max_attempts`.

## Problem

The web UI shows the `ai-runnable` tag but none of that execution state. While a
task is actively running (`ai_status: running`) it looks identical to an idle
task in both the list and the detail pane; a parked task is only distinguishable
by the raw `ai-failed` tag chip; `ai_attempts` (n/max) and `ai_last_error` are
invisible. So from the dashboard you cannot tell whether an ai-runnable task is
idle, running now, agent-completed, or failed/parked — and crucially you cannot
see how many times it has already been attempted.

Root cause: `TaskView` (src/output.rs) does not serialize the `ai_*` markers, so
`/api/tasks` and `/api/tasks/{id}` never carry them and the frontend has no data
to render. Verified on 0.8.0 — TaskView has no ai_status/ai_attempts/
ai_run_started/ai_last_error field. (Only `ai-failed` leaks through via `tags`.)

## Expected

- API: `TaskView` exposes the run markers (skip-if-none): `ai_status`,
  `ai_attempts`, `ai_run_started`, `ai_last_error`.
- **Attempt count must be shown cleanly and always** — on ANY task that has been
  run, not just while it is running, and visible at a glance (e.g. "2/3 attempts"
  on the card). It should not require opening the raw file.
- Web list: a status chip on ai-runnable tasks, e.g. "running" (+ started),
  "n/max attempts", "failed/parked".
- Web detail: a run-state block (status, attempts n/max, started, last error).

## Repro

1. Tag a task `ai-runnable`, `karamd run` it (or let the timer pick it up).
2. While `ai_status: running`, open the web UI: the task appears (correctly,
   under Today / NO PHASE) but with no indication it is currently executing, and
   after failures there is no visible attempt count.

## Notes

- Pointers: `src/output.rs` (TaskView), web `web/src/types.ts` (Task),
  `web/src/views/List.tsx` + `Detail.tsx`.
- `SessionInfo.running` in the web types is the "Run with Claude" terminal
  session, unrelated to `karamd run`.
- Found running karamd 0.8.0 on the dastyar box via the zeroclaw integration.
