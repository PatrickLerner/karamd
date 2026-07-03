---
title: "Port remaining taskmd commands surfaced by the spec"
id: "014"
status: completed
priority: low
type: feature
tags: ["work", "core"]
dependencies: ["011"]
created_at: "2026-07-02"
completed_at: 2026-07-02
---

# Port remaining taskmd commands surfaced by the spec

## Objective

Reading `taskmd spec` + `taskmd --help` (v0.2.5) revealed a large command
surface beyond the original create/complete/cancel/pending + list + query + next
scope (#008/#011/#012). Capture the rest here so nothing is lost; implement
selectively as needed, not all at once.

## Description

The core library (#008) already models the full format, so most of these are
thin read-only views over it. Grouped by value:

High value / natural next steps:

- **`validate`** — split out to its own task (#015).
- **`graph`** — export the dependency graph (e.g. DOT/JSON). Reuses #008's graph.
- **`search`** — full-text search across titles and bodies.
- **`stats` / `phases` / `tags`** — computed metrics; per-phase progress; tag
  counts. Read-only aggregations.

Medium:

- **`board`** — kanban-style grouped view (maps well onto the #009 web UI too).
- **`snapshot`** — frozen machine-readable representation of all tasks.
- **`templates`** — manage `.taskmd/templates/*.md`; `create --template` (#011)
  is the consumer.
- **`tracks`** — parallel work tracks from `touches`/scope overlap (needs the
  `scopes` config from #008).
- **`verify`** — run the typed `verify:` checks (`bash` / `assert`) on a task.
- **`worklog`** — view/add worklog entries.

Lower / environment-specific:

- **`archive`**, **`deduplicate`**, **`rm`**, **`next-id`**, **`context`**,
  **`report`**, **`feed`**, **`commit-msg`**, **`import`/`sync`**,
  **`projects`** (multi-project registry), **`mcp`** (taskmd's own MCP server).

Decide per command whether karamd needs it at all (some are taskmd-workflow
specific). This task is a menu, not a mandate; split into per-command tasks when
picked up.

## Tasks

- [x] Triage the list; pick which commands karamd actually needs
- [x] Implement chosen commands as thin layers over #008 (read-only where
      possible); human + JSON/YAML output via #011's serializer
- [x] Split anything non-trivial into its own task
- [x] Document implemented commands in README

## Implemented (this pass)

- **`search <text>`** — case-insensitive full-text over titles + bodies
  (`src/verbs.rs`).
- **`graph`** — dependency graph; human output is Graphviz DOT, `--json`/`--yaml`
  emit nodes+edges+readiness (`src/analyze.rs`).
- **`stats`** — counts by status/priority/phase plus ready/blocked
  (`src/analyze.rs`).

All three are read-only over the #008 model/graph, render through #011's
serializer, and are covered by unit + e2e tests under the 100% gate.

## Deferred (explicitly not dropped)

Not implemented; revisit per demand, splitting each into its own task when
picked up: `board`, `snapshot`, `templates`, `tracks`, `verify`, `worklog`,
`archive`, `deduplicate`, `rm`, `next-id`, `context`, `report`, `feed`,
`commit-msg`, `import`/`sync`, `projects`, `mcp`. Several are taskmd-workflow
specific (git `commit-msg`, multi-project `projects`, taskmd's own `mcp`) and
may never be needed in karamd. `board` maps naturally onto the web UI (#009)
rather than a CLI view.

## Acceptance Criteria

- Chosen commands produce output consistent with the #008 model and #011 formats
- Anything deferred is explicitly recorded, not silently dropped
- fmt, clippy, tests, and the coverage gate pass for what is implemented
