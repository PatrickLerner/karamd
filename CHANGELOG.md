# Changelog

All notable changes to karamd are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Web UI: a per-task **AI-runnable** toggle in the task detail view adds or
  removes the `ai-runnable` tag (the tag `karamd run` selects on) by re-sending
  the full tag set, so the task's other tags are never dropped. The button
  reflects the tagged state and annotates itself when `run.enabled` is off in the
  vault config. The `/api/config` response now includes `run_enabled` (#043).

## [0.7.0] - 2026-07-09

### Added

- Task execution mode: `karamd run` runs a configured AI agent (claude, opencode,
  or any CLI) against tasks explicitly tagged `ai-runnable`, to autonomously
  implement recurring chores (e.g. fetch from an MCP server and update a note).
  Off unless a `run:` section in `.taskmd.yaml` sets `enabled: true`; the command
  comes only from `run.agents` (a task may pick which named agent, never an
  arbitrary command). Attempts are tracked in `ai_attempts` and incremented
  *before* the spawn, so a crash still counts; at `max_attempts` the task is
  parked with an `ai-failed` tag and no longer selected. A run counts as success
  only on exit 0 **and** the task reaching a terminal status. `run --dry-run`
  lists what would run without spawning. Tasks execute sequentially (#039).
- Recurring rules accept an optional `frontmatter:` map, merged verbatim onto the
  generated task; `tags` merges with the rule's own tags and karamd-managed keys
  are rejected. This lets a rule emit an `ai-runnable` task, so `generate` and
  `run` compose end to end (#040).
- `next --runnable` ranks only the tasks `karamd run` would execute (tagged
  `ai-runnable`, attempts left, not parked), using the same selection predicate
  so there is no drift. The existing `tag:` / `open:` query terms also compose in
  `list` (e.g. `list "tag:ai-runnable AND open:true"`) (#041).
- `create` now refuses a second **open** task with the exact same title (trimmed,
  case-sensitive), naming the colliding task; `--force` bypasses it. Terminal
  (completed/cancelled) tasks never block a fresh one. `--json` emits the error
  as `{"error": ..., "existing_id": ...}` so scripted callers can branch. Stops
  agents that retry a `create` call from leaving duplicate open tasks (#038).

### Fixed

- `index.html` and the SPA deep-link fallbacks are now served `Cache-Control:
  no-store` instead of `no-cache`. Under a Nix store path every file's mtime is a
  constant `1970-01-01`, so `no-cache` revalidation returned `304` and served a
  stale entrypoint after an upgrade, breaking the SPA with a module MIME error
  until a hard refresh. Content-hashed assets stay `immutable` (#037).

## [0.6.1] - 2026-07-07

### Fixed

- Web asset filenames are now content-hashed (`main-<hash>.js`,
  `styles-<hash>.css`) and the server sends `Cache-Control`: hashed assets and
  fonts are immutable (cached for a year), while `index.html` revalidates. A
  deploy that changes the JS or CSS now gets a fresh URL and is picked up on a
  normal reload, instead of serving a stale bundle until a hard refresh.

## [0.6.0] - 2026-07-07

### Added

- New `nth_weekday` trigger: recur on the Nth (`week: 1`-`4`) or `last` weekday
  of the month, once per month (dedup marker `key:YYYY-MM`), e.g. "first Monday",
  "last Friday". Due on or after that date so a late run catches up within the
  month; an open task for the key blocks a second.
- `weekly`, `monthly`, and `nth_weekday` accept an optional `interval` (every Nth
  period, default 1) plus an optional `anchor` (`YYYY-MM-DD` on the cadence) for
  biweekly / every-other-month / quarterly schedules. `interval: 1` (or omitted)
  is a no-op, so existing rules are unchanged; omitting `anchor` aligns the
  cadence to a fixed epoch.
- The web Rules editor now supports every trigger and field: `day_of_week`
  (weekly, nth_weekday), `week` (nth_weekday), and `interval`/`anchor`.

### Changed

- Rule validation now rejects a trigger-specific field set on a rule whose
  trigger does not own it (e.g. `day_of_month` on a `weekly` rule), instead of
  silently ignoring it.

### Fixed

- The version in the web header is aligned as a baseline lockup next to the
  wordmark instead of floating at the bottom of the header bar.

## [0.5.0] - 2026-07-07

### Added

- New `weekly` recurring trigger: a task recurs on a fixed `day_of_week`
  (`mon`…`sun`), once per ISO week, with dedup marker `key:YYYY-Www`. It fires on
  or after that weekday within the week, so a run that misses the target day
  still catches up (Sat/Sun cover a missed Friday); a fully missed week is not
  backfilled. An open task for the key blocks a second, so there is never more
  than one at a time. Motivating case: a weekly review that must always land on a
  Friday. `day_of_week` accepts only the lowercase three-letter form; anything
  else fails `validate`.

## [0.4.0] - 2026-07-06

### Added

- Web "Today" tab grouping is config-driven. A new karamd-specific `web.today`
  list in `.taskmd.yaml` declares which phase ids the Today tab merges and in
  what order, so renaming a phase id no longer silently breaks the grouping.
  Omitting it keeps the previous default (`[ongoing, now]`); an explicit empty
  list merges only unphased open tasks. Served via `GET /api/config` (`today`).

## [0.3.0] - 2026-07-05

### Added

- First-class `due` date (`YYYY-MM-DD`): settable via `create --due` /
  `edit --due` and the web UI, shown in the detail view, and enforced
  (`validate` flags a malformed `due`; an empty string clears it). Every other
  unmodelled frontmatter field keeps round-tripping verbatim across all writes.
- `edit` verb: set any non-status field in place
  (`--title`/`--priority`/`--effort`/`--type`/`--phase`/`--due`/`--owner`/
  `--tag`/`--depends-on`/`--body`). An empty string clears a clearable field;
  terminal timestamps are never touched; dependency existence and cycles are
  checked up front.
- `open` query filter (`open:true`/`open:false`): match tasks whose status is
  not terminal (neither `completed` nor `cancelled`), so
  `karamd list 'open:true'` is the quick "still on my plate" view.

### Changed

- Web UI moved to TanStack Query for server state; status changes and edits
  invalidate the task list and counts so they update immediately without a
  reload.
- Recurring-rules web view redesigned as a collapsible list (compact rows, click
  a rule to edit it) instead of every rule rendered as an expanded form.
- Form placeholders show example values prefixed with `e.g.` and render clearly
  fainter than entered text.

### Fixed

- Web UI: completing or cancelling a task in the detail pane left it stale in the
  list and sidebar counts; it now drops out immediately.
- Web UI: the Today tab's group order was unstable (Ongoing sometimes rendered
  below This week); it is now deterministic regardless of the server's phase
  config.

## [0.2.0] - 2026-07-03

### Added

- Third trigger kind `monthly`: due `lead_days` before a fixed `day_of_month`,
  once per month (dedup marker `key:YYYY-MM`). Days 29-31 clamp to the month's
  last day so `31` fires in February too.
- taskmd library layer (`src/taskmd/`): full `.taskmd.yaml` config (phases, id
  strategies, workflow, scopes), complete task model with round-trip-safe
  frontmatter (unknown fields preserved, CRLF tolerated), auto
  `completed_at`/`cancelled_at` maintenance, atomic collision-safe writes, and
  the dependency/parent graph (readiness, cycles, dangling refs).
- Task verbs: `create` (with `--template` feature/bug/chore or custom
  `.taskmd/templates/`), `complete` (workflow-aware: pr-review sets
  `in-review` + `--pr`), `cancel`, `reopen`, `status`, `show`.
- `list` with a query mini-grammar (`field:value`, `>=`/`>`/`<=`/`<` on
  priority/effort/dates, `AND`/`OR`/`NOT`, parentheses) and `--json`/`--yaml`
  machine output backed by one serializable view.
- `next`: ranked recommendations, a faithful port of taskmd's algorithm
  (score-for-score verified), with a human view that also surfaces blocked
  high-priority tasks and the best unblocker.
- `validate`: lints the vault against the taskmd spec (errors exit 1; warnings
  exit 2 under `--strict` for CI gating).
- `web`: `karamd web` serves a React SPA (built with bun) plus a JSON API over
  the taskmd library, on `axum`/`tokio`. Binds `127.0.0.1` by default (tailnet
  is the security boundary). Nix bundles the frontend and binary in one closure
  (`packages.karamd-web`). Manage recurring rules from the UI too, with a
  dry-run preview (`GET/PUT /api/rules`, `POST /api/rules/preview`).
- Embedded terminal: a task's **Run with Claude** opens an xterm.js session
  wired over `GET /api/tasks/{id}/run` to a process spawned in a PTY (cwd = the
  vault), seeded with the task's context. Command is configurable via
  `--run-command` / `KARAMD_RUN_COMMAND` (default `claude`).
- Persistent server-side sessions (one per task): a Claude run outlives its
  socket, so closing the tab keeps it alive; reattaching replays scrollback and
  reconnects to the live stream. Killed only explicitly (`DELETE
  /api/sessions/{id}`, from the sidebar) or on server shutdown. `GET
  /api/sessions` lists them.
- `graph` (Graphviz DOT of the dependency graph) and `stats` (counts by
  status/priority/phase, ready/blocked/invalid).

### Changed

- Web UI redesigned around a paper aesthetic and an aligned three-pane shell
  (nav | list | detail, collapsing to a drawer + single pane on narrow
  screens). Navigation is by phase view (Today / Next week / Later / Done) with
  reload-safe nested URLs (`#/view/<tab>/task/<id>`), not by status filter.

## [0.1.2] - 2026-07-02

### Added

- Rules can carry an optional `body:` (markdown) for the generated task,
  replacing the default `TODO` stub. Omit it to keep the stub. karamd always
  writes the frontmatter, `# <title>` heading, and provenance comment; the body
  is everything after that.

## [0.1.1] - 2026-07-01

### Added

- `--config` now defaults to `<vault>/.taskmd.recurring.yaml` when omitted, so a
  rules file kept next to `.taskmd.yaml` needs no flag and unattended runs pass
  only `--vault`.

## [0.1.0] - 2026-07-01

### Added

- Initial release: `generate` command with `after_completion` and `calendar`
  triggers, idempotent creation via a `recurring:` frontmatter marker, `--dry-run`
  and `--today` overrides, Nix flake, and CI.

[0.7.0]: https://github.com/PatrickLerner/karamd/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/PatrickLerner/karamd/compare/v0.6.0...v0.6.1
[0.3.0]: https://github.com/PatrickLerner/karamd/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/PatrickLerner/karamd/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/PatrickLerner/karamd/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/PatrickLerner/karamd/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/PatrickLerner/karamd/releases/tag/v0.1.0
