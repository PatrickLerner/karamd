#!/usr/bin/env bash
# Compare `karamd next` against the real taskmd binary on the same vault.
# Usage: scripts/next-parity.sh [vault-dir ...]   (defaults to the repo root)
#
# Not part of CI (taskmd is not installed there); run locally after touching
# src/next.rs. Exits non-zero on any divergence in rank/id/score/reasons.
set -euo pipefail

command -v taskmd >/dev/null || { echo "taskmd not installed; skipping" >&2; exit 0; }
cargo build --quiet

norm() {
  ruby -rjson -e 'puts JSON.parse(STDIN.read).map { |t|
    [t["rank"], t["id"], t["score"], t["reasons"].join("|")].join(" ")
  }'
}

fail=0
for vault in "${@:-.}"; do
  a=$( (cd "$vault" && taskmd next --limit 20 --format json) | norm)
  b=$(./target/debug/karamd next --limit 20 --vault "$vault" --json | norm)
  if [ "$a" = "$b" ]; then
    echo "OK   $vault"
  else
    echo "DIFF $vault"
    diff <(echo "$a") <(echo "$b") || true
    fail=1
  fi
done
exit "$fail"
