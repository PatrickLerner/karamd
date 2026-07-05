---
title: 'Today view: Ongoing/This week group order is unstable, Ongoing renders below'
id: '022'
status: completed
priority: medium
type: bug
tags:
- web
- ui
created_at: 2026-07-03
completed_at: 2026-07-05
---

# Today view: Ongoing/This week group order is unstable, Ongoing renders below

## Steps to Reproduce

1. Open the web UI on the **Today** tab (merges the `ongoing` and `now`
   phases, each rendered as its own heading: "Ongoing / background", then
   "This week").
2. Reload a few times / observe on the remote-deployed instance.

## Expected Behavior

Within the Today tab the group order is stable and intentional: **Ongoing /
background first, then This week**, matching the phase order in `.taskmd.yaml`.

## Actual Behavior

The order is unstable and "Ongoing / background" sometimes renders **below**
"This week".

## Hypothesis

`List.tsx` builds groups by iterating `config.phases` (config order) first, then
appends any phase **not** present in the config as a leftover group, in `Map`
insertion order (i.e. task/rank order, which varies). So if the config the web
server loaded does not list `ongoing` (e.g. a stale/lagging `.taskmd.yaml` on
the remote, or a phase present on tasks but missing from config), `ongoing`
tasks drop into the leftover bucket and render after the config phases (`now`,
`next`, …) — below "This week" — with a non-deterministic position.

Likely fix directions (to confirm):

- Give the leftover/unknown-phase groups a deterministic order (stable sort by
  phase id) so it is at least not "unstable".
- For the Today tab specifically, render its constituent phases in the intended
  fixed order (`ongoing` before `now`) regardless of whether both are in the
  server's config, since `tabs.ts` already hardcodes that merge set.
- Consider surfacing when a task's `phase` is not in `.taskmd.yaml` (today it is
  silently bucketed).

## Environment

- Where: remote-deployed `karamd web` (tailnet instance), not observed locally.
- Version: v0.2.0.
