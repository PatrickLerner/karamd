---
title: "Core taskmd library: config, model, safe read/write"
id: "008"
status: completed
priority: high
type: feature
tags: ["work", "core"]
created_at: "2026-07-02"
completed_at: 2026-07-02
---

# Core taskmd library: config, model, safe read/write

## Objective

The foundation for everything else: a reusable Rust `taskmd` library that parses
the taskmd config, models a task with all its frontmatter, and reads/writes task
files safely (round-trip-safe, atomic, id-collision-free). Verbs, query, ranking
(#011, #012) and the web UI (#009) all build on this. No new CLI surface here
beyond what the library needs; this task is the library layer.

## Description

Settle first, because it reverses a documented premise:

- **Design shift to record in CLAUDE.md.** Today's rule is "karamd only *adds*
  files and *reads* completion state; completions happen elsewhere
  (taskmd/Obsidian)." Making karamd a first-class *writer* of task state (#011)
  breaks the read-only assumption. Document that karamd is now a general taskmd
  tool, of which recurring generation is one command.
- **Compatibility.** The binary interface need NOT match taskmd's; only the
  *output format* (files, frontmatter fields) must stay compatible so
  taskmd/Obsidian keep working on the same vault. The authoritative contract is
  `taskmd spec` (spec version 1.2 for taskmd 0.2.5) — regenerate with
  `taskmd spec --stdout` and follow it exactly; do not guess field names or enum
  values.
- **Library-first.** All logic lands in the library crate as a reusable
  `taskmd` layer. The binary stays a thin clap shim. Keep functions pure and I/O
  thin so the 100%-line-coverage gate holds.

Config (`.taskmd.yaml` by default; also the `_config/taskmd.yaml` shape). Parse
more than `dir`/`phases` — the full config affects output:

- `dir` — tasks directory relative to vault root.
- `phases` — ordered; each `{id, name, description?, due?}`. Task `phase` matches
  phase `id` (falls back to `name`).
- **`id`** — id generation: `strategy` (`sequential` | `prefixed` | `random` |
  `ulid`), `prefix` (for prefixed), `length`, `padding`. Default sequential,
  padding 3. karamd must honor the configured strategy, not assume zero-padded
  numeric.
- **`workflow`** — `solo` (default) or `pr-review`. In `pr-review`, "mark done"
  sets `in-review` (+ a PR url) instead of `completed`. Model this so #011's
  complete verb behaves correctly.
- **`scopes`** — scope-id -> `{description?, paths[]}` map, backing `touches`.

Model + I/O:

- `Task` model covering the **full** frontmatter per spec, not a subset:
  `id`, `title`, `status`, `priority`, `effort`, `type`, `dependencies`, `tags`,
  `group`, `owner`, `phase`, `touches`, `context`, `parent`, `created_at`
  (accept the `created` alias), `completed_at`, `cancelled_at`, `verify`, `pr`,
  `external_id`, plus karamd's custom `recurring`, plus the markdown body.
- **Enums per spec (do not invent):** status is
  `pending | in-progress | completed | in-review | blocked | cancelled`
  (NOTE: `completed`, not `done`; `in-progress`/`in-review` are hyphenated).
  priority `low|medium|high|critical`; effort `small|medium|large`; type
  `feature|bug|improvement|chore|docs`.
- **Auto timestamps:** set `completed_at` when status -> `completed` and CLEAR it
  when status leaves `completed`; same for `cancelled_at`/`cancelled`.
- **Round-trip safe:** unknown fields are preserved as-is (spec says parser
  ignores + preserves them); tolerate CRLF. karamd -> taskmd -> karamd drops
  nothing and never duplicates.
- **Scanner robustness:** only files with real leading `---` frontmatter and a
  valid `id`/`title` are tasks. Ignore everything else — `TASKMD_SPEC.md`,
  READMEs, `.taskmd/templates/`, and docs whose bodies contain fenced ```yaml```
  examples. A fenced example must never be mistaken for a task.
- **Groups:** resolve `group` from explicit field, else parent directory name,
  else none. Support subdirectories under the tasks dir.
- **Atomic writes:** temp file + rename; never a partial file a syncing Obsidian
  could pick up mid-write.
- **Id allocation under concurrency:** vault synced by Obsidian, written by three
  actors (Obsidian, recurring generator, this library). Allocate per the
  configured id strategy by scanning at write time; handle collisions; do not
  cache a stale max.
- **External-change awareness:** re-read before mutating so an edit does not
  clobber a change synced in since load.

Dependencies + hierarchy (backbone of `next` in #012):

- `dependencies`: list of task ids. Build the graph; compute per-task
  **readiness** (ready only when every dependency is `completed`; taskmd's `next`
  treats a task actionable when deps are completed). Expose is-ready, direct
  blockers, transitive readiness. Reject **cycles** and **dangling refs**.
- `parent`: single task id, hierarchical grouping only — NOT a dependency, no
  blocking, no status cascade. Children computed dynamically. Validate: parent
  exists, no self-reference, no parent cycles. Keep distinct from dependencies.

This task does NOT include the verbs, query language, output formats, or ranked
`next` (those are #011/#012). It provides the model + safe I/O + config those
depend on.

## Tasks

- [ ] Settle and document the read-write design shift in CLAUDE.md
- [ ] Config: parse `dir`, `phases` (id/name/description/due), `id` strategy,
      `workflow`, `scopes`; default `.taskmd.yaml` in vault root
- [ ] `Task` model with the full spec field set + body; correct enums; `created`
      alias; round-trip-safe parse/serialize preserving unknown fields + CRLF
- [ ] Auto set/clear `completed_at` and `cancelled_at` on status change
- [ ] Configurable id allocation (sequential/prefixed/random/ulid), collision-safe
- [ ] Scanner ignores non-task files (spec doc, README, templates, fenced yaml)
- [ ] Group resolution (explicit / dir / none) with subdirectory support
- [ ] Atomic write (temp + rename); re-read-before-mutate
- [ ] Dependency graph (readiness, blockers, cycles, dangling) + `parent`
      hierarchy validation (exists, no self-ref, no parent cycles)
- [ ] TDD throughout; keep 100% line coverage (exclude the main.rs shim)
- [ ] Update README/CLAUDE.md (design shift, library layout)

## Acceptance Criteria

- Config parsed incl. `id` strategy, `workflow`, `phases`, `scopes`
- Full field set modeled with spec-correct enums (`completed`, not `done`)
- A karamd -> taskmd -> karamd round trip preserves all fields, incl. custom and
  unknown ones; no duplicates, CRLF tolerated
- `completed_at`/`cancelled_at` auto-set and cleared correctly on status change
- Writes are atomic; ids never collide; id strategy honored
- Scanner never treats the spec doc / templates / fenced yaml as tasks
- Dependency cycles, dangling refs, and parent cycles are detected and rejected
- Recurring generation still works unchanged
- fmt, clippy, tests, and the 100%-line coverage gate all pass
