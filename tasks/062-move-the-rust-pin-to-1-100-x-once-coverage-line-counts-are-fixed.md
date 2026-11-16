---
id: '062'
title: Move the Rust pin to 1.100.x once coverage line counts are fixed
status: completed
created_at: 2026-11-16
type: chore
tags:
- rust
- ci
completed_at: 2026-11-16
---

## Description

Rust 1.100.0 reports lines inside multi-line expressions as unexecuted even when the surrounding function and its closures ran (for example `GraphView::build` in `src/analyze.rs`: function hit 6 times, closure 12, inner lines 0). Line coverage drops from 100% to 95.86% with no test change, so the 100% gate fails. The repo is pinned to 1.99.1 in `rust-toolchain.toml` and both workflows until a fixed release.

1.100.0 also makes `ExitCode` `#[must_use]`; the tests already use `let _ =` for that.

## Acceptance Criteria

- [ ] A 1.100.x (or later) release reports 100% line coverage on this repo again
- [ ] `rust-toolchain.toml`, `ci.yml` and `release.yml` are bumped together
- [ ] CI is green on the new version
