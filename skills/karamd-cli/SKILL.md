---
name: karamd-cli
description: karamd CLI usage and its idempotency contract: the generate command and its flags, the task verbs (create/list/show/complete/cancel/reopen/status/next/validate/web), the query grammar, and machine output. Use when running karamd, scripting it, or explaining what a karamd command does or guarantees.
---

# karamd CLI

karamd is a Rust CLI for [taskmd](https://github.com/driangle/taskmd) markdown
vaults: recurring-task generation (taskmd has none) plus task verbs, a query
language, ranked next, validation, and a web UI. karamd's CLI surface is its
own; the **files it writes are fully taskmd-compatible (spec 1.2)**, so taskmd
and Obsidian keep working on the same vault. See the `taskmd-format` skill for
file details and `karamd-recurring` for the rules config.

## generate: recurring-task generation

```
karamd generate --vault /path/to/vault [--config FILE] [--dry-run] [--today YYYY-MM-DD]
```

- Reads a rules file, inspects existing tasks, creates a new task file **only
  when a rule is due**.
- **Idempotent**: re-running on the same day never creates duplicates. It reads
  task *state* each run (via the `recurring:` dedup marker and status/date
  fields) rather than emitting on a schedule.
- `--config` defaults to `<vault>/.taskmd.recurring.yaml`, so a rules file kept
  next to `.taskmd.yaml` needs no flag.
- `--dry-run` reports what would be created without writing.
- `--today YYYY-MM-DD` overrides the date (backfill or testing); defaults to the
  system date.
- `--vault` is **required** for `generate` so an unattended cron run can never
  silently target the wrong directory.

## Task verbs

Every verb below takes `--vault` (defaults to the current directory) and
supports `--json` / `--yaml` for machine/agent consumption (one serializable
model backs all output formats).

```
karamd create "Fix the flaky test" --priority high --type bug --tag ci
karamd create "Ship feature" --template feature --depends-on 008,011
karamd list                                  # table of all tasks
karamd list 'status:pending AND priority>=high'
karamd show 012                              # full task incl. body
karamd complete 012                          # solo: completed; pr-review: in-review
karamd complete 012 --pr https://github.com/o/r/pull/4
karamd cancel 013
karamd reopen 013                            # back to pending, timestamps cleared
karamd status 014 in-progress                # any of the six statuses
karamd next
karamd validate
karamd web
```

- `create --template` knows taskmd's built-ins (`feature`, `bug`, `chore`) and
  custom `.taskmd/templates/<name>.md` files. The configured id strategy
  (`sequential`, `prefixed`, `random`, `ulid`) is honored at create time.
- `complete` respects the `.taskmd.yaml` `workflow`: `solo` (default) sets
  `completed`; `pr-review` sets `in-review` and records `--pr`.
- Status changes auto-maintain `completed_at`/`cancelled_at` (set on entering,
  cleared on leaving), per the taskmd spec.
- The full status enum for `status`: `pending`, `in-progress`, `in-review`,
  `completed`, `blocked`, `cancelled`.

### next: ranked recommendations

```
karamd next [--limit N] [--quick-wins] [--critical] [--phase P] [--strict-phases]
```

A faithful port of taskmd's `next` scoring. Only *actionable* tasks are
recommended: `pending` or `in-progress`, every dependency `completed` (a
`cancelled` dependency blocks forever), no unresolved children. `--json`/`--yaml`
emit taskmd's recommendation shape for diffing.

### validate

```
karamd validate [--strict] [--json|--yaml]
```

Lints the vault against the taskmd spec. **Errors** (exit 1): malformed
frontmatter, missing `id`/`title`, invalid enum values, duplicate ids,
dependencies on nonexistent tasks, dependency cycles, parent defects. **Warnings**
(exit 0, or 2 under `--strict` for CI gating): a `phase` outside configured
phases, `touches` naming an unconfigured scope, missing `created_at`, filenames
off the `ID-slug.md` convention. Non-task files are never flagged.

### web

```
karamd web [--vault DIR] [--bind ADDR] [--web-dir DIR]
```

Serves a React SPA plus a JSON API over the vault. `--bind` defaults to
`127.0.0.1:8787` (loopback only; there is no app-level auth). `--web-dir` (or
`KARAMD_WEB_DIR`) points at the pre-built SPA bundle, default `dist`.

## Query grammar (for `list`)

```
term        := field OP value            e.g. status:pending, priority>=high
OP          := :  >=  >  <=  <           (ordering on priority, effort, dates)
combinators := AND, OR, NOT, ( ... )     case-insensitive; AND binds tighter
values      := bare or "quoted string"   e.g. title:"user auth"
```

Fields: `status`, `priority`, `effort`, `type`, `phase`, `tag`, `owner`,
`group`, `scope` (matches `touches`), `id`, `title` (case-insensitive
substring), `depends` (has that id as a dependency), `ready` (true/false: all
deps completed), and dates `created`, `completed`, `cancelled` (`YYYY-MM-DD`).
Missing `status` reads as `pending` and missing `priority` as `medium` (spec
defaults). A typo in a field or enum value is a parse error, not an empty result.

## Idempotency & safety contract

- The recurring *generator* only ever **adds** task files and **reads**
  completion state. Task verbs write state through a defensive store: atomic
  temp+rename saves, `create_new` for new files (never clobber), a fresh re-read
  before every mutation, id allocation at write time.
- Unknown frontmatter fields round-trip untouched and CRLF is tolerated, so
  karamd, taskmd, and Obsidian share a vault without eating each other's data.
- The vault is synced across devices by an external sync setup; karamd just
  reads and writes files in the synced directory. No git wrapper.
