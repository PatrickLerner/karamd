---
id: '041'
title: 'query/next: expose ai-runnable selection (tag filter + attempts-left) as a query/next surface'
status: completed
created_at: 2026-07-09
priority: low
type: feature
tags:
- run
- query
- next
dependencies:
- '039'
completed_at: 2026-07-09
---

## Description

`karamd run` (#039) has its own selection predicate (`run::is_runnable`): tagged
`ai-runnable`, not parked (`ai-failed`), not terminal, `ai_attempts < max_attempts`, not
locked by a fresh run. `karamd run --dry-run` already prints exactly this set. What's
missing is composing that selection into the general query/next surface, the way a human
browses tasks: e.g. "show me the AI-runnable tasks that still have attempts left, ranked".

## What exists vs. what's wanted

- **Exists:** `karamd run --dry-run` lists the runnable tasks (the executor's own view).
- **Wanted:** the same idea as a *query filter* usable by `list` and `next`, so it composes
  with other terms and the ranking, not just the executor.

## Proposed work

1. **`tag:<name>` query term.** Add a tag-membership filter to the `list` mini-grammar
   (`src/query.rs`), so `list "tag:ai-runnable AND open:"` works. Generally useful beyond
   this feature; pairs with the existing `open:` filter.
2. **Attempts-left filter.** `ai_attempts < max_attempts` is the config-dependent part: the
   query evaluator currently has no access to `run.max_attempts`. Either
   - thread the vault config into query evaluation and add a computed
     `ai-runnable:`/`runnable:` pseudo-term that mirrors `run::is_runnable`, or
   - keep it out of the pure grammar and instead add a `karamd run --list`/`--json` mode
     (a thin wrapper over `run::plan`) that emits the selectable set as task views for
     scripting.
   Decide during design; option (b) is smaller and avoids leaking config into the grammar.
3. **Optional: `next` integration.** A `--runnable`/`--ai` flag on `next` that pre-filters
   to the run-selectable set before ranking, so you get a prioritized queue of AI work.

## Acceptance Criteria

- [ ] `list` supports `tag:<name>` (membership), with unit tests and 100% line coverage.
- [ ] There is a documented, scriptable way to get the AI-runnable-with-attempts-left set
      as task views (either a `runnable:` query term or `run --list --json`).
- [ ] Whatever surface is chosen agrees with `run::is_runnable` (no drift between what
      `run` executes and what the selector shows).
- [ ] Machine output (`--json`/`--yaml`) works for the new surface.

## Notes

Depends on #039 (the executor and its `is_runnable`/`plan`). The single source of truth for
"is this task runnable" must stay `run::is_runnable`; this task only exposes it, it does not
re-implement the predicate.
