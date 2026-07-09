---
id: '042'
title: 'run: optional concurrency (run.concurrency) with per-working-dir serialization'
status: pending
created_at: 2026-07-09
priority: low
type: improvement
tags:
- run
- concurrency
dependencies:
- '039'
---

## Description

`karamd run` (#039) executes runnable tasks strictly sequentially: `run_all` walks the
selected ids in order and blocks on each agent (spawn → wait-with-timeout → record) before
starting the next. Fine for v1 (easy to watch, one autonomous writer at a time), but a large
runnable set with slow agents serialises badly.

## Proposed work

Add an opt-in concurrency bound so independent tasks run in parallel.

- **Config knob:** `run.concurrency: N` (default 1 = today's behaviour). Cap the number of
  agents in flight at N.
- **Working-dir safety:** tasks sharing a `working_dir` must NOT run concurrently (parallel
  writers would clobber the same repo). Serialize per resolved working dir; only tasks with
  distinct working dirs run together. This is the core correctness constraint.
- **Lock model still holds:** each task marks `ai_status: running` before its spawn, so the
  existing cross-invocation lock and stale-reconciliation keep working. Confirm no races on
  the attempt counter when several tasks write their own files concurrently (they write
  different files, so this should be fine, but verify).
- **Output:** keep per-task result lines deterministic (collect, then print in id order) so
  logs stay readable regardless of finish order.

## Testability / coverage

The orchestration already runs behind the `AgentRunner` trait, so a fake runner can simulate
slow/fast agents deterministically. Keep the scheduling logic pure/testable; if a thread pool
or async is introduced, isolate the untestable glue the way `run_spawn.rs` is excluded.
Decide between std threads and reusing the existing tokio runtime.

## Acceptance Criteria

- [ ] `run.concurrency: N` runs up to N agents at once; default 1 preserves current behaviour.
- [ ] Tasks that resolve to the same working dir never run concurrently.
- [ ] Attempt counting / parking / marker clearing stay correct under concurrency.
- [ ] Result reporting is deterministic (id order) regardless of completion order.
- [ ] 100% line coverage on the scheduling logic; only real thread/process glue excluded.

## Notes

Depends on #039. Not urgent (deferred deliberately at #039 time). The single source of truth
for selection stays `run::is_runnable`; this task only changes *how many* run at once.
