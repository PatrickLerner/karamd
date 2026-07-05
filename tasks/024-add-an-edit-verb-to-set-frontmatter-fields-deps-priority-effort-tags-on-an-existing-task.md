---
id: '024'
title: Add an 'edit' verb to set frontmatter fields (deps, priority, effort, tags) on an existing task
status: completed
created_at: 2026-07-04
priority: medium
type: feature
tags:
- work
- core
dependencies:
- '016'
completed_at: 2026-07-05
---

## Objective

Add a verb to modify a task's frontmatter fields in place, so karamd is
self-sufficient for the whole task lifecycle and callers never fall back to the
`taskmd` CLI for a field karamd can't set.

## Motivation

karamd today has `status` (status enum only) and `create --depends-on`, but no
way to set dependencies (or priority/effort/tags/type/owner) on an *existing*
task. Hit live on 2026-07-04: blocking a task on a newly-created one required
`taskmd set <id> --depends-on <id>` because karamd offered no equivalent. Since
task creation was deliberately routed through karamd (to kill the confabulated
`taskmd add`), the edit path should live there too — one tool, not two.

## Description

- New subcommand (name TBD: `edit` or `set`) taking a task id + flags mirroring
  `create`'s field flags: `--depends-on` (comma-separated; consider
  `--add-dep`/`--remove-dep` for non-destructive edits), `--priority`,
  `--effort`, `--type`, `--add-tag`/`--remove-tag`, `--owner`.
- Deliberately does NOT set `status` (that stays the `status` verb) — or, if
  unified, keep `status`/`complete`/`cancel` as the workflow-aware path and make
  `edit` field-only.
- Rewrite frontmatter canonically (same writer as `create`/`status`); never
  touch `completed_at`/terminal timestamps. `--json`/`--yaml` output.
- Validate references (a `--depends-on` id must exist; reject cycles — reuse
  `validate`'s graph check).

## Tasks

- [ ] Add the subcommand + clap flags to the tree (depends on #016)
- [ ] Field writers reusing the create/status frontmatter serializer
- [ ] Dependency edit with existence + cycle checks
- [ ] TDD; keep 100% line coverage
- [ ] Update README + CLAUDE.md CLI section

## Acceptance Criteria

- `karamd edit <id> --depends-on 018` sets deps on an existing task; re-running
  is idempotent; a missing/cyclic dep is rejected with a clear error
- Non-status fields (priority/effort/tags/type/owner) are settable in place
- `completed_at` and other terminal timestamps are never disturbed
- fmt, clippy, tests, and the 100%-line coverage gate all pass
