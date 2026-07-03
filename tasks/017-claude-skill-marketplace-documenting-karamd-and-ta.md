---
title: "Claude skill marketplace documenting karamd and taskmd formats"
id: "017"
status: completed
priority: medium
type: feature
tags: ["work", "docs"]
created_at: "2026-07-02"
completed_at: 2026-07-02
---

# Claude skill marketplace documenting karamd and taskmd formats

## Objective

A Claude Code plugin marketplace (installable via `/plugin marketplace add`)
that ships skills documenting the karamd/taskmd file formats and the recurring
rules config, so any Claude session can read, write, and validate a vault and
author recurring rules correctly without rediscovering the formats.

## Description

Claude working against a taskmd vault needs three pieces of format knowledge
that live only in this repo's code and spec today:

1. **taskmd task file format**: frontmatter fields and enums (`completed` not
   `done`, hyphenated `in-progress`/`in-review`), slug and filename rules, id
   strategies (prefixed emits `dr001`, no separator), `.taskmd.yaml` config
   keys, and validation rules. A local `taskmd-format` skill draft exists in
   `.claude/skills/` as a starting point.
2. **karamd recurring config** (`.taskmd.recurring.yaml`): rule shape, the two
   triggers (`after_completion` with `every_days`, `calendar` with `annual` +
   `lead_days`), optional `body`, dedup markers (`recurring: <key>` /
   `"<key>:<year>"`), leap-day clamping, and the default config path next to
   `.taskmd.yaml`. `recurring.example.yml` is the source of truth.
3. **karamd CLI usage**: `generate` flags and idempotency guarantees, plus
   verbs/query/validate as #011/#014/#015 land.

Package these as skills in a marketplace layout (`.claude-plugin/`
`marketplace.json` + plugin with `skills/`), hosted in this repo or a sibling
repo so it is installable from git. Skill descriptions must state precise
triggers (creating/editing task files, writing recurring rules, running
karamd). Content is derived from the spec and code; where they disagree the
code wins (as with the `dr001` id finding). No personal paths or vault
locations in skill content, this is public.

## Tasks

- [x] Decide hosting: marketplace inside this repo vs. dedicated repo
      (in-repo, `source: "./"`)
- [x] Marketplace scaffolding: `marketplace.json`, plugin manifest, README
- [x] Skill: taskmd task file + `.taskmd.yaml` format (promote the local
      `taskmd-format` draft)
- [x] Skill: karamd recurring rules config format
- [x] Skill or section: karamd CLI usage and idempotency contract
- [ ] Verify install end-to-end via `/plugin marketplace add` in a clean session
      (post-push manual step: the marketplace fetches from the remote, not the
      working tree)
- [x] Keep skills in sync note: update when spec version or CLI surface changes

## Acceptance Criteria

- Marketplace installs cleanly and the skills load in a fresh Claude session
- Skills cover task frontmatter, enums, slugs, ids, `.taskmd.yaml`, and the
  full recurring rule format including triggers, `body`, and dedup markers
- Format details match taskmd 0.2.5 behavior and karamd's implementation
- No personal paths, vault locations, or secrets in any skill file
