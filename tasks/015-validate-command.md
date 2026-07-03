---
title: "Validate command: lint tasks against the taskmd spec"
id: "015"
status: completed
priority: high
type: feature
tags: ["work", "core"]
dependencies: ["008", "011"]
created_at: "2026-07-02"
completed_at: 2026-07-02
---

# Validate command: lint tasks against the taskmd spec

## Objective

A `karamd validate` command that lints the vault against the taskmd spec and
reports problems. This is what guarantees karamd never emits (and can catch)
files taskmd would reject. Surfaces #008's validation primitives as a command.

## Description

Builds on #008 (which already computes enums, uniqueness, dependency graph,
parent graph) and #011's output formats. Distinguish **errors** from
**warnings**, matching taskmd's exit codes:

- `0` success, `1` error, `2` validation warnings under `--strict`.

Errors (invalid files):

- Missing required `id` / `title`; malformed frontmatter.
- Invalid enum value for `status` / `priority` / `effort` / `type`.
- Duplicate `id` across the project.
- `dependencies` referencing a non-existent task (dangling ref).
- Dependency cycle.
- `parent` referencing a non-existent task, self-reference, or parent cycle.

Warnings:

- `phase` not defined in `.taskmd.yaml` phases (when phases configured).
- `touches` scope not defined in `scopes` (when scopes configured).
- Missing `created_at`; filename not matching `ID-slug.md`.

Behavior:

- Report per file: path, id, severity, message. Human-readable by default;
  `--json`/`--yaml` via #011's serializer for CI/AI.
- Non-task files (spec doc, README, templates, fenced yaml) are skipped, not
  flagged (see #008 scanner rule).
- `--strict` makes warnings exit non-zero (code 2) for CI gating.

## Tasks

- [ ] `validate` command over the vault, reusing #008 validation
- [ ] Error vs warning classification with taskmd-compatible exit codes
- [ ] Human + `--json`/`--yaml` output; `--strict` flag
- [ ] Skip non-task files correctly
- [ ] Document in README; consider wiring into CI
- [ ] TDD throughout; keep 100% line coverage

## Acceptance Criteria

- Detects all listed errors and warnings with correct severity
- Exit codes: 0 clean, 1 on error, 2 on warnings under `--strict`
- Output available human-readable and as JSON/YAML
- Non-task files are not falsely flagged
- fmt, clippy, tests, and the coverage gate all pass
