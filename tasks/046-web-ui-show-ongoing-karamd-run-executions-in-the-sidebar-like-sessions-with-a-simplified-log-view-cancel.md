---
id: '046'
title: 'web-ui: show ongoing karamd run executions in the sidebar (like sessions) with a simplified log view + cancel'
status: completed
created_at: 2026-07-10
priority: medium
type: improvement
tags:
- run
- web-ui
dependencies:
- '039'
- '044'
- '045'
completed_at: 2026-07-10
---

## Context

The web sidebar already lists "Run with Claude" terminal sessions
(`SessionInfo { id, title, running, exit_code }`). Autonomous `karamd run` (#039)
executions have no web presence — you only learn a task is being run by reading
its `ai_status: running` frontmatter (which isn't even in the API yet, see #044).

## Request

Surface ongoing automated ai-task executions in the sidebar the same way sessions
are shown, whenever one is running. Each sidebar entry should have:

- a **simplified live log view** (tail of the run's captured output), and
- a **cancel** button that stops the running agent.

## Notes / implementation

- "Ongoing" = a task with `ai_status: running` and a fresh `ai_run_started`
  (karamd's `is_locked` predicate, src/run.rs). The sidebar lists that set.
- The run must be observable + cancellable from the web server. Today
  `karamd run` is a separate CLI/timer process (on dastyar: a systemd oneshot),
  so `karamd web` doesn't own it — it can't stream its output or kill it. Options:
  - let `karamd web` launch/own `karamd run` itself (like it owns the
    Run-with-Claude PTY sessions), giving it the pid + output stream + cancel; or
  - a shared run-state + per-run log dir (see #045) the web reads for the log
    view, plus a cancel mechanism (cancel flag / signal the pid).
- Cancel must play nice with attempt bookkeeping: a cancelled run should clear the
  `running` marker without necessarily counting as a failed attempt (or a distinct
  "cancelled" state), not silently burn toward `max_attempts`.
- Pairs with #045 (per-run logs back the log view) and #044 (run state in the API).
- Pointers: web sessions plumbing (`SessionInfo`, `GET /api/tasks/{id}/run`
  WebSocket, `src/web_terminal.rs`), `src/run.rs` / `src/run_spawn.rs`.
