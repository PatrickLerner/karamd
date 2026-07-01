This is **karamd**, a Rust CLI that generates recurring tasks for a taskmd
markdown vault. taskmd has no recurrence; karamd is the layer that adds it.

## What it is

- Reads a rules file, inspects existing task files, creates a new task only when
  a rule is due. Must be **idempotent** — re-running on the same day never
  duplicates. Dedup marker: `recurring: <key>` frontmatter on generated tasks.
- Two triggers: `after_completion` (N days after the last one was completed) and
  `calendar` (lead_days before a fixed annual date, once per year). The code
  reads task *state* each run, never blindly emits on a schedule.
- karamd only **adds** files and **reads** completion state. Completions happen
  elsewhere (in taskmd/Obsidian), not in karamd. The vault is kept in sync across
  devices by an external sync setup, not by karamd — karamd just writes new task
  files into the synced dir; there is no git/pull/commit/push wrapper.

## Layout

Split into a library crate (all logic, unit-testable) plus a thin binary:

- `src/main.rs` — three-line shim; calls `karamd::run(args)`.
- `src/lib.rs` — CLI (clap), `run`, the `generate` orchestration, and the
  per-trigger `decide` step. `Report`/`Created` describe what a run did.
- `src/rule.rs` — `Rule` model, `Trigger` enum, `load_rules`, `Rule::validate`,
  and `validate_all` (whole-file check: unique keys + well-formed `annual`).
- `src/due.rs` — pure due-checks (`after_completion_due`, `calendar_due`,
  `calendar_occurrence`). Every fn takes `today: NaiveDate` so tests never touch
  the clock.
- `src/task.rs` — vault scanning (`scan_dir`, `tasks_dir`), `slugify`, `next_id`,
  frontmatter parsing, and `render_task`.
- `recurring.example.yml` — rule format reference.

Core logic keeps I/O thin and functions pure so the suite hits **100% line
coverage** (`cargo llvm-cov --ignore-filename-regex 'src/main.rs'`); the binary
shim is excluded. TDD: write the test, watch it fail, implement.

## taskmd frontmatter to emit (match taskmd's own output)

```
id: "NNN"          # zero-padded, next after scanning tasks dir
title: "..."
status: pending
priority: medium
phase: next         # optional
dependencies: []
tags: [...]
created_at: YYYY-MM-DD
recurring: <marker>  # karamd's dedup marker (see below)
```

Slug rule: lowercase, non-`[a-z0-9]` → `-`, collapse/trim; non-ASCII letters
dropped ("prüfen" → "pr-fen"). Covered by a unit test — keep it green.

## Design decisions (settled)

- **Default config path**: `--config` is optional; when omitted it resolves to
  `<vault>/.taskmd.recurring.yaml` (const `DEFAULT_CONFIG`). Keeps the rules file
  next to `.taskmd.yaml` so unattended runs need only `--vault`.
- **Completion date source**: taskmd stamps `completed_at: YYYY-MM-DD` in
  frontmatter on `set --done`, and it *preserves* our custom `recurring:` field
  across edits. So the after_completion interval reads `completed_at` directly —
  no git archaeology or mtime. Verified against taskmd 0.2.5.
- **Dedup marker format**: after_completion writes `recurring: <key>`; calendar
  writes `recurring: "<key>:<year>"`. The year is what makes "once per year"
  hold even if the task is completed early inside its lead window. Grouping a
  rule's tasks matches the key (after_completion) or the `<key>:` prefix
  (calendar).
- **after_completion**: an *open* task for the key blocks re-creation; otherwise
  due when `today - last_occurrence >= every_days`, where `last_occurrence` is
  the most recent terminal task's `completed_at`, else `cancelled_at`, else
  `created_at`. Never-run keys are due. **Cancelling** an occurrence keeps the
  series: the next one schedules `every_days` after `cancelled_at`, not
  immediately. The `created_at` fallback stops an undated terminal task from
  looking like "never ran" and re-firing every run.
- **CRLF**: frontmatter parsing tolerates `\r\n`; a synced vault can pick up CRLF
  and an LF-only parser would drop the `recurring:` marker and duplicate tasks.
- **calendar**: due when `0 <= (occurrence - today) <= lead_days`, checking this
  year and next year (so a window straddling Jan 1 resolves to next year).
- **Leap day**: `annual: "02-29"` clamps to `02-28` in common years so the rule
  still fires yearly.

## Conventions

- Rust, edition 2024. Prefer std; keep the dependency set small.
- Linters gate everything: `cargo fmt --all -- --check`, `cargo clippy
  --all-targets --all-features` (clippy `all = deny`, `unsafe_code = forbid`),
  `cargo test`, and the 100%-line coverage check. All run in GitHub Actions
  (`.github/workflows/ci.yml`).
- No em-dashes in prose/commit messages. No "Co-Authored-By" / AI mentions in
  commits. Do not commit unless asked.
- Intended to run unattended on a schedule (e.g. cron) against the synced vault.
  Packaging and scheduling are environment-specific and live outside this repo.

## This repo's own tasks

Managed with taskmd in `tasks/` (MCP via the `taskmd-mcp` plugin, enabled in
`.claude/settings.json`). Prefer the taskmd MCP tools. Fill task templates fully.
