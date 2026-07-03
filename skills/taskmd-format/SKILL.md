---
name: taskmd-format
description: The taskmd task-file format: frontmatter fields, enum values, ID strategies, .taskmd.yaml config keys, slug/filename rules, and validation. Use when creating, editing, or validating any taskmd task file (the `.md` files in a tasks/ directory), or when implementing tooling that must stay taskmd-compatible. Keeps files parseable by taskmd, karamd, and Obsidian.
---

# taskmd file format

Authoritative source: `taskmd spec --stdout` (spec v1.2, taskmd 0.2.5). If taskmd
is upgraded, regenerate and reconcile this skill. Where the spec doc and the
0.2.5 binary disagree, the binary wins (noted inline below).

Each task is one `.md` file: YAML frontmatter between `---` delimiters, then a
markdown body. Only files with valid leading frontmatter and an `id` + `title`
are tasks; the spec doc, READMEs, templates, and fenced ```yaml``` examples are
not.

## Frontmatter fields

Required: `id` (unique string, e.g. `"001"`, `"cli-049"`) and `title`.

Optional:

| Field | Values / format |
|-------|-----------------|
| `status` | `pending`, `in-progress`, `in-review`, `completed`, `blocked`, `cancelled` |
| `priority` | `low`, `medium`, `high`, `critical` |
| `effort` | `small`, `medium`, `large` |
| `type` | `feature`, `bug`, `improvement`, `chore`, `docs` |
| `dependencies` | array of task-id strings, e.g. `["001", "015"]` |
| `tags` | array, lowercase hyphen-separated |
| `group` | string (else derived from parent directory) |
| `owner` | free-form string |
| `phase` | string; matches a phase `id` in `.taskmd.yaml` |
| `touches` | array of scope ids (e.g. `["cli/graph"]`) |
| `context` | array of file paths |
| `parent` | single task id (hierarchy only, not a dependency) |
| `created_at` | `YYYY-MM-DD` (alias: `created`) |
| `completed_at` | `YYYY-MM-DD`; auto-set when status becomes `completed`, cleared when it leaves |
| `cancelled_at` | `YYYY-MM-DD`; auto-set when status becomes `cancelled`, cleared when it leaves |
| `verify` | array of typed checks (`{type: bash, run, dir?}` or `{type: assert, check}`) |
| `pr` | array of PR URLs |
| `external_id` | string from an external system |

### Exact facts that bite

- Status is **`completed`**, never `done`. `in-progress` and `in-review` are
  **hyphenated**. The full set is exactly the six above.
- `completed_at` / `cancelled_at` are auto-maintained: set on entering that
  status, cleared on leaving it. Don't hand-stamp them out of sync with `status`.
- `created` is an accepted alias for `created_at`.
- `parent` is organizational only: no blocking, no status cascade.
  `dependencies` is the blocking relation.
- **Unknown fields are preserved verbatim on edit: never drop them.** e.g.
  karamd's `recurring:` marker must survive a `status` change. CRLF line endings
  are tolerated (a synced vault can pick them up); don't assume LF-only.

## Validation (must)

Valid `id` + `title`; valid enum values; ids unique project-wide;
`dependencies` and `parent` reference existing tasks; no dependency cycles; no
parent self-reference or parent cycles. A `cancelled` dependency blocks forever
(it never reaches `completed`).

## `.taskmd.yaml` config

```yaml
dir: tasks                 # task directory
workflow: solo             # or pr-review (completing then sets in-review + records PR)
id:
  strategy: sequential     # sequential | prefixed | random | ulid
  prefix: "dr"             # for prefixed
  padding: 3               # sequential/prefixed zero-pad width
  length: 6                # random/ulid length
phases:
  - id: core
    name: "Core"
    due: 2026-04-01
scopes:
  cli/graph:
    paths: ["src/graph/"]
```

### ID strategies

- `sequential`: zero-padded running number, e.g. `001` (default padding 3).
- `prefixed`: `<prefix><NNN>` with **no separator**, e.g. `dr001`. The spec
  doc's `dr-001` is **wrong**; the 0.2.5 binary emits `dr001`.
- `random`: base36, default length 6.
- `ulid`: Crockford base32, timestamp-prefixed, default length 6.

## File naming

`ID-descriptive-slug.md` (e.g. `009-add-feature.md`, `dr001-nightly-check.md`).

Slug rule: lowercase; every non-`[a-z0-9]` character becomes `-`; collapse and
trim runs of `-`; **non-ASCII letters are dropped** ("prüfen" → "pr-fen"). The
ID prefix in the filename follows the `id` strategy.

## Minimal example

```markdown
---
id: "001"
title: "Fix login button alignment"
status: pending
---

# Fix Login Button Alignment

Update the CSS to center the button.
```
