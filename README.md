# karamd

A Rust CLI for [taskmd](https://github.com/driangle/taskmd) markdown vaults:
recurring-task generation (taskmd has none), plus task verbs, a query language,
and machine-readable output. karamd's CLI is its own; the *files* it writes are
fully taskmd-compatible (spec 1.2), so taskmd and Obsidian keep working on the
same vault.

`kar` (کار) = work/task, `md` = markdown. Named after taskmd; may replace it
one day, not yet.

## Claude Code skills

This repo doubles as a Claude Code plugin marketplace. It ships one plugin,
`karamd-formats`, with four skills documenting the formats and workflow so any
Claude session can read, write, and validate a vault, file tasks, and author
recurring rules without rediscovering them:

- `taskmd-format`: the taskmd task-file format. Frontmatter fields and enums,
  id strategies, slug/filename rules, `.taskmd.yaml` config, validation.
- `karamd-recurring`: the `.taskmd.recurring.yaml` rules config. The three
  triggers, optional `body`, and the dedup markers karamd stamps.
- `karamd-cli`: karamd CLI usage and its idempotency contract.
- `filing-tasks`: how to file a task or follow-up with `karamd create` — the
  flags that matter and the state verbs, keyed to auto-load on filing intent.

Install from Claude Code:

```
/plugin marketplace add PatrickLerner/karamd
/plugin install karamd-formats@karamd
```

Layout: `.claude-plugin/marketplace.json` (marketplace manifest) +
`.claude-plugin/plugin.json` (plugin manifest) + `skills/<name>/SKILL.md`
(auto-discovered). The `.claude/skills/taskmd-format/` draft is this repo's own
local skill and stays in place; the plugin copy under `skills/` is the
published one.

**Keep the skills in sync**: the skill content is derived from this repo's code
and the taskmd spec. When the taskmd spec version changes (regenerate with
`taskmd spec --stdout`) or karamd's CLI surface changes, update the matching
`skills/<name>/SKILL.md` in the same change.

### opencode

The same four `skills/<name>/SKILL.md` files are valid
[opencode skills](https://opencode.ai/docs/skills/) unchanged — the format is
identical (`SKILL.md` + `name`/`description` frontmatter, loaded on demand).
opencode does not read Claude plugin marketplaces, so expose them through one of
its skill-discovery directories. Symlink each skill folder into the global
opencode skills dir (do **not** use the `skills.paths` config key — it shows up
in `opencode debug skill` but is not handed to the agent's `skill` tool):

```sh
mkdir -p ~/.config/opencode/skills
for s in karamd-cli karamd-recurring taskmd-format filing-tasks; do
  ln -sfn "$PWD/skills/$s" ~/.config/opencode/skills/"$s"
done
```

Symlinks keep the single `skills/` tree as the source of truth for both tools —
no copies to sync. Verify the agent actually sees them (not just discovery):

```sh
opencode run 'List the names of every skill available to you.'
```

The four names must appear. `opencode run` starts a fresh server each time, so
it reflects changes immediately.

**Gotcha:** the opencode TUI runs a per-project server that scans skills **once
at startup** — no hot reload. After adding or changing skills, closing the TUI
window is not enough; fully kill the process (`pkill -f opencode`) and relaunch,
or the old skill list persists. (This, not symlinks, is why a freshly added
skill can appear via `opencode run`/`opencode debug skill` yet be missing in an
already-open TUI.)

For clone-free remote distribution, publish the skills under a
`.well-known/skills/` endpoint and use `"skills": { "urls": [...] }` in
`~/.config/opencode/opencode.jsonc` instead.

## Recurring generation

Reads a rules file, inspects existing tasks, and creates a new task file only
when a rule is due. Idempotent: running it repeatedly on the same day never
creates duplicates. Dedup is a `recurring: <key>` frontmatter field on every
generated task.

Five trigger kinds:

- **after_completion** — next task appears N days after the *last one was
  completed* (interval-from-completion, e.g. a periodic check-in every ~18d).
  Self-healing: a missed run just catches up next time.
- **calendar** — task appears `lead_days` before a fixed annual date, once per
  year (e.g. a birthday, with lead time to buy a present).
- **monthly** — task appears `lead_days` before a fixed `day_of_month`, once
  per month (dedup marker `key:YYYY-MM`, e.g. a bill due on the 12th surfaced a
  week early). Days 29-31 clamp to the month's last day, so `31` still fires in
  February; `lead_days` is limited to 0-27 so windows never overlap.
- **weekly** — task appears on a fixed `day_of_week` (`mon`…`sun`), once per ISO
  week (dedup marker `key:YYYY-Www`, e.g. a LinkedIn review every Friday). Runs
  on or after that weekday catch up within the same week (Sat/Sun still fire a
  missed Friday); a fully missed week is not backfilled. An open task for the
  key blocks a second one, so there is never more than one at a time.
- **nth_weekday** — task appears on the Nth (`week: 1`-`4`) or `last` weekday of
  the month, once per month (dedup marker `key:YYYY-MM`, e.g. first Monday, last
  Friday). Due on or after that date so a late run catches up within the month;
  an open task for the key blocks a second.

These are genuinely different triggers. karamd reads task *state* each run
rather than blindly emitting on a schedule, so none piles up duplicates.

`weekly`, `monthly`, and `nth_weekday` accept an optional `interval` (every Nth
period, e.g. `interval: 2` for biweekly or every other month) plus an optional
`anchor` (a `YYYY-MM-DD` date on the desired cadence). Omitting `interval` (or
`interval: 1`) means every period; omitting `anchor` aligns the cadence to a
fixed epoch, so it stays deterministic even without one.

A rule may carry an optional `body:` (markdown) that replaces the default
`TODO` stub in the generated task. karamd always writes the frontmatter, the
`# <title>` heading, and a provenance comment; the body is everything after
that. Omit `body:` to keep the stub.

```
karamd generate --vault /path/to/vault [--config FILE] [--dry-run] [--today YYYY-MM-DD]
```

`--config` defaults to `<vault>/.taskmd.recurring.yaml`, so a rules file kept
next to `.taskmd.yaml` needs no flag. `--dry-run` reports what would be created
without writing. `--today` overrides the date (for backfill or testing); it
defaults to the system date. `generate` requires an explicit `--vault` so an
unattended cron run can never silently target the wrong directory.

See `recurring.example.yml` for the rule format.

## Task commands

Every other command takes `--vault` too but defaults it to the current
directory, and supports `--json` / `--yaml` for machines and AI agents (one
serializable model backs all formats).

```
karamd create "Fix the flaky test" --priority high --type bug --tag ci
karamd create "Ship feature" --template feature --depends-on 008,011 --due 2026-08-01
karamd edit 012 --priority high --due 2026-08-01 --depends-on 008
karamd edit 012 --due "" --phase ""      # empty string clears the field
karamd list                              # table of all tasks
karamd list 'open:true'                  # everything not completed/cancelled
karamd list 'status:pending AND priority>=high'
karamd show 012                          # full task incl. body
karamd complete 012                      # solo: completed; pr-review: in-review
karamd complete 012 --pr https://github.com/o/r/pull/4
karamd cancel 013
karamd reopen 013                        # back to pending, timestamps cleared
karamd status 014 in-progress            # full enum: pending, in-progress,
                                         # in-review, completed, blocked, cancelled
```

- `create --template` knows taskmd's built-ins (`feature`, `bug`, `chore`,
  byte-matched against taskmd 0.2.5) and custom `.taskmd/templates/<name>.md`
  files (frontmatter = field defaults, body = task body).
- `edit` sets any non-status field in place (`--title`/`--priority`/`--effort`/
  `--type`/`--phase`/`--due`/`--owner`/`--tag`/`--depends-on`/`--body`); an empty
  string clears the clearable fields, terminal timestamps are never touched, and
  arbitrary/unknown frontmatter is preserved. Status still moves through
  `complete`/`cancel`/`reopen`/`status`.
- `complete` respects the `.taskmd.yaml` `workflow`: `solo` (default) sets
  `completed`; `pr-review` sets `in-review` and records `--pr`.
- Status changes maintain `completed_at`/`cancelled_at` automatically (set on
  entering the status, cleared on leaving, per the taskmd spec).
- The configured id strategy (`sequential`, `prefixed`, `random`, `ulid`) is
  honored when creating tasks.

### Ranked next task

```
karamd next [--limit N] [--quick-wins] [--critical] [--phase P] [--strict-phases]
```

A faithful port of taskmd's `next` algorithm (verified score-for-score against
the 0.2.5 binary; `scripts/next-parity.sh` re-checks it). Only *actionable*
tasks are recommended: explicitly `pending` or `in-progress`, every dependency
`completed` (a `cancelled` dependency blocks forever — it never completes),
and no unresolved children. Scoring: priority base (40/30/20/10) +
critical-path bonus (15) + downstream bonus (3 per unblocked task, cap 15),
the last two scaled by the most important priority found downstream
(critical/high x1, medium x0.5, else x0.25), + effort (small +5 "quick win",
medium +2) + phase bonus (25 - 5 x position in the configured phase order).
Ties break by id.

The human output adds what taskmd does not show: blocked high-priority tasks
with the open dependencies holding them, and which recommendation unblocks the
most downstream work. `--json`/`--yaml` emit exactly taskmd's recommendation
shape for diffing.

### Validate

```
karamd validate [--strict] [--json|--yaml]
```

Lints the vault against the taskmd spec. **Errors** (exit 1): malformed
frontmatter, missing `id`/`title`, invalid enum values, duplicate ids,
dependencies on nonexistent tasks, dependency cycles, and parent defects
(missing, self-referencing, cyclic). **Warnings** (exit 0, or 2 under
`--strict` for CI gating): a `phase` not in the configured phases, `touches`
naming an unconfigured scope (both only checked when configured), missing
`created_at`, and filenames off the `ID-slug.md` convention. Non-task files
(READMEs, spec docs, templates, fenced yaml examples) are never flagged. This
repo's CI validates its own `tasks/` with `karamd validate --strict`.

### Search, graph, stats

Read-only views over the same model (all support `--json` / `--yaml`):

```
karamd search "login"    # full-text (case-insensitive) over titles and bodies
karamd graph             # Graphviz DOT; pipe to `dot -Tsvg`. --json/--yaml = nodes+edges
karamd stats             # counts by status/priority/phase, plus ready/blocked
```

`graph` edges run dependency to dependent, so arrows follow the flow of
unblocking. Other spec commands (`board`, `snapshot`, `verify`, `worklog`,
`archive`, and more) are catalogued in `tasks/014` and implemented on demand.

### Query language

`list` takes a query expression:

```
term        := field OP value            e.g. status:pending, priority>=high
OP          := :  >=  >  <=  <           (ordering on priority, effort, dates)
combinators := AND, OR, NOT, ( ... )     case-insensitive; AND binds tighter
values      := bare or "quoted string"   e.g. title:"user auth"
```

Fields: `status`, `priority`, `effort`, `type`, `phase`, `tag`, `owner`,
`group`, `scope` (matches `touches`), `id`, `title` (case-insensitive
substring), `depends` (has that id as a dependency), `ready` (true/false: all
dependencies completed), `open` (true/false: status is not `completed` or
`cancelled`), and the dates `created`, `completed`, `cancelled`
(`YYYY-MM-DD`). Missing `status` reads as `pending` and missing `priority` as
`medium` (spec defaults); a typo in a field or enum value is a parse error, not
an empty result.

## Web UI

`karamd web` serves a small React SPA plus a JSON API over the vault, built on
the same taskmd library the CLI uses (writes stay spec-compatible, custom fields
preserved).

```
karamd web [--vault DIR] [--bind ADDR] [--web-dir DIR]
```

- `--bind` defaults to `127.0.0.1:8787` (loopback only). Reach it from other
  devices by binding a Tailscale IP or `0.0.0.0` and letting the **tailnet +
  Tailscale ACLs** be the security boundary; there is no app-level auth, so
  never bind a public interface directly. Prefer `tailscale serve` for TLS
  rather than terminating it in karamd.
- `--web-dir` (or the `KARAMD_WEB_DIR` env var) points at the pre-built SPA
  bundle; it defaults to `dist`. The backend is async (axum on tokio) and
  WebSocket-capable.

### Today tab grouping

The dashboard's default **Today** tab merges several phases into one view (plus
any unphased open task). Which phases, and their render order, is config-driven,
not hardcoded, so renaming a phase id never silently breaks the grouping. Set it
in a karamd-specific `web:` section of `.taskmd.yaml` (taskmd ignores the
section):

