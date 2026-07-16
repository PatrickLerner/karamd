---
id: '056'
title: 'web ui: move the AI-runnable toggle into the tags row as an inline +ai-runnable affordance'
status: completed
created_at: 2026-07-15
priority: medium
type: feature
tags:
- web
completed_at: 2026-07-15
---

## Description

The `ai-runnable` toggle is a standalone button in the detail actions row. Move
it into the tags row (the `tags` frontmatter Field) as an inline, tag-like
toggle. When the task is not runnable, show a clickable `+ai-runnable` text to
add it; when it is, show it as an active tag. The affordance shows even when the
task has no other tags (so the tags row always renders).

## Design

- Frontend only; reuses the existing `toggleRunnable` / `patchTask({ tags })`
  path. No backend.
- Remove the `.toggle` button from the actions row (Detail.tsx).
- The `tags` Field always renders: list the task's other tags (excluding
  `ai-runnable`, which the toggle owns) followed by an inline toggle button
  labelled `+ai-runnable` when off and `ai-runnable` (active) when on. Keep the
  existing tooltip/`aria-pressed` and disable-while-busy behaviour.
- Style the toggle as inline text (link/pill voice), not an actions button.

## Acceptance Criteria

- [ ] The AI-runnable toggle no longer appears in the actions row.
- [ ] The tags row shows `+ai-runnable` inline even when the task has no tags.
- [ ] Clicking it adds/removes the `ai-runnable` tag (one click, no edit view)
      and the label/state updates.
- [ ] `ai-runnable` is not duplicated in the plain tag list.
- [ ] Frontend builds clean (bun typecheck); Rust gates unaffected.
