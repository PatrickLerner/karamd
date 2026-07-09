---
id: '039'
title: 'run: autonomous task execution mode via a configured AI agent (claude/opencode)'
status: completed
created_at: 2026-07-09
priority: medium
type: feature
tags:
- run
- ai
- automation
completed_at: 2026-07-09
---

## Description

A new unattended verb, `karamd run`, sibling to `generate` and cron-friendly. Each
invocation scans the vault, selects the AI-runnable, non-terminal tasks, and for each one
spawns a configured AI agent (claude or opencode) as a subprocess with a prompt built from
a template plus the task. The agent autonomously implements the task; karamd records the
outcome and tracks failures.

Motivating examples are recurring chores an agent can do headless: "fetch data from an MCP
server and update a local file", "regenerate a report", "sync X into note Y".

Execution is a distinct command from `generate`; the two only compose. Getting the
`ai-runnable` tag onto generated tasks is a separate `generate` capability (arbitrary
frontmatter passthrough in rules), filed on its own.

## How it composes

`run` pairs with `generate`. A recurring rule emits a task tagged `ai-runnable`; the next
`run` tick executes it. `generate` schedules, `run` does the work. Both are idempotent and
read task *state* every run, never fire blindly on a timer.

## Design decisions (settled)

- **Per-task opt-in: the `ai-runnable` tag.** A task runs only if it carries the reserved
  tag `ai-runnable`. A plain tag keeps files taskmd-spec-clean, shows up in Obsidian, and
  is already filterable by the query grammar. No new required frontmatter field.
- **Success signal: both exit code AND self-reported status.** A non-zero exit is always a
  failure. On exit 0, the run counts as done only if the agent also moved the task to a
  terminal status (the agent is instructed to run `karamd complete <id>` itself). Exit 0
  with the task still open = not finished = eligible for retry. Belt and suspenders: a
  clean exit alone never marks a task complete.
- **Working dir: config default, per-task override.** A global `working_dir` in config,
  overridable per task via a frontmatter field (e.g. `ai_working_dir:`). Supports "edit
  file X in repo Y" tasks. The override is a non-spec field, preserved verbatim by karamd,
  ignored by taskmd. Documented as an intentional karamd extension.
- **Agent-agnostic: run any CLI, not just claude/opencode.** An agent is just a named
  command template in config. claude and opencode are example entries, nothing about them
  is special-cased. Any tool that can act on a prompt works: an aider invocation, a shell
  script wrapping an API call, a custom binary.
- **Prompt delivery: template + task in one string, delivered per-agent.** A configured
  template with placeholders (`{id}`, `{title}`, `{body}`, `{path}`) renders to a single
  prompt string. Because tools differ, each agent declares how it receives that string via
  `prompt_via`: `arg` (a `{prompt}` token in the argv), `stdin` (piped in), or `file` (a
  temp file whose path fills a `{prompt_file}` token). Default `arg`.

## Config sketch (new `run:` block in `.taskmd.recurring.yaml`)

```yaml
run:
  enabled: true          # absent/false => feature off (top-level lock)
  agent: claude          # default agent; a task may override via frontmatter
  agents:
    claude:
      command: ["claude", "-p", "{prompt}", "--permission-mode", "acceptEdits"]
      prompt_via: arg
    opencode:
      command: ["opencode", "run", "{prompt}"]
      prompt_via: arg
    my-script:            # any tool: this one reads the prompt on stdin
      command: ["/Users/.../bin/agent.sh"]
      prompt_via: stdin
  working_dir: /Users/.../some-repo   # default cwd; task frontmatter may override
  timeout_secs: 900
  max_attempts: 3
  prompt_template: |
    You are completing a taskmd task autonomously. When finished, run
    `karamd complete {id}` from the vault. Task {id}: {title}

    {body}
```

The command comes only from the config allowlist; task text never supplies the command,
only which named agent to use (and that too must resolve to a configured entry).

## Failure tracking (stop infinite retries)

