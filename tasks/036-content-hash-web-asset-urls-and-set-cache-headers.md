---
id: '036'
title: Content-hash web asset URLs and set cache headers
status: completed
created_at: 2026-07-07
priority: medium
type: improvement
completed_at: 2026-07-07
---

## Objective

The web SPA is built with fixed asset filenames (`main.js`, `styles.css`) and karamd serves them with only `Last-Modified` (no `Cache-Control`, no `ETag`). Browsers fall back to heuristic caching, so after a deploy that keeps the same filenames a plain reload can serve stale JS/CSS and users need a manual hard refresh (`Cmd+Shift+R`). Fix cache-busting properly: emit content-hashed asset URLs and serve them immutable, while keeping `index.html` uncached so it always points at the current hashes.

## Tasks

- [ ] Configure the bun build to emit content-hashed asset names (e.g. `main.[hash].js`, `styles.[hash].css`) and rewrite the references in `index.html`
- [ ] Serve hashed assets with `Cache-Control: public, max-age=31536000, immutable` in the axum static layer (`src/web.rs`)
- [ ] Serve `index.html` (and the SPA fallback) with `Cache-Control: no-cache` so it always revalidates and points at fresh hashes
- [ ] Keep the fonts under `fonts/` cacheable (hash them too, or long-cache since they are stable)
- [ ] Verify a deploy that changes JS/CSS is picked up on a normal reload, no hard refresh
- [ ] Rebuild `web/dist` and update Nix prebuilt hashes if the release assets change

## Acceptance Criteria

- Asset URLs change whenever their content changes; `index.html` references the current hashes
- Hashed assets respond with a long-lived immutable `Cache-Control`; `index.html` is `no-cache`
- After a rebuild with changed JS/CSS, a plain browser reload shows the new build (no manual hard refresh)
- `bun` build/typecheck passes and the dashboard renders correctly
