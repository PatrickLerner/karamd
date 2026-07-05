This is **karamd**, a Rust CLI for taskmd markdown vaults. It began as a
recurring-task generator (taskmd has no recurrence) and is growing into a
general taskmd tool: task verbs, query, ranked next, validate, web UI
(#008–#016 in `tasks/`).

## What it is

- **Recurring generator** (`karamd generate`): reads a rules file, inspects
  existing task files, creates a new task only when a rule is due. Must be
  **idempotent** — re-running on the same day never duplicates. Dedup marker:
  `recurring: <key>` frontmatter on generated tasks.
- Three triggers: `after_completion` (N days after the last one was completed),
  `calendar` (lead_days before a fixed annual date, once per year), and
  `monthly` (lead_days before a fixed day of the month, once per month; marker
  `key:YYYY-MM`; day 29-31 clamps to the month's last day; lead_days 0-27). The
  code reads task *state* each run, never blindly emits on a schedule.
- **Design shift (settled in #008):** the *generator* still only adds files and
  reads completion state, but karamd as a whole is now a first-class taskmd
  writer via the `src/taskmd/` library layer (verbs, web UI edits). Completions
  can happen in taskmd/Obsidian *or* karamd. The vault is kept in sync across
  devices by an external sync setup, not by karamd — no git/pull/commit/push
  wrapper. All writes are defensive against concurrent sync (atomic temp+rename,
  `create_new` for new files, re-read before mutate, id allocation at write
  time).
- **Compatibility contract:** karamd's CLI surface may differ from taskmd's, but
  every file written must match the taskmd spec (1.2, taskmd 0.2.5; regenerate
  with `taskmd spec --stdout`). Unknown frontmatter fields are preserved
  verbatim; CRLF tolerated.

## Layout

Split into a library crate (all logic, unit-testable) plus a thin binary:

- `src/main.rs` — three-line shim; calls `karamd::run(args)`.
- `src/lib.rs` — CLI (clap subcommand tree), `run`, the `generate`
  orchestration, and the per-trigger `decide` step. `Report`/`Created` describe
  what a run did.
- `src/rule.rs` — `Rule` model, `Trigger` enum, `load_rules`, `Rule::validate`,
  and `validate_all` (whole-file check: unique keys + well-formed `annual`).
- `src/due.rs` — pure due-checks (`after_completion_due`, `calendar_due`,
  `calendar_occurrence`). Every fn takes `today: NaiveDate` so tests never touch
  the clock.
- `src/task.rs` — the generator's own thin scanner (`scan_dir`, `tasks_dir`),
  `slugify`, `next_id`, frontmatter parsing, and `render_task`.
- `src/taskmd/` — the reusable taskmd library layer (#008):
  - `config.rs` — full `.taskmd.yaml`: `dir`, `phases`, `id` strategy
    (sequential/prefixed/random/ulid; prefixed emits `dr001`, **no separator** —
    verified against taskmd 0.2.5, the spec doc's `dr-001` is wrong), `workflow`
    (solo/pr-review), `scopes`.
  - `model.rs` — `Task` backed by its complete frontmatter as an ordered YAML
    mapping, so unknown fields round-trip untouched. Spec enums (`completed`,
    not `done`; hyphenated `in-progress`/`in-review`). Auto set/clear of
    `completed_at`/`cancelled_at` on status change. `created` alias accepted.
    First-class `due` (YYYY-MM-DD target date): `due_raw`/`due`/`set_due`,
    settable via `create`/`edit`/web, `YYYY-MM-DD` enforced by `validate` and the
    verbs; every other unmodelled field is preserved verbatim across all writes.
  - `store.rs` — `Vault`: recursive scan (dir-derived groups; non-task files
    ignored, broken task files reported separately), atomic saves, collision-
    safe `create`, re-reading `update`. Entropy is injectable (`Entropy` trait)
    so id-generation tests are deterministic.
  - `graph.rs` — dependency graph: readiness (all deps `completed`; a
    `cancelled` dep blocks, matching taskmd), blockers, transitive downstream
    count/depth, cycle + dangling detection, `parent` hierarchy validation
    (exists / no self-ref / no cycles).
- CLI verbs + views over the library: `src/verbs.rs` (create/edit/list/show/
  status/complete/search), `src/query.rs` (the `list` mini-grammar, incl. the
  `open:` filter = status not terminal), `src/next.rs`
  (taskmd-parity ranking), `src/validate.rs` (spec lint), `src/analyze.rs`
  (`graph` DOT + `stats`), `src/output.rs` (one `TaskView` behind human/JSON/YAML).
- Web (#009/#013): `src/web.rs` — axum JSON API over the library (`karamd web`,
  `--bind`/`--web-dir`/`--run-command`), served alongside the bun-built SPA in
  `web/`. Embedded terminal (#010): `src/terminal.rs` (pure prompt-seeding +
  argv parsing, tested) and `src/web_terminal.rs` (the PTY + WebSocket glue,
  excluded from coverage).
- `recurring.example.yml` — rule format reference.

Core logic keeps I/O thin and functions pure so the suite hits **100% line
coverage** (`cargo llvm-cov --ignore-filename-regex 'src/(main|web_terminal)\.rs'`).
Two files are excluded as untestable process/network glue: `src/main.rs` (the
binary shim) and `src/web_terminal.rs` (the PTY + WebSocket bridge for the
embedded terminal, whose pure logic lives in the covered `src/terminal.rs`). TDD:
write the test, watch it fail, implement.

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
- **Per-rule body**: chose Option A — a single optional `body:` field (free
  markdown) over structured `objective/tasks/acceptance` fields. When present it
  replaces the `TODO` stub verbatim; when absent the stub is emitted unchanged so
  existing rules do not regress. Regardless of `body`, karamd always writes the
  frontmatter, the `# <title>` heading, and the `<!-- Generated by karamd ... -->`
  provenance comment. An empty/whitespace-only `body` is rejected by
  `Rule::validate` (worse than the stub).

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

Never hand-write or hand-edit task `.md` files with an editor. Create and modify
them through the CLI tool — `taskmd add` / `taskmd set` (and `karamd` once it can,
per #011/#015) — so files are always spec-valid. Run `taskmd validate` after
changes.

Any follow-up or bug found mid-work that is not fixed immediately becomes its own
taskmd task in `tasks/` (don't leave it only in a commit message, a code comment,
or the conversation). Link dependencies via the `dependencies:` frontmatter. This
is a public repo: never put secrets or personal paths/identifiers in task files.