The selection predicate is "non-terminal AND tagged `ai-runnable`". A failed run leaves the
task open, so without a gate it is re-selected every tick forever. The gate is a
**pre-incremented attempt counter plus a hard park**, all in frontmatter (non-spec,
preserved verbatim):

1. **Before spawning**, atomically write `ai_attempts: n+1`, `ai_status: running`,
   `ai_run_started: <ts>`. Incrementing *before* the run is the trick: a mid-run crash
   costs one attempt instead of being a free infinite retry.
2. Run with `timeout_secs`.
3. On the result:
   - Success (exit 0 AND task reached terminal status): clear all `ai_*` markers.
   - Failure (non-zero, timeout, or exit 0 but task still open): set `ai_status: failed`,
     `ai_last_error`, `ai_last_run`; keep the counter.
4. When `ai_attempts >= max_attempts`: park the task by adding the `ai-failed` tag. The
   selection predicate excludes `ai-failed`, so it is never picked again until a human
   removes the tag or resets the counter. Hard stop.

**Selection predicate:** non-terminal AND tag `ai-runnable` AND NOT tag `ai-failed`
AND `ai_attempts < max_attempts` AND no live lock.

**Crash / stale-lock recovery:** an `ai_status: running` whose `ai_run_started` is older
than `2 * timeout_secs` means the previous run died without cleanup. Treat it as a
completed failed attempt (the counter was already bumped pre-spawn), clear `running`, and
fall through to normal failure handling. Stops both a wedged `running` marker blocking a
task forever and a crash-retry loop.

**Optional (not v1):** backoff via `ai_last_run` so retries wait `f(attempts)` before the
cap is hit, instead of firing every tick.

## Safety

Three independent locks, all required before anything spawns:

1. `run.enabled: true` in config (feature is off by default).
2. The task carries the `ai-runnable` tag.
3. The command is an allowlisted template from config.

Plus: `timeout_secs` bounds every run, `--dry-run` prints what would execute without
spawning, and per-run stdout/stderr is captured to a `runs/` log dir so failures are
debuggable (frontmatter holds only the summary).

## Testability

Selection, prompt rendering, the run/skip/park decision, and the frontmatter state
transitions are pure and unit-tested (take `today: NaiveDate`, no clock, no spawn). The
actual subprocess spawn + timeout is thin I/O behind a trait, excluded from coverage like
`src/web_terminal.rs`.

## Acceptance Criteria

- [ ] `karamd run` selects only non-terminal tasks tagged `ai-runnable`; everything else is skipped.
- [ ] With `run.enabled` absent/false, `run` does nothing and spawns no process.
- [ ] The prompt is rendered from the configured template with `{id}/{title}/{body}/{path}` interpolated.
- [ ] Any configured agent runs, not just claude/opencode; `prompt_via` arg/stdin/file all deliver the prompt correctly.
- [ ] A task may select a non-default agent, but only one that resolves to a configured entry; an unknown agent is a failure, not an arbitrary command.
- [ ] The agent command comes only from config; non-zero exit records a failure.
- [ ] `ai_attempts` is incremented BEFORE the spawn, so a crash/timeout still counts as an attempt.
- [ ] Exit 0 marks the task done only if the task reached a terminal status; otherwise it stays eligible.
- [ ] `working_dir` defaults from config and is overridable per task via frontmatter.
- [ ] At `max_attempts` the task is parked with the `ai-failed` tag and is no longer selected.
- [ ] A stale `ai_status: running` (older than 2x timeout) is reconciled as a failed attempt, not a permanent block.
- [ ] A run in progress cannot be double-started by a second `run` invocation (lock/marker).
- [ ] `--dry-run` prints planned runs without spawning; each real run captures stdout/stderr to a log.
- [ ] Pure logic reaches 100% line coverage; only the spawn glue is excluded.

## Follow-ups (file as separate tasks if pursued)

- Concurrency across multiple ai-runnable tasks in one tick (serial first; parallel later).
- Notification on failure (reuse the web `--run-command` / push mechanism).
- Web UI surface: a Today badge for `ai-runnable`/`ai-failed` and a "run now" button.
