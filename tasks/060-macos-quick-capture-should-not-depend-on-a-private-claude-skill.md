---
id: '060'
title: 'macos: quick capture should not depend on a private Claude skill'
status: completed
created_at: 2026-11-16
type: improvement
tags:
- macos
completed_at: 2026-11-16
---

## Description

Quick capture (⌥⌘T) tells a headless `claude -p` to "use the file-task skill". That skill is not part of this repo, so capture only works on a machine that has it. The prompt lives in `macos/Sources/KaramdBarCore/CaptureRequest.swift`.

## Acceptance Criteria

- [ ] The prompt uses the `karamd-cli` skill shipped in `skills/`, or the skill name is a `defaults` setting with that as the default
- [ ] `CaptureRequestTests` cover the new prompt
- [ ] README documents the setting if one is added
