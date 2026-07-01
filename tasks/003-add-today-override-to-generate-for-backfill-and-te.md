---
title: "Add --today override to generate for backfill and testing"
id: "003"
status: completed
priority: low
type: feature
tags: ["core"]
created_at: "2026-07-01"
completed_at: 2026-07-01
---

# Add --today override to generate for backfill and testing

## Objective

Let a run pretend it is a specific date, so behaviour can be checked against the
real vault without waiting for the calendar and a missed cron day can be
backfilled deterministically.

## Description

The core `generate(vault, config, today, dry_run)` already takes `today` as a
parameter (that is how the unit tests stay deterministic). Only the CLI hard-codes
`Local::now().date_naive()`. Expose an optional `--today YYYY-MM-DD` flag on the
`generate` subcommand that overrides it, defaulting to the real date.

## Tasks

- [ ] Add `--today` (parse `YYYY-MM-DD` via `NaiveDate`) to the Generate command
- [ ] Default to `Local::now().date_naive()` when absent
- [ ] Reject an unparseable value with a clear error
- [ ] Test the flag through `run(...)` (parsed) and the default path

## Acceptance Criteria

- `generate --today 2026-07-20 --dry-run` reports as if run on that date
- Omitting the flag behaves exactly as today
