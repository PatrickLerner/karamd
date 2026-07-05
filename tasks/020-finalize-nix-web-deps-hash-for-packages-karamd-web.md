---
id: "020"
title: "Finalize Nix web-deps hash for packages.karamd-web"
status: completed
priority: low
dependencies: ["009"]
tags: ["work", "web", "nix"]
created_at: 2026-07-02
completed_at: 2026-07-05
---

# Finalize Nix web-deps hash for packages.karamd-web

## Objective

`packages.karamd-web` builds the SPA in a fixed-output derivation
(`karamd-web-deps`) whose `outputHash` is a placeholder (`lib.fakeHash`), the
same convention as the prebuilt-binary hashes (see #005). It must be filled with
the real hash so the closure is reproducible, mirroring #005 for the release
binaries.

## Tasks

- [ ] Run `nix build .#karamd-web` on a machine with network access; Nix prints
      the real `karamd-web-deps` hash on the fakeHash mismatch.
- [ ] Paste it into `flake.nix` in place of `final.lib.fakeHash`.
- [ ] Rebuild to confirm `nix build .#karamd-web` succeeds and the wrapped
      binary has `KARAMD_WEB_DIR` pointing at the bundled `dist`.

## Acceptance Criteria

- `nix build .#karamd-web` builds offline (after the deps FOD is fetched) and
  produces a `karamd` that serves the bundled SPA with no `--web-dir` flag.

