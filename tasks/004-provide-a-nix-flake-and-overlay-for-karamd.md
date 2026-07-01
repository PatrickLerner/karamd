---
title: "Provide a Nix flake and overlay for karamd"
id: "004"
status: completed
priority: medium
type: chore
tags: ["build", "nix"]
created_at: "2026-07-01"
completed_at: 2026-07-01
---

# Provide a Nix flake and overlay for karamd

## Description

Package karamd with Nix so downstream configs consume it without a manual
`cargo build`. Two consumption paths:

- `packages.karamd` / `overlays.default` — build from source (rustPlatform,
  pinned via `Cargo.lock`). Always available.
- `packages.karamd-bin` — download the prebuilt release binary (no compile).
  `.github/workflows/release.yml` builds static-musl (Linux) + macOS binaries on
  a `vX.Y.Z` tag and attaches them to the GitHub Release; the flake `fetchurl`s
  the matching asset.

## Tasks

- [x] `flake.nix`: `packages.<system>.karamd` from source + `overlays.default`
- [x] `apps.<system>.default` so `nix run` works
- [x] devShell with cargo/rustc/clippy/rustfmt
- [x] `release.yml`: build + upload prebuilt binaries per target on tag
- [x] `packages.<system>.karamd-bin` fetching the prebuilt asset
- [x] README: document both consumption paths

## Acceptance Criteria

- `nix build .#karamd` produces a working binary (from source)
- `nix run . -- generate --help` works
- A flake that adds `overlays.default` can reference `pkgs.karamd`
- On a `vX.Y.Z` tag, release binaries are attached to the GitHub Release

## Follow-up

- Prebuilt asset hashes cannot be pinned until the first release exists; tracked
  in a separate task.
