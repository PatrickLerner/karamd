---
id: '040'
title: 'generate: let recurring rules set arbitrary frontmatter (e.g. ai-runnable tag) on generated tasks'
status: completed
created_at: 2026-07-09
priority: medium
type: feature
tags:
- generate
- recurring
- frontmatter
completed_at: 2026-07-09
---

## Description

Recurring rules can currently only influence a fixed set of fields on the tasks they emit
(title, tags via the template, body, the `recurring:` marker). To make a rule produce a
task that another tool acts on, a rule needs to set **arbitrary frontmatter** on the
generated task, e.g. the `ai-runnable` tag (or an `agent:`/`ai_working_dir:` field) so the
new `karamd run` command (#039) picks it up. This closes the loop: a rule schedules an
autonomous chore, `run` executes it, with no hand-editing in between.

## Proposed behaviour

Add an optional `frontmatter:` map to a rule. Every key/value is merged verbatim into the
generated task's frontmatter, on top of the fields karamd already writes.

```yaml
- title: "Fetch KPIs and update dashboard note"
  trigger: weekly
  day_of_week: mon
  frontmatter:
    tags: [ai-runnable, reporting]
    ai_working_dir: /Users/.../notes-repo
```

- **Merge, don't clobber the essentials.** karamd-owned fields (`id`, `created_at`,
  `status`, and the `recurring:` dedup marker) always win and cannot be overridden by the
  rule; overriding them would break idempotency or the spec. Everything else the rule
  supplies is written as-is.
- **`tags` merges, not replaces**, with the tags the template/rule already produces
  (dedup, preserve order).
- **Values are passed through verbatim** (scalars, lists, nested maps), consistent with the
  "unknown frontmatter preserved verbatim" contract. karamd does not validate rule-supplied
  fields against the taskmd spec; that is the author's responsibility.
- **`Rule::validate` rejects** a `frontmatter:` that tries to set a karamd-owned key, with a
  clear error, rather than silently ignoring it.

## Why separate from #039

`run` is the executor; this is how a *generated* task becomes eligible for it. They compose
but neither strictly blocks the other: you can already tag a task by hand or with
`create --tag ai-runnable`. Kept as its own task so `run` is not gated on the rule-model
change.

## Acceptance Criteria

- [ ] A rule with a `frontmatter:` map writes those keys onto every task it generates.
- [ ] `tags` in `frontmatter:` merges (deduped) with existing tags rather than replacing them.
- [ ] Attempting to set `id`/`created_at`/`status`/`recurring` via `frontmatter:` is rejected by validation.
- [ ] Nested/list values round-trip verbatim into the generated file.
- [ ] Rules without `frontmatter:` generate byte-for-byte the same output as today (no regression).
- [ ] A rule emitting `tags: [ai-runnable]` produces a task that `karamd run` (#039) selects.
