---
id: '049'
title: 'run: re-scan the runnable set after each task concludes (pick up tasks launched mid-run)'
status: completed
created_at: 2026-07-10
priority: medium
type: improvement
tags:
- run
dependencies:
- '039'
completed_at: 2026-07-10
---

## Context

`karamd run` computes its runnable set once at the start (`run::plan`, src/run.rs)
and processes that snapshot sequentially. Tasks that become runnable *during* the
invocation are not seen until the next `karamd run` (on dastyar: the next hourly
`:30` timer).

## Problem

Sequential runs can be long (up to `timeout_secs` per task; now 30 min on the box).
If a new `ai-runnable` task appears while a run is in progress — `generate`
materialises a recurring one, another process/agent creates one, a parked task is
un-parked, or a dependency completes making a blocked task ready — it waits for the
next whole invocation instead of being drained now.

## Request

After each task concludes, `karamd run` should **re-scan** the vault for the current
runnable set (same `is_runnable` / `plan` predicate — no drift) and continue with any
newly-eligible tasks, until the set is empty. An invocation should drain everything
runnable *as of each step*, not just the initial snapshot.

## Notes

- Keep the safety invariants: same selection predicate, attempts bumped before spawn,
  `max_attempts` parking, the running-lock so a task can't be double-picked.
- Guard against a pathological loop (a rule/agent that keeps spawning new runnable
  tasks could make one invocation run forever): a per-invocation cap (max tasks or
  wall-clock budget) with a clear log when hit.
- Pointers: src/run.rs (orchestration loop over `plan`, `run::plan`, `is_runnable`).
- Relates to #039 (run) and the sequential-throughput discussion (#042 concurrency).
