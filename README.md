# karamd

Recurring-task generator for a [taskmd](https://github.com/driangle/taskmd)
markdown vault. taskmd has no recurrence; karamd adds a thin layer that
materialises the next occurrence of a rule when it is due.

`kar` (کار) = work/task, `md` = markdown. Named after taskmd; may replace it
one day, not yet.

## What it does

Reads a rules file, inspects existing tasks, and creates a new task file only
when a rule is due. Idempotent: running it repeatedly on the same day never
creates duplicates. Dedup is a `recurring: <key>` frontmatter field on every
generated task.

Two trigger kinds:

- **after_completion** — next task appears N days after the *last one was
  completed* (interval-from-completion, e.g. a periodic check-in every ~18d).
  Self-healing: a missed run just catches up next time.
- **calendar** — task appears `lead_days` before a fixed annual date, once per
  year (e.g. a birthday, with lead time to buy a present).

These are genuinely different triggers. karamd reads task *state* each run
rather than blindly emitting on a schedule, so neither piles up duplicates.

## Usage

```
karamd generate --vault /path/to/vault --config recurring.yml [--dry-run] [--today YYYY-MM-DD]
```

`--dry-run` reports what would be created without writing. `--today` overrides
the date (for backfill or testing); it defaults to the system date.

See `recurring.example.yml` for the rule format.

## Design

- karamd only ever **adds** task files and **reads** completion state.
  Completions happen elsewhere (in taskmd/Obsidian). No write-write conflict.
- The vault is kept in sync across devices by an external sync setup; karamd
  just writes new task files into the synced directory. It does no syncing
  itself, so it stays pure file IO.
- **Cancelling** a recurring task does not stop the series: the next
  `after_completion` occurrence is scheduled `every_days` after the cancellation.
- Frontmatter parsing tolerates CRLF, so a cross-platform synced vault does not
  silently defeat the dedup marker.

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
