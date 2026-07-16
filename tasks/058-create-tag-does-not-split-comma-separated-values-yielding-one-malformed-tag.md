---
id: '058'
title: create --tag does not split comma-separated values, yielding one malformed tag
status: pending
created_at: 2026-07-16
priority: high
type: bug
tags:
- cli
- create
- tags
---

# create --tag does not split comma-separated values

`--tag` is documented as repeatable (`--tag a --tag b`), which works correctly.
But passing a comma-separated value to it (natural, since taskmd's `add` used a
comma-separated `--tags`) silently stores the whole string as ONE tag rather
than splitting it. The result is a malformed single tag and an invalid-looking
vault.

## Steps to Reproduce

```
karamd create --tag alpha,beta,gamma "Comma form"
```

## Actual Behavior

```yaml
tags:
- alpha,beta,gamma      # one tag containing commas
```

The repeatable form is fine:

```
karamd create --tag one --tag two "Repeat form"
```
```yaml
tags:
- one
- two
```

## Expected Behavior

A comma-separated value should split into multiple tags (matching taskmd's
`--tags` convention and `--depends-on`'s own comma handling), so
`--tag alpha,beta,gamma` yields `[alpha, beta, gamma]`. At minimum, reject or
warn on a tag value containing a comma instead of writing a malformed tag.

Consider aligning `--tag` to also accept comma-separated input (still
repeatable), and/or add a `--tags` alias for drop-in taskmd compatibility.

## Environment

- OS: macOS (Darwin)
- Version: karamd on PATH (`/run/current-system/sw/bin/karamd`), 2026-07-16
