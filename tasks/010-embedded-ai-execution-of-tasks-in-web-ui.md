---
title: "Embedded AI execution of tasks in the web UI"
id: "010"
status: in-progress
priority: low
type: feature
tags: ["work", "web", "ai"]
dependencies: ["009"]
created_at: "2026-07-02"
---

# Embedded AI execution of tasks in the web UI

## Objective

From the web UI (#009), launch Claude in a session to execute a task, with the
session embedded directly in the web view (not a separate terminal window). Tech
is undecided; this task is to prototype and pick it.

## Description

Depends on #009. The framework choice there (async + WebSocket-capable
axum/tokio) exists specifically to make this feasible without re-platforming.

Likely shape (to validate, not yet committed):

- **PTY:** spawn `claude` in a pseudo-terminal (`portable-pty`), one child per
  session, working dir = the vault/project. Stream stdout/stdin over a
  **WebSocket** to the browser.
- **Terminal in the browser:** render with `xterm.js` (+ fit addon) as a normal
  dependency in the #009 React SPA (bun), bundled into the embedded build. A
  React `<Terminal>` component wraps it. Evaluate whether a lighter streamed log
  (append-only, no full emulator) is enough for v1.
- **Session lifecycle:** start from a task's detail page ("run with Claude"),
  attach/detach, show status, terminate. Decide session persistence across page
  reloads (reconnect to the running PTY vs. one-shot).
- **Safety:** single user, explicit launch, reached over Tailscale (tailnet is
  the security boundary; see #009). Consider how Claude's own permission prompts
  surface through the embedded terminal.
- **Mobile:** the session is used from a phone over Tailscale (#009 is
  mobile-first). xterm.js on touch devices is awkward (soft keyboard, no real
  keys); this is a strong reason to weigh a lighter streamed-log view plus
  tap-able approve/deny controls over a full emulator for mobile.

Open questions to resolve in a spike before building:

- Full terminal emulator (xterm.js) vs. structured streamed output?
- How the task context is passed to Claude (prompt seeded from task frontmatter
  + body).
- Whether karamd shells out to the `claude` CLI or uses another integration.

## Tasks

- [x] Spike: PTY (`portable-pty`) + WebSocket streaming of a `claude` session
      (`src/web_terminal.rs`; verified end-to-end with a harmless `--run-command`)
- [x] Decide terminal rendering: xterm.js (vendored) vs. structured stream
      (chose xterm.js, vendored + bundled by bun)
- [x] "Run with Claude" entry point from a task detail page (#009)
- [x] Seed the session with task context (frontmatter + body) via
      `terminal::seed_prompt`, written to the PTY as initial input (not
      auto-submitted)
- [~] Session lifecycle: attach/detach, status, terminate, reconnect policy
      (one child per WS connect; closing the socket ends the child; status shown
      in the view. Reconnect-to-running-PTY is deferred, see below)
- [x] Localhost-only safety review; surface Claude permission prompts sanely
      (loopback bind default; tailnet is the boundary; prompts render in the
      terminal like any other output)
- [x] Document the chosen approach and `karamd web` AI usage

## Chosen approach (implemented)

- Backend: `GET /api/tasks/{id}/run` upgrades to a WebSocket; `portable-pty`
  spawns `--run-command` (default `claude`, cwd = vault). Protocol: binary
  frames = PTY bytes both ways; a text `{"type":"resize",...}` from the client
  resizes; a final text `{"type":"exit","code":N}` on child exit. The seed
  prompt is written to the PTY as the initial (un-submitted) input.
- Rendering: xterm.js + fit addon in a React `<Terminal>` view, bundled by bun
  into the SPA (no CDN). Solarized Light theme.
- The blocking PTY IO is bridged to the async socket in `src/web_terminal.rs`,
  which is excluded from the coverage gate (like `src/main.rs`); the pure parts
  (prompt seeding, argv parsing) live in the covered `src/terminal.rs`.

## Deferred follow-ups

- Reconnecting to a still-running PTY after a page reload (sessions are
  currently one per WebSocket connection; a reload starts fresh).
- Mobile ergonomics of xterm.js (soft keyboard) — a lighter streamed-log view
  with tap-able approve/deny was considered and not built.

## Acceptance Criteria

- From a task in the web UI, Claude launches and runs embedded in the view
- Live bidirectional I/O works over the #009 server with no re-platform
- xterm.js bundled via the #009 SPA build; runtime stays a single binary
- Approach and its trade-offs are documented
