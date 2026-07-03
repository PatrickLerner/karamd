---
name: taskmd-format
description: The taskmd task-file format — frontmatter fields, enum values, ID strategies, config keys, and validation rules. Use when creating, editing, or validating any taskmd task file (the `.md` files in tasks/), or when implementing karamd's taskmd compatibility. Keeps generated files parseable by taskmd/Obsidian.
---

# taskmd file format

Authoritative source: `taskmd spec --stdout` (spec v1.2, taskmd 0.2.5). If taskmd
is upgraded, regenerate and reconcile this skill. karamd's job is to emit files
that match this format exactly.

Each task is one `.md` file: YAML frontmatter between `---` delimiters, then a
markdown body. Only files with valid leading frontmatter and an `id` + `title`
are tasks; the spec doc, READMEs, templates, and fenced ```yaml``` examples are
not.

## Frontmatter fields

Required: `id` (unique string, e.g. `"001"`, `"cli-049"`), `title`.

Optional:

| Field | Values / format |
|-------|-----------------|
| `status` | `pending`, `in-progress`, `completed`, `in-review`, `blocked`, `cancelled` |
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
| `completed_at` | `YYYY-MM-DD`; auto-set when status→`completed`, cleared when it leaves |
| `cancelled_at` | `YYYY-MM-DD`; auto-set when status→`cancelled`, cleared when it leaves |
| `verify` | array of typed checks (`{type: bash, run, dir?}` or `{type: assert, check}`) |
| `pr` | array of PR URLs |
| `external_id` | string from an external system |

Common pitfalls:
- Status is **`completed`**, never `done`; `in-progress`/`in-review` are hyphenated.
- `parent` is organizational only — no blocking, no status cascade. Dependencies
  are the blocking relation.
- Unknown fields (e.g. karamd's `recurring`) are preserved as-is — never drop them
  on edit.

## Validation (must)

Valid `id` + `title`; valid enums; unique ids project-wide; `dependencies` and
`parent` reference existing tasks; no dependency cycles; no parent self-reference
or parent cycles.

## `.taskmd.yaml` config

```yaml
dir: tasks                 # task directory
workflow: solo             # or pr-review (then "done" → in-review + PR)
id:
  strategy: sequential     # sequential | prefixed | random | ulid
  prefix: ""               # for prefixed
  padding: 3               # sequential zero-pad width
phases:
  - id: core
    name: "Core"
    due: 2026-04-01
scopes:
  cli/graph:
    paths: ["src/graph/"]
```

## File naming

`ID-descriptive-slug.md` (e.g. `009-add-feature.md`, `cli-049-add-graph.md`).
Slug: lowercase, hyphen-separated. ID prefix in the filename follows the `id`
strategy.

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
