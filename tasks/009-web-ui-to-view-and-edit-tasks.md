---
title: "Web UI to view and edit tasks (karamd web)"
id: "009"
status: completed
priority: medium
type: feature
tags: ["work", "web"]
dependencies: ["008", "011"]
created_at: "2026-07-02"
completed_at: 2026-07-02
---

# Web UI to view and edit tasks (karamd web)

## Objective

A minimalist, visually pleasing web UI to view tasks, launched with
`karamd web`. A React SPA built with **bun**, whose production bundle is
embedded into the Rust binary and served by an axum backend. Solarized Light
theme, a nice readable font, generous whitespace. Beyond viewing it can add and
edit tasks. Built on the taskmd library from #008.

## Description

Depends on the taskmd library (#008) for parse/model/mutate; the axum backend is
a thin JSON API over it, no duplicate task logic.

Tech choices, constrained by the AI-execution follow-up (#010):

- **Backend: axum on tokio (async, WebSocket-capable).** Serves the embedded SPA
  bundle plus a small JSON API. Async + WebSocket now so #010 can stream a live
  Claude terminal (WebSocket + PTY) with no re-platform.
- **Access model: local + remote over Tailscale.** Runs on a remote machine and
  is reached from other devices over the tailnet. Configurable bind address
  (`--bind`, default `127.0.0.1`; opt in to the Tailscale IP / `0.0.0.0`). No
  app-level auth: the tailnet + Tailscale ACLs are the security boundary; never
  bind a public interface. Prefer `tailscale serve` for TLS/HTTPS rather than
  terminating TLS in Rust. Ensure WebSocket (#010) works through
  `tailscale serve`.
- **Frontend: React SPA, built with bun.** `bun install` + `bun build` (bun
  replaces the npm+vite toolchain with one fast tool) produce a static `dist/`.
  axum serves it from a path resolved via `--web-dir` / env var. No `rust-embed`:
  Nix (see #004) builds the frontend and the binary in one derivation and pins
  the `dist/` store path, so the deployed closure is self-contained without
  embedding. Dev loop: rebuild the frontend with bun, no cargo recompile.
  Build/CI needs bun.
- **API shape:** axum exposes JSON endpoints (list, get, create, update,
  transition); the SPA renders them. Writes go through the #008 library so files
  stay taskmd-compatible and custom fields (e.g. `recurring:`) are preserved.
- **Styling: Solarized Light**. **Font: iA Quattro** (SIL OFL), self-hosted
  `.woff2` bundled with the SPA (no external fetch / CDN). Minimalist: clear
  typographic hierarchy, comfortable line length, subtle Solarized accent colors
  for status/priority.
- **Mobile-first / responsive.** Primary use is managing tasks from a phone over
  Tailscale. Responsive layout, `viewport` meta, touch-sized targets, single
  column on narrow screens. Test at phone widths, not just desktop.

Views:

- Task list grouped by phase (order from config), status/priority as color
  chips, dependency state visible.
- Task detail: full frontmatter + body.
- Add + edit forms (title, status, priority, phase, tags, body), and status
  transitions (complete/cancel/reopen) via the #008 library.

All assets (JS/CSS/font) are self-hosted from `dist/`, no runtime network / CDN
dependency. Nix (#004) bundles frontend + binary so the deployed closure is
self-contained.

## Tasks

- [x] `karamd web` subcommand with `--bind` (default 127.0.0.1); print URL
- [x] JSON API over the #008 library: list, get, create, update, transition
- [x] React SPA scaffold built with bun; axum serves `dist/` via `--web-dir`
- [x] Nix (#004) builds frontend + binary in one derivation, pins `dist/` path
      (hash placeholder to finalize on first `nix build`, tracked in #020)
- [x] Solarized Light theme + bundled iA Quattro font, minimalist layout
- [x] Responsive mobile-first layout (viewport meta, touch targets, 1-col)
- [x] List grouped by phase + task detail + add/edit forms + transitions
- [ ] Verify remote access over Tailscale, incl. `tailscale serve` for TLS
      (live deployment check, tracked in #019)
- [x] Two-stage build documented (bun build then cargo build); CI runs bun
- [x] Tests for the API handlers over the library; keep coverage gate green
- [x] Document `karamd web` (bind, Tailscale, mobile) in README

## Acceptance Criteria

- `karamd web` serves the SPA from `dist/` (`--web-dir`); no runtime network/CDN
  dependency; Nix closure is self-contained
- Tasks render grouped by phase in a clean Solarized Light UI with iA Quattro
- UI is usable on a phone (responsive) reached remotely over Tailscale
- Adding/editing/transitioning tasks writes taskmd-compatible files via #008,
  custom fields intact
- Backend is async + WebSocket-capable so #010 needs no re-platform
- fmt, clippy, Rust tests, and the coverage gate pass; bun build succeeds in CI
