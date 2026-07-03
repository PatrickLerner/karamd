---
title: "Ranked next-task mode"
id: "012"
status: completed
priority: medium
type: feature
tags: ["work", "core"]
dependencies: ["011"]
created_at: "2026-07-02"
completed_at: 2026-07-02
---

# Ranked next-task mode

## Objective

A ranked mode that surfaces the next task to do, matching taskmd's `next`
algorithm and flags so results agree with taskmd on the same vault.

## Description

Builds on the #008 model (incl. the dependency graph + readiness) and #011
query/eval. Dependencies are the backbone of this command, not a tie-breaker.

- **Readiness is a hard gate.** Only *ready* tasks (all dependencies
  `completed`, per #008) are eligible to be `next`. A task blocked by an open
  dependency is never surfaced as the next task, regardless of its priority.
  (Edge case to settle: a dependency that is `cancelled` never completes —
  decide whether that permanently blocks the dependent or counts as satisfied.)
- **Match taskmd's algorithm.** Score ready tasks on priority, critical-path
  position, downstream impact (how many tasks this unblocks), effort, and phase
  ordering — the same factors taskmd's `next` uses. Actionable = `pending` or
  `in-progress` with all dependencies `completed`. Mirror its flags: `--limit`,
  `--quick-wins` (effort=small), `--critical`, `--phase`, `--strict-phases`.
  Verify parity against `taskmd next --format json` on the same vault.
- **Surface the blockers.** For a blocked high-priority task, show what is
  blocking it (its open dependencies) so the user sees the real next action is
  to clear a blocker. Consider suggesting the ready dependency that unblocks the
  most / highest-priority downstream work.
- Expose as a verb (e.g. `karamd next`) with human and `--json`/`--yaml` output
  (reuse #011's serializer).
- Document the scoring and the readiness gate so results are explainable.

## Tasks

- [ ] Readiness gate: only ready tasks (all deps `completed`) are eligible
- [ ] Scoring per taskmd: priority, critical-path, downstream impact, effort,
      phase ordering
- [ ] Flags: `--limit`, `--quick-wins`, `--critical`, `--phase`, `--strict-phases`
- [ ] Blocker view: show open deps blocking a task; suggest unblocking task
- [ ] `next` verb; human + JSON/YAML output via #011
- [ ] Parity check against `taskmd next --format json`
- [ ] Document the scoring model + readiness gate in README
- [ ] TDD throughout; keep 100% line coverage

## Acceptance Criteria

- A task blocked by open dependencies is never returned as `next`
- Ranking matches taskmd's factors and agrees with `taskmd next` on a test vault
- Blockers of a high-priority blocked task are shown
- fmt, clippy, tests, and the 100%-line coverage gate all pass
