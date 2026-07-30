---
id: '053'
title: 'web ui: installable PWA (manifest, service worker, icons)'
status: pending
created_at: 2026-07-15
priority: low
type: feature
tags:
- web
---

## Description

Make the `karamd web` SPA an installable Progressive Web App, so it can be added
to a phone home screen and launched like a native app. Scope is the **basic
setup**, not offline-first data or push: get it installable and standalone, with
a sensible app shell.

## Motivation

karamd web is already a self-contained SPA served by the axum binary (bun-built
bundle in `web/`, hashed immutable assets, `index.html` revalidated). It is used
on the phone over the tailnet. An installable PWA gives a real app icon,
standalone (no browser chrome) launch, and a faster warm start, without a native
app.

## Scope (basic)

- **Web app manifest** (`manifest.webmanifest`): `name`, `short_name` (karamd),
  `start_url` (`/`), `display: standalone`, `theme_color` / `background_color`
  matched to the Solarized surface, and `icons` (see below). Linked from
  `index.html`.
- **Icons**: at least 192x192 and 512x512 PNGs plus a maskable variant, embedded
  in the bundle (self-hosted, no external fetch, matching the strict-CSP/no-CDN
  rule).
- **Service worker**: minimal, registered from `index.html`. Precache the app
  shell (`index.html` + the hashed JS/CSS) so a cold launch renders instantly;
  network-first for `/api/*` (never cache task data - the vault is the source of
  truth and syncs across devices). Cache-first for the content-hashed static
  assets (safe: the hash changes on deploy). Handle SW update/activation so a new
  deploy is picked up (skipWaiting + clients.claim, or an update prompt).
- **Serving**: axum must serve `manifest.webmanifest` and `sw.js` at the right
  paths with correct content-types, and the SW scope must cover `/`. `sw.js`
  should be revalidated (not immutably cached) like `index.html`.
- **Build**: `web/build.ts` emits the manifest, icons, and service worker into
  `dist/` as part of the normal bun build; the Nix `karamd-web-bundle` picks them
  up unchanged.

## Explicitly out of scope (for now)

- Offline access to task data / background sync / conflict handling.
- Push notifications.
- App-store packaging.

## Acceptance Criteria

- [ ] The site is installable (Chrome/Safari "Add to Home Screen" offers it; a
      Lighthouse PWA installability check passes).
- [ ] Launched from the home screen it opens standalone with the app icon and
      theme color.
- [ ] A cold launch renders the shell from cache; `/api/*` still goes to the
      network and task data is never served stale.
- [ ] A new deploy (changed asset hashes) is picked up without a manual cache
      clear.
- [ ] All PWA assets are self-hosted (no external/CDN request), consistent with
      the existing bundle.
