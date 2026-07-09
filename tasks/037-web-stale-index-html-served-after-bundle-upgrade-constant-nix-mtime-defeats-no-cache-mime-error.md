---
id: '037'
title: 'web: stale index.html served after bundle upgrade (constant Nix mtime defeats no-cache → MIME error)'
status: completed
created_at: 2026-07-08
priority: medium
type: bug
tags:
- web
- cache
completed_at: 2026-07-09
---

## Description

`karamd web` serves a stale `index.html` after any web-bundle upgrade, breaking the
SPA until the user hard-refreshes. Browser console:

```
main.js:1 Failed to load module script: Expected a JavaScript-or-Wasm module script
but the server responded with a MIME type of "text/html".
```

## Root cause

`index.html` is served with `Cache-Control: no-cache` (`src/web.rs`, `CACHE_REVALIDATE`).
Intent: always revalidate so the HTML points at the current hashed asset URLs after a
deploy. But `no-cache` means *revalidate*, not *don't store*, and the only validator
tower-http's `ServeDir`/`ServeFile` emits is `Last-Modified` (the file mtime). No `ETag`.

When served from a Nix store path (`--web-dir /nix/store/...`), every file's mtime is
normalized to a constant `1970-01-01 00:00:01`, so revalidation is defeated:

1. Client loads `/` on version A -> caches index.html, `Last-Modified: 1970-01-01 00:00:01`.
2. Deploy version B. 0.6.1 content-hashes assets, so B's index.html references
   `main-<hash>.js` instead of A's `main.js`.
3. Client revisits `/`, sends `If-Modified-Since: 1970-01-01 00:00:01`. The new file's
   mtime is also `1970-01-01 00:00:01` -> server returns `304 Not Modified`.
4. Browser serves the stale A index.html, requests `/main.js`. Gone -> SPA fallback
   returns index.html as text/html -> module MIME error -> white screen.

Confirmed live:

```
$ curl -sD- -o/dev/null http://box:8787/
last-modified: Thu, 01 Jan 1970 00:00:01 GMT
cache-control: no-cache
$ curl -sD- -o/dev/null -H 'If-Modified-Since: Thu, 01 Jan 1970 00:00:01 GMT' http://box:8787/
HTTP/1.1 304 Not Modified
```

The constant Nix mtime makes this deterministic on every upgrade. Hard-refresh (skips
the conditional request) is the only client-side recovery.

## Fix

Use `no-store` for the SPA HTML entrypoint / deep-link fallbacks so the browser never
stores it and never issues a conditional request:

```rust
const CACHE_REVALIDATE: &str = "no-store";
```

index.html is ~400 bytes and there is one; refetching per navigation is negligible.
Content-hashed assets keep `immutable`. Alternatives: emit a content-based `ETag` for
the revalidate class, or strip `Last-Modified` from those responses.

## Acceptance Criteria

- [ ] After a web-bundle upgrade, a normal reload (no hard-refresh) loads the new SPA.
- [ ] `index.html` responses no longer produce a `304` off the constant Nix mtime.
- [ ] Content-hashed assets remain cacheable (`immutable`).
