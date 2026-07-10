---
id: '043'
title: 'web-ui: button to toggle ai-runnable tag on a task'
status: completed
created_at: 2026-07-10
priority: low
type: improvement
tags:
- run
- web-ui
dependencies:
- '039'
completed_at: 2026-07-10
---

## Description

`karamd run` (#039) executes tasks tagged `ai-runnable`. Today that tag is only
addable via the CLI (`karamd edit <id> --tag ... ai-runnable`) or by hand-editing
frontmatter. The web UI has no way to mark a task AI-executable.

## Proposed work

Add a per-task button in the web UI to toggle the `ai-runnable` tag.

- **Toggle, not append:** `karamd edit --tag` replaces the whole tag set, so the
  action must re-send the full list with `ai-runnable` added/removed — never drop
  the task's other tags.
- **Reflect state:** show the button as on when the task already carries the tag
  (e.g. a robot/lightning icon), off otherwise.
- **Optional hint:** the tag does nothing unless `run.enabled: true` in the vault
  config, so gate or annotate the button when run is disabled.

## Acceptance Criteria

- A task's `ai-runnable` tag can be added and removed from the web UI.
- Toggling preserves all other tags on the task.
- The button reflects the current tagged/untagged state.
