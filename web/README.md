# karamd web frontend

Minimalist task viewer/editor for a taskmd vault. React SPA, hash routing,
plain CSS, no framework beyond react + react-dom. Designed mobile-first
(primary use: phone over Tailscale), Solarized Light, iA Writer Quattro
(self-hosted, SIL OFL, license in `public/fonts/LICENSE.md`). No network
requests at runtime except `/api` on the same origin.

## Build

bun is not installed globally; run it through nix:

```sh
cd web
nix shell nixpkgs#bun --command bun install
nix shell nixpkgs#bun --command bun run build
```

`bun run build` runs `bun build src/main.tsx --outdir dist --minify` (with
`NODE_ENV=production`, otherwise bun bundles the development build of React,
roughly doubling the bundle) and copies
`index.html`, `src/styles.css` and `public/` (fonts + favicon) into `dist/`.
The result is fully self-contained:

```
dist/
  index.html        <- entry; loads ./main.js and ./styles.css
  main.js           <- bundled, minified app
  styles.css
  favicon.svg
  fonts/*.woff2     <- iA Writer Quattro Regular/Italic/Bold
  fonts/LICENSE.md
```

Typecheck: `nix shell nixpkgs#bun --command bunx tsc --noEmit`

## Mock dev server

```sh
nix shell nixpkgs#bun --command bun run mock   # http://localhost:8790
```

`mock.ts` serves `dist/` (build first) plus an in-memory `/api` with eight
sample tasks covering all statuses, priorities, phases and a dependency chain.
`PORT=nnnn` overrides the port. State resets on restart.

## Serving from the Rust backend

The SPA uses **hash routing** (`#/`, `#/task/ID`, `#/task/ID/edit`, `#/new`),
so the server never sees client routes: it only has to serve `/` ->
`dist/index.html` and the static assets (`/main.js`, `/styles.css`,
`/favicon.svg`, `/fonts/*`) with correct content types (`font/woff2` for the
fonts). No SPA fallback route is needed. All API calls go to `/api/...` on the
same origin.

## API contract

Base path `/api`, same origin. Errors: non-2xx with `{"error": string}`.

- `GET /api/tasks` -> `{"tasks": [TaskSummary], "invalid": [{"path": string, "reason": string}]}`
- `GET /api/tasks/{id}` -> TaskDetail
- `POST /api/tasks` body `{"title": string, "priority"?: string, "effort"?: string, "type"?: string, "phase"?: string|null, "tags"?: string[], "dependencies"?: string[], "body"?: string}` -> TaskDetail (201)
- `PATCH /api/tasks/{id}` body: any subset of `{"title", "priority", "effort", "type", "phase", "tags", "dependencies", "owner", "body"}` -> TaskDetail
- `POST /api/tasks/{id}/status` body `{"status": "pending"|"in-progress"|"in-review"|"completed"|"blocked"|"cancelled"}` -> TaskDetail (backend handles auto-timestamps and workflow mode)
- `GET /api/config` -> `{"phases": [{"id": string|null, "name": string, "description": string|null, "due": string|null}], "workflow": "solo"|"pr-review"}`
- `GET /api/next?limit=5` -> `[{"rank": number, "id": string, "title": string, "status": string, "priority": string, "score": number, "reasons": [string]}]`

TaskSummary: `{"id", "title", "status", "priority", "effort", "type", "phase",
"tags": [], "dependencies": [], "group", "owner", "parent", "created_at",
"completed_at", "cancelled_at", "recurring", "ready": bool, "blockers": [string
ids]}` — nullable strings are `null`. TaskDetail = TaskSummary + `{"body":
string}`.

Enums: status `pending|in-progress|in-review|completed|blocked|cancelled`;
priority `low|medium|high|critical`; effort `small|medium|large`; type
`feature|bug|improvement|chore|docs`.

## Design notes

- Solarized Light palette as CSS custom properties in `src/styles.css`; status
  and priority render as small color chips (cancelled = strikethrough).
- Single centered column, `max-width: 40rem`; touch targets are at least 44px;
  sticky minimal header.
- Views: list grouped by phase (order from `/api/config`, unknown phases in a
  trailing "No phase" group) with status filter chips, client-side search and a
  "Next up" strip; detail with contextual status-transition buttons ("To
  review" only appears in `pr-review` workflow); one form for add and edit.
- `src/markdown.ts` is a deliberately minimal renderer: headings, paragraphs,
  bold/italic/inline code, fenced code, ordered/unordered lists, checkboxes,
  links (unsafe href schemes are left as plain text). Everything is
  HTML-escaped before formatting.