```yaml
web:
  today: [ongoing, now]   # phase ids merged into Today, in this order
```

Omit `web.today` and it defaults to `[ongoing, now]` (the previous behavior). An
explicit empty list (`today: []`) merges no named phase, so only unphased open
tasks fall into Today. Each remaining phase gets its own tab, and terminal tasks
collect in **Done**.

### Run a task with an AI (embedded terminal)

A task's detail page has a **Run with Claude** button. It opens an embedded
xterm.js terminal wired over a WebSocket (`GET /api/tasks/{id}/run`) to a process
spawned in a PTY, working directory = the vault. The session is seeded with the
task's id, title, and body as the initial input (not auto-submitted — you review
and press enter).

```
karamd web --vault /path/to/vault --web-dir web/dist --run-command claude
```

- `--run-command` (or `KARAMD_RUN_COMMAND`) is what gets spawned; it defaults to
  `claude`. Set it to any command (e.g. a wrapper) for testing.
- With `run.agents` configured you can pick a specific agent instead. The seeded
  prompt is passed as the program's final positional argument (`claude
  "<prompt>"`). A TUI that treats a positional as a **directory** — opencode does
  — needs the prompt behind a flag instead; set `terminal_prompt_flag` on that
  agent so it launches as `opencode --prompt "<prompt>"`:

  ```yaml
  run:
    agents:
      opencode:
        command: [opencode]
        terminal_prompt_flag: "--prompt"
  ```
- **Safety:** this spawns a real process that can edit the vault/project, over
  the same no-auth server. The tailnet + Tailscale ACLs are the only boundary,
  so never expose `karamd web` on a public interface. Launch is always explicit
  (per task, on button press).

The bundle is built with **bun**, a separate step from the Rust build. The
`build` script (`web/build.ts`) bundles with `NODE_ENV=production` so React's
production JSX runtime is used, and content-hashes the JS/CSS filenames so
deploys are picked up without a manual hard refresh (the server caches hashed
assets immutably, revalidates `index.html`):

```
cd web && bun install && bun run build      # produces web/dist
karamd web --vault /path/to/vault --web-dir web/dist
```

All assets (JS/CSS/font) are self-hosted from the bundle; there is no runtime
network or CDN dependency. The API: `GET /api/tasks`, `GET /api/tasks/{id}`,
`POST /api/tasks`, `PATCH /api/tasks/{id}`, `POST /api/tasks/{id}/status`,
`GET /api/config`, `GET /api/next?limit=N`, `GET/PUT /api/rules`,
`POST /api/rules/preview`, and the `GET /api/tasks/{id}/run` WebSocket; failures
return a non-2xx `{ "error": ... }`.

## Design

- The recurring *generator* only ever **adds** task files and **reads**
  completion state. The task verbs write state too, through a defensive store:
  atomic temp+rename saves, `create_new` for new files (never clobber), a fresh
  re-read before every mutation, and id allocation at write time.
- Unknown frontmatter fields round-trip untouched and CRLF is tolerated, so
  karamd, taskmd, and Obsidian can share a vault without eating each other's
  data.
- The vault is kept in sync across devices by an external sync setup; karamd
  just reads and writes files in the synced directory. No git wrapper.
- **Cancelling** a recurring task does not stop the series: the next
  `after_completion` occurrence is scheduled `every_days` after the cancellation.

## Nix

A flake is provided:

```
nix build .#karamd        # build the binary
nix run . -- generate --help
```

To use it from another flake, add karamd as an input and apply its overlay, then
reference `pkgs.karamd`:

```nix
{
  inputs.karamd.url = "github:PatrickLerner/karamd";
  # in your nixpkgs instantiation:
  #   overlays = [ karamd.overlays.default ];
  # then pkgs.karamd is available.
}
```

`pkgs.karamd` / `packages.karamd` build from source. For a no-compile install,
`packages.karamd-bin` downloads the prebuilt release binary instead: a tagged
`vX.Y.Z` push triggers `.github/workflows/release.yml`, which builds static-musl
(Linux) and macOS binaries and attaches them to the GitHub Release. After the
first release, pin the asset hashes in `flake.nix` (build once, paste the hash
Nix reports).

## Deployment

Intended to run unattended on a schedule (e.g. cron or a systemd timer) against
the synced vault. Packaging and scheduling are environment-specific and left to
the operator.

## Development

Tasks for this repo are managed with taskmd in `tasks/`.

Logic lives in the `karamd` library crate (`src/lib.rs` + `rule`/`due`/`task`
modules); `src/main.rs` is a thin shim. Due-checks take `today` as a parameter,
so tests are deterministic.

```
cargo fmt --all -- --check
cargo clippy --all-targets --all-features   # clippy all=deny, unsafe forbidden
cargo test
cargo llvm-cov --ignore-filename-regex 'src/main.rs' --fail-under-lines 100
```

The suite holds 100% line coverage (the binary shim aside). CI runs all of the
above on every push and PR (`.github/workflows/ci.yml`).
