---
id: '029'
title: Update Nix prebuilt hashes for the v0.3.0 release assets
status: pending
created_at: 2026-07-05
type: chore
tags:
- nix
- release
---

## Context

Tag v0.3.0 was pushed and the Release workflow builds/attaches the per-target
binaries and the `karamd-web-dist` bundle. The Nix flake pins these assets by
hash (see prior commits like "Update karamd-bin prebuilt hashes" and "Fill
karamd-web-deps FOD hash"), so `nix build` of `karamd`/`karamd-web` needs the
v0.3.0 hashes.

## Tasks

- [ ] After the release assets publish, update the prebuilt binary hashes and the
      `karamd-web-dist` FOD hash in the flake for v0.3.0
- [ ] `nix build .#karamd` and `.#karamd-web` succeed against the release

## Acceptance Criteria

- The flake builds v0.3.0 from the published release assets with no hash mismatch
