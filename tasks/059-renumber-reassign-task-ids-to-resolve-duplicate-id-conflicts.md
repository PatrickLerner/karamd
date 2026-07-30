---
id: '059'
title: 'renumber: reassign task ids to resolve duplicate-id conflicts'
status: pending
created_at: 2026-07-30
priority: medium
type: feature
tags:
- cli
- ids
---

## Description

A new `karamd renumber` subcommand that detects duplicate task ids in a vault and
reassigns them so every id is unique again.

Duplicates happen in practice: the vault is synced across devices, so two
machines can allocate the same next id offline, and a merge of two branches of
this repo's own `tasks/` produces the same collision. Today nothing repairs it.
`validate` can report it at best; the id is baked into both the frontmatter and
the filename, and `dependencies:` may point at the id from elsewhere.

## Behavior

- Scan the vault, group tasks by id, and find every id held by more than one task.
- Keep one task per duplicate group (the oldest by `created_at`, tie-broken by
  filename, so the winner is stable across runs and machines) and allocate a new
  id for each other member using the configured id strategy.
- Rewrite the renumbered task's `id:` frontmatter *and* rename its file to match
  the `NNN-slug.md` convention.
- Rewrite every `dependencies:` entry in the vault that pointed at the moved task
  so the graph stays intact. A dependency on a duplicated id is ambiguous by
  definition: report it and leave it pointing at the kept task.
- Idempotent: a vault with no duplicates is a no-op, and a second run changes
  nothing.
- `--dry-run` prints the plan (old id -> new id, old path -> new path, and the
  dependency edits) without writing.

## Acceptance Criteria

- [ ] `karamd renumber` resolves duplicate ids and exits 0 with a summary of what moved
- [ ] `karamd renumber --dry-run` shows the same plan and writes nothing
- [ ] New ids come from the configured `id` strategy (sequential/prefixed/random/ulid) and never collide with an existing id
- [ ] Files are renamed to match the new id; frontmatter and filename never disagree
- [ ] `dependencies:` references to a renumbered task are updated; ambiguous references are reported, not silently rewritten
- [ ] Every unmodelled frontmatter field round-trips verbatim; writes are atomic (temp+rename) and re-read before mutate
- [ ] Running twice in a row is a no-op the second time
- [ ] `karamd validate` reports duplicate ids as an error, pointing at `renumber` as the fix
- [ ] Pure core (grouping, winner selection, plan) is unit-tested; only the Vault I/O shell is thin. 100% line coverage holds

## Notes

Keep the decision logic pure and take the vault snapshot as input, like
`reschedule::plan`, so the collision/winner/plan math is tested without I/O.
