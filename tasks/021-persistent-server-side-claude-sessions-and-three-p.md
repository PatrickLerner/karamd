---
title: "Persistent server-side Claude sessions and three-pane web UI"
id: "021"
status: completed
priority: medium
type: feature
tags: []
created_at: "2026-07-03"
completed_at: 2026-07-03
---

# Persistent server-side Claude sessions and three-pane web UI

## Objective

Make Claude runs first-class, detachable sessions and restructure the web UI
around phase views instead of status filters.

## Tasks

- [x] Server-side `SessionRegistry` (one session per task): PTY + child kept
      alive independent of any socket; output always drained into a capped
      scrollback ring (`terminal::Scrollback`, unit-tested).
- [x] Attach/detach over WebSocket: replay scrollback then live-stream via a
      broadcast channel; detach no longer kills the child.
- [x] Explicit kill only: `DELETE /api/sessions/{id}` and registry `Drop`
      (server shutdown). No kill-on-disconnect.
- [x] `GET /api/sessions` + sidebar Sessions section (live/exited dot, × kill).
- [x] Three-pane layout (nav | list | detail) with mobile drawer; Settings
      pinned to sidebar bottom; date in the top bar.
- [x] Nested, reload-safe URLs: `#/view/<tab>/task/<id>[/edit|/run]`.
- [x] Phase-based tabs (Today = ongoing+this-week, Next week, Later, Done)
      with per-phase headings retained inside a tab.

## Acceptance Criteria

- Closing the browser tab leaves a running `claude` alive; reopening the
  session replays output and reconnects to the live stream.
- Killing from the sidebar terminates the child; the row shows exited then
  disappears once removed.
- Reload restores the exact tab and open task.
- Rust suite green, clippy clean, 100% line coverage with `web_terminal.rs`
  excluded (PTY/registry glue; pure logic covered in `terminal.rs`).
