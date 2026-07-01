---
title: "Pin prebuilt Nix binary hashes after first release"
id: "005"
status: completed
priority: low
type: chore
tags: ["build", "nix"]
created_at: "2026-07-01"
completed_at: 2026-07-01
---

# Pin prebuilt Nix binary hashes after first release

## Description

`flake.nix` `packages.karamd-bin` fetches prebuilt release assets, but the
per-target hashes are `lib.fakeHash` placeholders until a real release exists
(the `release.yml` workflow produces the assets on a `vX.Y.Z` tag). Until pinned,
`nix build .#karamd-bin` fails with a hash mismatch that reports the real hash.

## Tasks

- [ ] Cut the first release: tag `vX.Y.Z`, confirm `release.yml` uploaded all
      four target archives
- [ ] For each system, `nix build .#karamd-bin` and paste the reported hash into
      the `prebuilt` set in `flake.nix` (or use `nix store prefetch-file`)
- [ ] Verify `nix run .#karamd-bin -- --version` on at least the box's arch
- [ ] Consider automating hash updates in the release workflow later

## Acceptance Criteria

- `nix build .#karamd-bin` succeeds on the target arch with no fakeHash left
- The prebuilt binary runs and reports the expected version
