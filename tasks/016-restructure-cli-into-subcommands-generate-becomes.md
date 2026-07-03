---
title: "Restructure CLI into subcommands (generate becomes one of many)"
id: "016"
status: completed
priority: high
type: feature
tags: ["work", "core"]
created_at: "2026-07-02"
dependencies: ["008"]
completed_at: 2026-07-02
---

# Restructure CLI into subcommands (generate becomes one of many)

## Objective

Turn karamd from a single-purpose recurring-task generator into a multi-command
CLI (`karamd <subcommand>`), so the new verbs (#011), `next` (#012), `validate`
(#015), and `web` (#009) hang off one clap command tree. The existing recurring
generator becomes the `generate` subcommand.

## Description

Foundational plumbing that #011/#012/#015/#009 all assume. Small but must settle
a back-compat decision first.

- **Back-compat decision (settle first):** today cron runs karamd as a bare
  invocation (`karamd --vault ... [--config ...]`) to generate. Options:
  (a) keep bare invocation meaning `generate` (no cron change; add subcommands
  alongside), or (b) require `karamd generate` and update the scheduled job.
  Recommend (a) — default to `generate` when no subcommand is given — to avoid
  breaking the unattended run. Document the choice.
- Introduce a clap subcommand tree: `generate` (existing logic moved verbatim),
  plus stubs the other tasks fill in (`list`, `next`, `validate`, `web`,
  `create`/`complete`/`cancel`/etc.). `src/main.rs` stays a thin shim.
- Shared global flags where sensible (`--vault`/`--task-dir`, `--config`) and
  consistent config/vault-root resolution across subcommands.
- Keep `karamd::run` as the entry point; move the current generate body behind
  the `generate` arm without behaviour change.

## Tasks

- [ ] Decide and document bare-invocation back-compat (recommend: defaults to
      `generate`)
- [ ] Add clap subcommand tree; move existing generator to `generate` arm
- [ ] Shared global flags + consistent config/vault-root resolution
- [ ] Placeholder arms for `list`/`next`/`validate`/`web`/task verbs
- [ ] TDD; keep 100% line coverage; recurring generation unchanged
- [ ] Update README + CLAUDE.md CLI section

## Acceptance Criteria

- `karamd generate` reproduces today's behaviour exactly; existing scheduled run
  keeps working (per the documented back-compat choice)
- Subcommand tree in place for later tasks to extend
- fmt, clippy, tests, and the 100%-line coverage gate all pass
