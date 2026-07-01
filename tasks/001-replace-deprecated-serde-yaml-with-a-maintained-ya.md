---
title: "Replace deprecated serde_yaml with a maintained YAML crate"
id: "001"
status: completed
priority: medium
type: chore
tags: ["core"]
created_at: "2026-07-01"
completed_at: 2026-07-01
---

# Replace deprecated serde_yaml with a maintained YAML crate

## Description

`serde_yaml` 0.9 is unmaintained (archived by dtolnay). karamd uses it in two
places: `rule::load_rules` (parse the rules file) and `task.rs` (parse task
frontmatter + `.taskmd.yaml`). Move to a maintained drop-in before it bit-rots
or picks up an advisory. Candidates: `serde_yml` (community fork) or
`serde_norway`. Keep the dependency set small and the public API unchanged.

## Tasks

- [ ] Pick a maintained crate; check it round-trips our frontmatter and rules
- [ ] Swap the dependency and the two call sites; delete `serde_yaml`
- [ ] Confirm `cargo test` + 100% line coverage still hold
- [ ] Re-run the `taskmd list` integration check on a generated file

## Acceptance Criteria

- No `serde_yaml` in `Cargo.lock`
- Rules and frontmatter parse identically to before
- CI stays green (fmt, clippy deny, tests, coverage)
