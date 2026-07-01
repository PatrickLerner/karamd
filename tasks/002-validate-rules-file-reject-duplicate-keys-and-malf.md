---
title: "Validate rules file: reject duplicate keys and malformed annual dates"
id: "002"
status: completed
priority: medium
type: feature
tags: ["core"]
created_at: "2026-07-01"
completed_at: 2026-07-01
---

# Validate rules file: reject duplicate keys and malformed annual dates

## Objective

Catch two classes of bad rules at load time instead of silently misbehaving or
failing mid-run.

## Description

`Rule::validate` currently only checks that a trigger's required fields are
present. Two gaps surfaced while building the core loop:

1. **Duplicate `key`s** across rules silently share a dedup marker. Two
   after_completion rules with the same key would interfere (one blocks the
   other). Keys must be unique within a rules file.
2. **Malformed `annual`** (e.g. `"99-99"`) passes `validate` and only fails deep
   inside `calendar_due` at run time. Validate the `MM-DD` format up front so a
   typo fails loudly before any task is scanned.

## Tasks

- [ ] Add a whole-file check for duplicate keys (report the offending key)
- [ ] Parse/validate `annual` as `MM-DD` in `validate` (reuse
      `due::calendar_occurrence` logic)
- [ ] Unit-test: duplicate keys rejected, bad annual rejected, good file passes

## Acceptance Criteria

- A rules file with duplicate keys errors before generation
- A calendar rule with a malformed `annual` errors at load, not mid-run
- Existing valid rules files still load
