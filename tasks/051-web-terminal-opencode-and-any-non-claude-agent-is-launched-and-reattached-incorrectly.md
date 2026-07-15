---
id: '051'
title: 'web terminal: opencode (and any non-claude agent) is launched and reattached incorrectly'
status: completed
created_at: 2026-07-15
priority: high
type: bug
tags:
- web
- terminal
dependencies:
- '047'
completed_at: 2026-07-15
---

## Description

Launching the embedded web terminal (#010/#047) with **opencode** is broken in two
independent ways. Both surface for opencode specifically but are general defects in how
karamd handles agents other than the default `claude`.

### Defect 1: the task prompt is passed as a bare positional argument

`src/web_terminal.rs` seeds the task context by appending the prompt as the final
argv element:

```rust
cmd.arg(&prompt);   // -> opencode "Work on task 174: ..."
```

This is a `claude`-ism: `claude "<text>"` treats a bare positional as the initial
prompt. But `opencode`'s default command is `opencode [project]`, where the positional
is **a directory to start in**. opencode therefore tries to `cd` into a directory named
after the prompt and dies:

```
Error: Failed to change directory to /Work on task 174: Time Keeper Tool ...
process exited (code 0)
```

opencode wants the prompt via a flag instead: `opencode --prompt "<text>"`.

The prompt-delivery method must be per-agent, like the headless `run.agents`
`{prompt}` / `{prompt_file}` placeholders already are, rather than hard-coded as
"append as the last positional arg".

### Defect 2: the agent choice is lost on reattach from the sidebar

The session route can carry the agent (`#/view/<tab>/task/<id>/run/<agent>`), but the
sidebar reattach drops it (`web/src/main.tsx`):

```ts
onSelectSession={(id) => navigate(runHref(tabForLinks, id))}  // no agent
```

So reattaching navigates to `.../run` with agent = null. `Terminal` reconnects with no
`?agent=`, `resolve_launch_argv` falls back to `--run-command` (claude), the resulting
argv differs from the live opencode session's argv, and `get_or_create` kills the
opencode session and relaunches claude. The user sees "the session won't go to
opencode."

Deeper cause: `SessionInfo` (`src/web_terminal.rs`) exposes no `agent`, so the frontend
has nothing to fill the `/run/<agent>` segment with when building the sidebar href.

## Fix

1. Per-agent terminal prompt seeding: give the agent spec a way to declare how the
   interactive prompt is delivered (a `terminal`/prompt-arg template mapping opencode to
   `--prompt {prompt}`, claude to the bare positional). Replace the hard-coded
   `cmd.arg(&prompt)`.
2. Thread the agent through the session: store the agent name on `Session`, expose it on
   `SessionInfo`, and have the sidebar build `runHref(tab, id, session.agent)` so reattach
   targets the same agent instead of the default. Keep single-session-per-task and the
   relaunch-on-different-tool behaviour intact.

## Acceptance Criteria

- [ ] Launching the terminal with opencode starts opencode with the task prompt seeded
      via its flag (`--prompt`), not as a directory positional; no "change directory"
      error.
- [ ] claude still launches with the prompt as before (no regression).
- [ ] Reattaching to a live opencode session from the sidebar reconnects to that same
      opencode session and does not relaunch claude.
- [ ] `SessionInfo` carries the agent; the sidebar href includes the agent segment.
- [ ] `cargo fmt`, `clippy`, `test`, and 100% line coverage all pass.
