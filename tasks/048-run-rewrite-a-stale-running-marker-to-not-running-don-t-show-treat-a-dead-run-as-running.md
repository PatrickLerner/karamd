---
id: '048'
title: 'run: rewrite a stale running marker to not-running (don''t show/treat a dead run as running)'
status: completed
created_at: 2026-07-10
priority: medium
type: improvement
tags:
- run
- reliability
dependencies:
- '039'
completed_at: 2026-07-10
---

## Context

While a task runs, `karamd run` sets `ai_status: running` + `ai_run_started`. For
*selection*, karamd re-considers a task once the marker is stale
(`now - ai_run_started >= 2 * timeout_secs`, src/run.rs `is_stale`/`is_locked`).
Keep that rule — the 2× re-selection is the right behaviour.

## Problem

That staleness only gates *selection* during a `karamd run` invocation; karamd
never actively *clears* the marker. So after an unclean kill (SIGKILL, reboot
mid-run) the frontmatter keeps `ai_status: running` indefinitely, and the file,
the API (#044), and the web sidebar (#046) show a long-dead run as "running"
until the task happens to be re-run to completion.

## Request

When a `running` marker is stale (by the existing `2 * timeout_secs` rule), karamd
should **rewrite it to not-running** — clear `ai_status` / `ai_run_started`, revert
to pending/selectable — so persisted state and the UI reflect reality rather than a
ghost run. Keep the 2× threshold; no separate hard cap needed.

## Notes

- Don't count the cleanup as an attempt (`ai_attempts` was already incremented
  before the spawn).
- Ideally the read/API path also *treats* a stale `running` marker as not-running
  for display, so the web UI self-corrects between runs without waiting for the
  next `karamd run` to rewrite the file.
- Pointers: src/run.rs (`is_stale`/`is_locked`, marker read/clear on outcome), and
  the API/TaskView read path for the display-side treatment.
- Pairs with #044 (run state in API/UI) and #046 (sidebar) so neither shows a ghost.
