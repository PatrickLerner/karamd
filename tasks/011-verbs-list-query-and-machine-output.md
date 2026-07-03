---
title: "Task verbs, list, query mini-grammar, and JSON/YAML output"
id: "011"
status: completed
priority: high
type: feature
tags: ["work", "core"]
dependencies: ["008", "016"]
created_at: "2026-07-02"
completed_at: 2026-07-02
---

# Task verbs, list, query mini-grammar, and JSON/YAML output

## Objective

CLI verbs and querying on top of the core library (#008): create, complete,
cancel, reopen; list tasks in the current vault; a query mini-grammar; and
output in both human-readable and machine-readable (JSON/YAML) form for AI
consumers. The binary interface may differ from taskmd's; only file output stays
compatible.

## Description

Builds on the #008 `Task` model + safe I/O; no duplicate task logic here.

State transitions (spec-correct enums; #008 does the auto-timestamping):

- **complete**: `status: completed` (NOT `done`); #008 sets `completed_at`. In
  `workflow: pr-review`, "mark done" instead sets `in-review` (+ a PR url) — the
  complete verb must respect the configured workflow mode.
- **cancel**: `status: cancelled`; #008 sets `cancelled_at`.
- **reopen / pending**: `status: pending`; #008 clears the terminal timestamp.
- Also support setting `in-progress`, `in-review`, `blocked` (full enum).
- **create**: new file via the #008 allocator + slug rule; support `--template`
  (`.taskmd/templates/*.md`: bug/feature/chore/custom) so created tasks match
  taskmd's templated output.

All writes go through #008 (atomic, re-read-before-mutate, custom + unknown
fields preserved).

Query mini-grammar:

- Terms `field:value` over status, phase, priority, effort, type, tag, owner,
  scope (`touches`), dependency/readiness state, dates; combine with
  `AND`/`OR`/`NOT` and parentheses, e.g.
  `status:pending AND priority>=high AND tag:core`.
- Support comparison operators on ordered enums (priority, effort): `>=`,`>`,
  `<=`,`<`, matching taskmd's `list --filter "priority>=medium"`.
- Prefer a small parser-combinator crate (`winnow`/`nom`; `chumsky`/`pest` only
  if the grammar grows) or a tiny hand-rolled recursive-descent parser. Keep the
  dependency set small. Document the grammar.

Output:

- Human-readable table/list by default; `--json` and `--yaml` for machines/AI.
  One serializable model backs all three.

## Tasks

- [ ] Verbs: create (+ `--template`), complete (workflow-aware), cancel,
      reopen/pending, and set in-progress/in-review/blocked (via #008)
- [ ] `list` over the current vault with filters + sort
- [ ] Query mini-grammar: `field:value` + comparisons + AND/OR/NOT + grouping
- [ ] Output: human default plus `--json` / `--yaml` from one model
- [ ] Document the grammar and verbs in README
- [ ] TDD throughout; keep 100% line coverage

## Acceptance Criteria

- Verbs produce taskmd-compatible files (correct enum `completed`; round trip
  clean; custom + unknown fields intact); complete respects `workflow` mode
- `list` + query return correct results; grammar handles comparisons, AND/OR/NOT,
  grouping
- Same query renders identically in human, JSON, and YAML forms
- fmt, clippy, tests, and the 100%-line coverage gate all pass
