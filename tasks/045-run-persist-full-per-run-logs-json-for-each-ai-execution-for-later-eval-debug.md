---
id: '045'
title: 'run: persist full per-run logs (JSON) for each ai execution, for later eval/debug'
status: completed
created_at: 2026-07-10
priority: medium
type: feature
tags:
- run
- logging
dependencies:
- '039'
completed_at: 2026-07-10
---

## Context

`karamd run` (#039) spawns an agent per `ai-runnable` task and only *inherits*
the agent's stdout/stderr — see `src/run_spawn.rs`: "Inherit stdout/stderr so the
operator sees agent output in the cron/terminal log; a per-run log file is a
follow-up." So today the only record of a run is whatever the surrounding process
happened to capture (on the dastyar box: the systemd journal) — ephemeral,
unstructured, and interleaved across tasks/timer fires.

## Problem

There is no durable, structured, per-run record for later eval/debug. Per
execution you want: which task, which agent + the resolved command, start/end
timestamps, duration, attempt number, exit code, karamd's outcome
(completed / failed / parked), last error, and the agent's full captured output.

## Proposed

`karamd run` writes a per-run artifact to a configurable dir (e.g. `run.log_dir`,
default under a state dir or `<vault>/.karamd/runs/`), one entry per execution:

- a JSON record: `{ id, agent, command, working_dir, started_at, ended_at,
  duration_s, attempt, exit_code, outcome, last_error }`
- the agent's captured stdout+stderr (tee'd — keep inheriting to the console too,
  so the journal stays useful, but also persist it), embedded or alongside.
- append each record to a single run-index JSONL so all runs are queryable in one
  place.
- a retention/prune knob (the target box has a 64 GB disk).

## Notes

- Found via the dastyar zeroclaw integration (opencode as the agent). opencode
  also keeps its own session JSON under `~/.local/share/opencode/storage` + debug
  logs under `.../log/`; karamd can't know per-agent internals, but capturing the
  tee'd stdout/stderr + the structured run record covers the general case.
- Pairs with #044 (surface run state in the web UI): the same records could back a
  web "run history" view.
- Pointers: `src/run_spawn.rs` (ProcessRunner::run), `src/run.rs` (orchestration,
  outcomes, attempt/park transitions).
