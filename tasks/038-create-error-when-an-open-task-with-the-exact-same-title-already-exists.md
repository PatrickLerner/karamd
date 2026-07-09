---
id: '038'
title: 'create: error when an open task with the exact same title already exists'
status: completed
created_at: 2026-07-08
priority: medium
type: feature
tags:
- create
- dedup
completed_at: 2026-07-09
---

## Description

`karamd create` unconditionally writes a new task file. When a caller runs it twice —
extremely common with LLM agents that retry or re-emit a tool call — you get two open
tasks with the **exact same title** and different ids. Pure noise that has to be cleaned
up by hand.

Real example (dastyar): an agent ran
`karamd create "Check voice input for dastyar and albert" …` twice within seconds,
producing `001-` and `002-` duplicates. Same happened with `"Instagram reel"`.

## Proposed behaviour

On `create`, before writing, scan the vault for an **open** task (status not in
`completed`/`cancelled`) whose title matches the new one **exactly**. If one exists,
exit non-zero with a clear error naming the existing task id:

```
error: an open task with this title already exists: 002-check-voice-input-for-dastyar-and-albert
       (use --force to create a duplicate anyway)
```

## Details

- **Open tasks only** — a `completed`/`cancelled` task with the same title must NOT block
  a fresh one (you may legitimately redo something).
- **Exact match** — trim surrounding whitespace; case-sensitive by default.
- **`--force`** escape hatch for the rare intentional duplicate.
- **`--json`**: emit the error as JSON (`{"error": "...", "existing_id": "002"}`) so
  scripted callers can branch instead of parsing stderr.
- Consistent with the existing recurring-rule dedup (an open instance already blocks a
  duplicate); this extends the same principle to plain `create`.

## Acceptance Criteria

- [ ] `create` with a title matching an existing OPEN task exits non-zero and does not write a file.
- [ ] The error names the colliding task id; `--json` returns structured error output.
- [ ] A matching title whose only tasks are completed/cancelled still creates normally.
- [ ] `--force` bypasses the check and creates the duplicate.
