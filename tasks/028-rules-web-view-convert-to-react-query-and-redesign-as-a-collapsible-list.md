---
id: '028'
title: 'Rules web view: convert to react-query and redesign as a collapsible list'
status: completed
created_at: 2026-07-05
type: improvement
tags:
- web
- ui
completed_at: 2026-07-05
---

## Motivation

The Rules view rendered every rule as a fully-expanded editable card, so a vault
with more than a couple of rules was a long scroll of forms. It also fetched and
saved rules with hand-rolled effects while the rest of the SPA moved to
TanStack Query.

## Description

- Redesign as an accordion: each rule is a compact, clickable row (trigger, key,
  title, schedule summary); clicking expands the editable fields. One row open at
  a time; a newly added rule opens automatically for editing.
- Convert the data layer to react-query: `useQuery(["rules"])` seeds a local
  editable draft; Save/Preview are `useMutation`s. Save writes back the
  `["rules"]` cache. Saving rules does not change the task list, so no
  `["tasks"]` invalidation.

## Acceptance Criteria

- Rules render as compact rows; only the clicked rule shows its form
- Add/Remove/Preview/Save still work; Save persists and shows "Saved."
- `bun run typecheck` and `bun run build` pass
