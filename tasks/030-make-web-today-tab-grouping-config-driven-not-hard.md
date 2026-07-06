---
id: '030'
title: Make web Today-tab grouping config-driven, not hardcoded
status: completed
priority: medium
dependencies: []
tags: []
created_at: 2026-07-06
completed_at: 2026-07-06
---

# Make web Today-tab grouping config-driven, not hardcoded

## Objective

The web dashboard's "Today" tab merges phases from a hardcoded constant, `TODAY_PHASE_ORDER = ["ongoing", "now"]` in `web/src/tabs.ts:10`, instead of reading the grouping from server config. When phase ids get renamed, this constant silently breaks the Today grouping. Make which phases compose the Today tab, and their order, driven by config so the web stays correct across phase renames.

Related: #022 (Today-view ongoing/this-week group order instability).

## Tasks

- [ ] Add a config field that declares which phase ids the Today tab merges and in what order (e.g. a `today` list, or a per-phase `today: true`/pinned flag on the `phases` entries in `.taskmd.yaml`)
- [ ] Surface the field through the server config API (`web/src/types.ts` `Config`/`Phase`, and the backend that serves it)
- [ ] Replace the `TODAY_PHASE_ORDER` constant in `web/src/tabs.ts` with the config-derived set; keep the "unphased open task falls into Today" behavior
- [ ] Update `web/src/views/List.tsx` fallback ordering that references `TODAY_PHASE_ORDER`
- [ ] Sensible default when the config omits the field (fall back to current `["ongoing", "now"]` so existing configs keep working)
- [ ] `web/mock.ts` and docs/README updated to reflect the new config field

## Acceptance Criteria

- The Today tab's merged phases come from server config, not a source constant
- Renaming a phase id in config keeps the Today grouping correct without a code change
- Omitting the field preserves today's behavior (ongoing + now merged, ongoing first)
- `bun` build/typecheck passes and the dashboard renders the Today tab correctly
