---
title: Add an open shortcut to list for non-terminal tasks (not completed or cancelled)
id: '026'
status: completed
priority: medium
type: feature
tags:
- work
- core
created_at: 2026-07-05
completed_at: 2026-07-05
---

# Add an open shortcut to list for non-terminal tasks (not completed or cancelled)

## Objective

Give `list` a one-word way to show only *non-terminal* tasks (anything not
`completed` or `cancelled`), so the common "what's still on my plate" view
doesn't require spelling out a boolean expression.

## Motivation

Today that view requires `karamd list "NOT status:completed AND NOT
status:cancelled"` (or an explicit `status:pending OR status:in-progress OR
status:in-review OR status:blocked`). That is the single most common thing you
want to see and it's the most verbose query to type.

## Description

Add `open` as a boolean filter term in the query grammar (`src/query.rs`),
mirroring the existing `ready:true/false` pseudo-field — not a CLI flag:

- `open:true` matches any task whose effective status is not terminal;
  `open:false` matches `completed`/`cancelled`. Reuse `Status::is_terminal()`.
- Composes with `AND`/`OR`/`NOT` like any other term, e.g.
  `karamd list "open:true AND tag:work"`.
- Document it in the grammar doc-comment alongside `ready`.
- The web UI list already orders active work before terminal (`web/src/`);
  consider an "Open" filter chip that sets `open:true`, but the CLI/grammar
  filter is the core deliverable.

## Tasks

- [ ] Add `Open` field to the query grammar + evaluator, backed by
      `Status::is_terminal()` (`src/query.rs`)
- [ ] Update the grammar doc-comment and README/CLAUDE.md `list` docs
- [ ] TDD; keep 100% line coverage

## Acceptance Criteria

- `karamd list "open:true"` returns exactly the non-terminal tasks;
  `open:false` returns only `completed`/`cancelled`
- `open:` composes with `AND`/`OR`/`NOT` like any other term
- fmt, clippy, tests, and the 100%-line coverage gate all pass
