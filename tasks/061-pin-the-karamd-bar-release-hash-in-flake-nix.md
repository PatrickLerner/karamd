---
id: '061'
title: Pin the karamd-bar release hash in flake.nix
status: completed
created_at: 2026-11-16
type: chore
tags:
- macos
- nix
completed_at: 2026-11-16
---

## Description

`packages.karamd-bar` fetches `karamd-bar-macos.zip` from the release, but its hash is `lib.fakeHash` until the first release that carries the zip. The release workflow builds and uploads it from the next tag on.

## Acceptance Criteria

- [ ] After the next tagged release, `nix build .#karamd-bar` reports the real hash and it is pasted into `flake.nix`
- [ ] `nix build .#karamd-bar` succeeds on aarch64-darwin and the app starts
