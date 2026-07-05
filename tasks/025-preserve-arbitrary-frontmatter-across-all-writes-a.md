---
title: Preserve arbitrary frontmatter across all writes and make due a first-class field in the web UI
id: '025'
status: completed
priority: medium
type: feature
tags:
- work
- core
created_at: 2026-07-04
completed_at: 2026-07-05
---

# Preserve arbitrary frontmatter across all writes and make due a first-class field in the web UI

## Objective

Guarantee that every karamd write path preserves frontmatter fields karamd
doesn't model (not just at read time), and promote taskmd's `due` field from a
merely-preserved unknown to a first-class field that can be viewed and edited in
the web UI (and set from the CLI).

## Motivation

Two related gaps:

1. **Arbitrary frontmatter.** `src/taskmd/model.rs` already backs a `Task` with
   its complete frontmatter as an ordered `Mapping` and mutates via positional
   upsert, so unknown fields *should* round-trip. But this is an invariant we
   assert in prose, not a tested guarantee, and it has to hold across *all*
   writers: `create`, `edit`/`patch_task`, `status`, and the generator. A single
   writer that reconstructs a task from a known-field subset would silently drop
   a user's custom fields on the next edit. taskmd/Obsidian users add their own
   frontmatter; losing it on a karamd web edit is data loss.
2. **`due` is second-class.** `due` (target date, `YYYY-MM-DD`) is a real taskmd
   task field, but karamd has no `due()` accessor/setter, it is absent from
   `TaskView` (`src/output.rs`), from `CreateSpec`/`EditSpec` (`src/verbs.rs`),
   from the web `CreateBody`/`PatchBody` (`src/web.rs`), and from the SPA
   (`web/src/`). It only survives as a preserved unknown, so the web UI can
   neither show nor set it.

## Description

- **Preservation (verify + lock in):** add regression tests proving a task with
  arbitrary custom frontmatter keys (in arbitrary positions) survives a full
  round trip through `create`, `edit`, and `status` unchanged (key order,
  values, unknown nested structures). Fix any writer that doesn't. Consider
  surfacing preserved-but-unmodelled keys read-only in the web detail view so
  users can see "existing stuff" is intact.
- **First-class `due`:**
  - `model.rs`: `due()` accessor + `set_due(Option<&str>)` setter (positional
    upsert like the other fields; clearing removes the key). Validate the
    `YYYY-MM-DD` shape; tolerate what taskmd accepts.
  - `output.rs`: add `due: Option<String>` to `TaskView` + `build`.
  - `verbs.rs`: `due` on `CreateSpec` and `EditSpec` (+ CLI flag `--due`,
    clearing via empty value, consistent with the `owner` double-option style).
  - `web.rs`: `due` on `CreateBody` and `PatchBody` (double-option so it can be
    set and cleared), threaded into `create`/`edit`.
  - `web/src/`: `types.ts` task `due` field; show it in the detail/summary view;
    an editable date input in the edit form.
- `validate` should flag a malformed `due` (bad date) the way it flags other
  field problems.

## Tasks

- [ ] Regression tests: arbitrary/unknown frontmatter round-trips through
      `create`, `edit`, `status` untouched (order + values)
- [ ] Fix any write path that drops unknown fields (if found)
- [ ] `due()` / `set_due()` in `model.rs` with `YYYY-MM-DD` validation
- [ ] `due` in `TaskView` (`output.rs`), `CreateSpec`/`EditSpec` + `--due` flag
      (`verbs.rs`), and `CreateBody`/`PatchBody` (`web.rs`)
- [ ] SPA: display `due` and edit it via a date input (`web/src/`)
- [ ] `validate` rejects a malformed `due`
- [ ] TDD; keep 100% line coverage; update README + CLAUDE.md field list

## Acceptance Criteria

- A task carrying custom frontmatter keys is edited via `karamd edit` and via
  the web UI, and every unmodelled key (and its position) is preserved
- `karamd create --due 2026-08-01` and `karamd edit <id> --due 2026-08-01` set
  the field; `--due ""` clears it; a bad date is rejected with a clear error
- The web UI shows a task's `due` and can set/change/clear it, persisting a
  spec-valid `due` in frontmatter
- `karamd validate` flags a malformed `due`
- fmt, clippy, tests, and the 100%-line coverage gate all pass
