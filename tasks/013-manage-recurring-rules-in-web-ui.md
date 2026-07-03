---
title: "Manage recurring rules from the web UI"
id: "013"
status: completed
priority: low
type: feature
tags: ["work", "web"]
dependencies: ["009"]
created_at: "2026-07-02"
completed_at: 2026-07-02
---

# Manage recurring rules from the web UI

## Objective

View and edit the recurring-rules file (`.taskmd.recurring.yaml`) from the web UI
(#009), and preview what a generation run would create, so recurrence is
manageable from the same interface as tasks.

## Description

karamd already owns recurrence (the original generator + rule model in
`src/rule.rs`, `src/due.rs`). This surfaces it in the web UI (#009):

- List existing rules with their trigger (`after_completion` / `calendar`) and
  next-due state.
- Add/edit/remove a rule, validating with the existing `Rule::validate` /
  `validate_all` before writing.
- **Dry-run preview:** show what a generation run would create today without
  writing files (reuse the existing decide/generate logic in read-only mode).
- Writes to `.taskmd.recurring.yaml` go through the same safe-write path as
  tasks (#008): atomic, preserve formatting where feasible.

## Tasks

- [x] API + views to list/add/edit/remove recurring rules (validated)
- [x] Dry-run preview of a generation run (no files written)
- [x] Reuse `Rule::validate`/`validate_all`; safe atomic write of the rules file
- [x] Mobile-friendly, consistent with #009 styling
- [x] TDD for the API/handlers; keep coverage gate green

## Acceptance Criteria

- Rules can be viewed, added, edited, removed from the web UI, with validation
- Dry-run preview matches what a real run would generate, writing nothing
- Rules file stays valid and re-parseable after edits
- fmt, clippy, tests, and the coverage gate all pass
