---
id: "019"
title: "Verify karamd web over Tailscale (remote access + tailscale serve TLS)"
status: completed
priority: low
dependencies: ["009"]
tags: ["work", "web", "ops"]
created_at: 2026-07-02
completed_at: 2026-07-05
---

# Verify karamd web over Tailscale (remote access + tailscale serve TLS)

## Objective

Confirm `karamd web` is usable from a phone/other device over the tailnet, and
that `tailscale serve` fronts it with TLS. This is a live deployment check that
cannot be exercised in CI or unit tests; #009 shipped the server (loopback bind
default, configurable `--bind`) but this last acceptance item is environment
specific.

## Tasks

- [ ] Run `karamd web --bind <tailscale-ip>:8787` (or `0.0.0.0`) on the host
      that holds the synced vault.
- [ ] Reach it from another tailnet device; confirm list/detail/edit/status all
      work on a phone-width screen.
- [ ] Put `tailscale serve` in front for HTTPS; confirm the SPA loads over TLS.
- [ ] Confirm the WebSocket path works through `tailscale serve` (so #010's live
      terminal will not need a re-platform).

## Acceptance Criteria

- The UI is reachable and fully usable remotely over Tailscale, over HTTPS via
  `tailscale serve`, with WebSocket upgrades passing through.
- No public interface is bound; the tailnet + ACLs remain the security boundary.
