---
name: filing-tasks
description: How to file / create / track a task or follow-up with karamd. Load whenever you are about to record work for later — a deferred or out-of-scope item, a "do this next", a TODO, a bug or idea to track, or a follow-up for anything you scope out. In a taskmd vault, tasks are markdown files managed by the karamd CLI; use it, don't hand-edit frontmatter or run --help first.
---

# Filing tasks with karamd

In a [taskmd](https://github.com/driangle/taskmd) vault, tasks are markdown
files managed by the **karamd** CLI. To record work for later, run
`karamd create` — never hand-edit a task's frontmatter, and don't `--help` to
recall the flags; they're below. Full CLI surface: the `karamd-cli` skill.
File-format details: the `taskmd-format` skill.

## The one command

```
karamd create "<title>" --type <t> --priority <p> --body "<markdown>"
```

Prints `karamd: created <id> (<file>)`. That's the whole flow for filing.
`--vault` defaults to the current directory, so run from the vault root and
omit it (pass `--vault <path>` only to target another vault).

## Flags you actually use

| Flag | Values | Notes |
|------|--------|-------|
| `--type` | `feature` `bug` `improvement` `chore` `docs` | |
| `--priority` | `low` `medium` `high` `critical` | |
| `--effort` | `small` `medium` `large` | optional |
| `--tag` | free-form, repeatable | e.g. `--tag research --tag ci` |
| `--depends-on` | `008,011` | comma-separated ids |
| `--body` | markdown string | replaces the template body; write the real content |
| `--template` | `feature` `bug` `chore` (or a custom `.taskmd/templates/<name>.md`) | scaffolds a body instead of `--body` |
| `--force` | | create even if an open task has the exact same title |

## File follow-ups as tasks, immediately

When you defer or scope out work, file it as a task **in the same action** —
not a note in a commit, PR, or comment. A deferred item with no task is lost
work. One `karamd create` is the whole cost.

## Changing state after filing

- `karamd status <id> <state>` — `pending` `in-progress` `in-review` `completed` `blocked` `cancelled`. Only `pending`/`in-progress` are actionable.
- `karamd complete <id>` (add `--pr <url>` under the `pr-review` workflow) / `karamd cancel <id>` / `karamd reopen <id>`.
- `karamd list`, `karamd show <id>`, `karamd next` to inspect.

Go through karamd for all of these — never edit the `.md` frontmatter directly.
