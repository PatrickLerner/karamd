---
title: "Optional per-rule body so generated tasks aren't TODO stubs"
id: "007"
status: completed
priority: medium
type: feature
tags: ["work", "core"]
created_at: "2026-07-01"
completed_at: 2026-07-02
---

# Optional per-rule body so generated tasks aren't TODO stubs

## Objective

Let a rule carry the task body text so `render_task` emits a real
Objective/Tasks/Acceptance instead of the current `TODO` stub. Keep the stub as
the fallback when a rule omits it.

## Description

Today `render_task` (src/task.rs) always writes a fixed stub body with `- [ ]
TODO` placeholders. That is fine for a self-explanatory "reach out" task but
contradicts the vault convention of never leaving placeholders. A rule should be
able to supply its own body.

Design decision to settle first (document it in CLAUDE.md):

- **Option A (recommended): single `body:` field.** Free markdown that replaces
  everything after the frontmatter verbatim. Simplest, most flexible; the rule
  author writes exactly what they want. karamd still prepends nothing except the
  frontmatter.
- **Option B: structured `objective:` / `tasks:` / `acceptance:` fields.** karamd
  assembles the sections. More guardrails, less flexible, more schema.

Lean A. Either way, when the field(s) are absent, keep today's stub so existing
rules do not change output. Preserve the `# <title>` heading and the "generated
by karamd for rule `<key>`" provenance (as an HTML comment) regardless.

## Tasks

- [ ] Add the optional field(s) to `Rule` (src/rule.rs); default `None`
- [ ] `render_task`: when a body is provided, emit it after the frontmatter;
      otherwise the current stub. Keep the title heading and provenance comment
- [ ] Update `Rule::validate` if any constraint applies (e.g. body non-empty)
- [ ] Document the format in `recurring.example.yml`, README, and CLAUDE.md
- [ ] TDD: tests for provided-body, omitted-body (stub unchanged), and provenance
      present in both; keep 100% line coverage

## Acceptance Criteria

- A rule with a body produces a task whose body is that text, no `TODO`
- A rule without a body produces the current stub unchanged (no regression)
- Generated tasks always keep the title heading and the karamd provenance comment
- fmt, clippy, tests, and the 100%-line coverage gate all pass
