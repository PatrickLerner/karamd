---
id: '047'
title: 'web-ui: pick which AI tool the embedded terminal launches (dropdown over configured agents), not just claude'
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

The web "Run with Claude" embedded terminal (#010/#021) spawns a single command
from `--run-command` / `KARAMD_RUN_COMMAND` (default `claude`; see
`src/web_terminal.rs`, `src/terminal.rs`, and the `Run { run_command }` CLI arg in
`src/lib.rs`). It's hardcoded to one tool. Meanwhile `run.agents` (#039) already
names multiple agent commands (e.g. `claude`, `opencode`) for `karamd run`.

## Request

- Let the embedded terminal launch **opencode** (or any configured tool), not just
  claude.
- When multiple tools are configured, show a **dropdown** on the task detail to
  pick which one; the "Run with X" button reflects the choice.
- Config should be able to list all the AI tools you want to launch.

## Proposed

- Reuse `run.agents` as the single source of launchable tools (so the CLI `karamd
  run` and the terminal launcher agree), or a dedicated web launcher list — prefer
  reusing `run.agents`.
- Button becomes "Run with <name>". If >1 agent: a dropdown/select beside it. If
  exactly 1: just the button. If 0 configured: fall back to `--run-command` /
  default `claude` (back-compat).
- The chosen agent's argv drives the PTY spawn, seeded with the task prompt as
  today. The terminal is interactive (a human drives it), so unlike `karamd run` it
  doesn't need `prompt_via` / auto-complete — just spawn + seed.
- Expose the launchable set via the API (GET /api/config already returns config;
  add the agent names/commands) so the frontend can render the dropdown.

## Notes

- Pointers: `src/web_terminal.rs`, `src/terminal.rs`, `Run { run_command }` in
  `src/lib.rs`, web `Detail.tsx` ("Run with Claude" button), `GET /api/config`.
- Relates to #039 (run.agents) and #046 (sidebar for headless runs). Found via the
  dastyar zeroclaw integration, where the box runs opencode rather than claude.
