# Changelog

All notable changes to karamd are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.3.0]: https://github.com/PatrickLerner/karamd/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/PatrickLerner/karamd/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/PatrickLerner/karamd/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/PatrickLerner/karamd/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/PatrickLerner/karamd/releases/tag/v0.1.0
