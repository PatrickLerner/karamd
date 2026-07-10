// Mock backend for developing the SPA without the Rust server.
// Serves web/dist/ (run `bun run build` first) plus an in-memory /api.
import type {
  Config,
  PreviewCreated,
  Rule,
  Status,
  TaskDetail,
  TaskSummary,
  Trigger,
} from "./src/types";

const PORT = Number(process.env.PORT ?? 8790);
const DIST = new URL("./dist/", import.meta.url).pathname;

const config: Config = {
  phases: [
    {
      id: "mvp",
      name: "MVP",
      description: "Serve the vault read/write over HTTP",
      due: null,
    },
    { id: "polish", name: "Polish", description: null, due: null },
  ],
  workflow: "pr-review",
  // Mirrors `web.today` in `.taskmd.yaml`: the phases the Today tab merges,
  // in render order. Here MVP work lands in Today; Polish gets its own tab.
  today: ["mvp"],
  run_enabled: true,
  run_max_attempts: 3,
  version: "0.0.0-mock",
};

const tasks: TaskDetail[] = [
  {
    id: "001",
    title: "Serve SPA bundle from axum",
    status: "in-progress",
    priority: "high",
    effort: "medium",
    type: "feature",
    phase: "mvp",
    due: null,
    tags: ["web", "backend"],
    dependencies: [],
    group: null,
    owner: null,
    parent: null,
    created_at: "2026-06-18",
    completed_at: null,
    cancelled_at: null,
    recurring: null,
    ai_status: null,
    ai_attempts: null,
    ai_run_started: null,
    ai_last_error: null,
    ready: true,
    blockers: [],
    body: [
      "## Objective",
      "",
      "Embed `web/dist/` into the binary and serve it at `/`.",
      "",
      "## Tasks",
      "",
      "- [x] Pick embedding crate (`include_dir`)",
      "- [ ] Wire static routes into axum",
      "- [ ] Content-type for `woff2` and `svg`",
      "",
      "See [axum docs](https://docs.rs/axum) for the fallback service.",
    ].join("\n"),
  },
  {
    id: "002",
    title: "JSON API for task CRUD",
    status: "pending",
    priority: "high",
    effort: "large",
    type: "feature",
    phase: "mvp",
    due: null,
    tags: ["web", "api"],
    dependencies: ["001"],
    group: null,
    owner: null,
    parent: null,
    created_at: "2026-06-18",
    completed_at: null,
    cancelled_at: null,
    recurring: null,
    ai_status: null,
    ai_attempts: null,
    ai_run_started: null,
    ai_last_error: null,
    ready: false,
    blockers: ["001"],
    body: [
      "Implement `GET/POST /api/tasks` and `GET/PATCH /api/tasks/{id}`.",
      "",
      "```rust",
      "async fn list_tasks(State(vault): State<Vault>) -> Json<TasksResponse> {",
      "    // scan_dir + frontmatter parse",
      "}",
      "```",
      "",
      "Errors are `{\"error\": string}` with a non-2xx status.",
    ].join("\n"),
  },
  {
    id: "003",
    title: "Status endpoint with auto-timestamps",
    status: "blocked",
    priority: "medium",
    effort: "small",
    type: "feature",
    phase: "mvp",
    due: null,
    tags: ["api"],
    dependencies: ["002"],
    group: null,
    owner: null,
    parent: null,
    created_at: "2026-06-19",
    completed_at: null,
    cancelled_at: null,
    recurring: null,
    ai_status: null,
    ai_attempts: null,
    ai_run_started: null,
    ai_last_error: null,
    ready: false,
    blockers: ["002"],
    body: "`POST /api/tasks/{id}/status` stamps `completed_at` / `cancelled_at` the way `taskmd set --done` does, honouring the workflow mode.",
  },
  {
    id: "004",
    title: "Fix CRLF frontmatter parsing",
    status: "completed",
    priority: "critical",
    effort: "small",
    type: "bug",
    phase: "mvp",
    due: null,
    tags: ["parser"],
    dependencies: [],
    group: null,
    owner: null,
    parent: null,
    created_at: "2026-06-10",
    completed_at: "2026-06-20",
    cancelled_at: null,
    recurring: null,
    ai_status: null,
    ai_attempts: null,
    ai_run_started: null,
    ai_last_error: null,
    ready: true,
    blockers: [],
    body: "A synced vault can pick up CRLF line endings. An LF-only parser dropped the `recurring:` marker and **duplicated tasks**.",
  },
  {
    id: "005",
    title: "Document the rules file format in README",
    status: "pending",
    priority: "low",
    effort: "small",
    type: "docs",
    phase: "polish",
    due: null,
    tags: ["docs"],
    dependencies: [],
    group: null,
    owner: null,
    parent: null,
    created_at: "2026-06-22",
    completed_at: null,
    cancelled_at: null,
    recurring: null,
    ai_status: null,
    ai_attempts: null,
    ai_run_started: null,
    ai_last_error: null,
    ready: true,
    blockers: [],
    body: "Cover both triggers:\n\n1. `after_completion`\n2. `calendar` (with `lead_days`)\n\nLink to `recurring.example.yml`.",
  },
  {
    id: "006",
    title: "Rotate vault backups",
    status: "pending",
    priority: "medium",
    effort: "small",
    type: "chore",
    phase: null,
    due: null,
    tags: ["ops"],
    dependencies: [],
    group: null,
    owner: null,
    parent: null,
    created_at: "2026-07-01",
    completed_at: null,
    cancelled_at: null,
    recurring: "rotate-backups",
    ai_status: null,
    ai_attempts: null,
    ai_run_started: null,
    ai_last_error: null,
    ready: true,
    blockers: [],
    body: "Generated by karamd. Verify the latest snapshot restores cleanly, then prune snapshots older than *90 days*.",
  },
  {
    id: "007",
    title: "Prototype server-rendered frontend",
    status: "cancelled",
    priority: "low",
    effort: "medium",
    type: "improvement",
    phase: "polish",
    due: null,
    tags: ["web"],
    dependencies: [],
    group: null,
    owner: null,
    parent: null,
    created_at: "2026-06-12",
    completed_at: null,
    cancelled_at: "2026-06-25",
    recurring: null,
    ai_status: null,
    ai_attempts: null,
    ai_run_started: null,
    ai_last_error: null,
    ready: true,
    blockers: [],
    body: "Dropped in favour of the React SPA embedded in the binary.",
  },
  {
    id: "008",
    title: "Next-up scoring heuristic",
    status: "in-review",
    priority: "medium",
    effort: "medium",
    type: "improvement",
    phase: "polish",
    due: null,
    tags: ["api"],
    dependencies: [],
    group: null,
    owner: "patrick",
    parent: null,
    created_at: "2026-06-24",
    completed_at: null,
    cancelled_at: null,
    recurring: null,
    ai_status: null,
    ai_attempts: null,
    ai_run_started: null,
    ai_last_error: null,
    ready: true,
    blockers: [],
    body: "Score = priority weight + number of open dependents. Exposed at `GET /api/next`.",
  },
];

const invalid = [
  { path: "tasks/broken-task.md", reason: "missing required field: title" },
];

let rulesExist = true;
let rules: Rule[] = [
  {
    key: "rotate-backups",
    title: "Rotate vault backups",
    trigger: "after_completion",
    every_days: 90,
    priority: "medium",
    tags: ["ops"],
  },
  {
    key: "review-okrs",
    title: "Review quarterly OKRs",
    trigger: "monthly",
    day_of_month: 1,
    lead_days: 3,
    priority: "high",
  },
];

const TRIGGERS: Trigger[] = [
  "after_completion",
  "calendar",
  "monthly",
  "weekly",
  "nth_weekday",
];

const WEEKDAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

function slugify(input: string): string {
  return input
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

// Validate a rule set the way the Rust server would: unique keys, required
// fields present, and trigger-specific fields set. Returns an error string or
// null when the set is valid.
function validateRules(input: Rule[]): string | null {
  const keys = new Set<string>();
  for (const r of input) {
    if (typeof r.key !== "string" || r.key.trim() === "")
      return "rule is missing key";
    if (keys.has(r.key)) return `duplicate rule key: ${r.key}`;
    keys.add(r.key);
    if (typeof r.title !== "string" || r.title.trim() === "")
      return `rule ${r.key} is missing title`;
    if (!TRIGGERS.includes(r.trigger))
      return `rule ${r.key} has invalid trigger`;
    if (r.trigger === "after_completion" && typeof r.every_days !== "number")
      return `rule ${r.key} needs every_days`;
    if (r.trigger === "calendar" && (typeof r.annual !== "string" || typeof r.lead_days !== "number"))
      return `rule ${r.key} needs annual and lead_days`;
    if (r.trigger === "monthly" && (typeof r.day_of_month !== "number" || typeof r.lead_days !== "number"))
      return `rule ${r.key} needs day_of_month and lead_days`;
    if (r.trigger === "weekly" && !WEEKDAYS.includes(r.day_of_week ?? ""))
      return `rule ${r.key} needs a valid day_of_week`;
    if (r.trigger === "nth_weekday") {
      if (!WEEKDAYS.includes(r.day_of_week ?? ""))
        return `rule ${r.key} needs a valid day_of_week`;
      const wk = r.week;
      const ok = wk === "last" || (typeof wk === "number" && wk >= 1 && wk <= 4);
      if (!ok) return `rule ${r.key} needs week 1-4 or last`;
    }
    if (r.interval !== undefined && (typeof r.interval !== "number" || r.interval < 1))
      return `rule ${r.key} interval must be >= 1`;
    if (r.body !== undefined && r.body.trim() === "")
      return `rule ${r.key} has an empty body`;
  }
  return null;
}

// A crude stand-in for `generate`: pretend every after_completion rule with no
// matching open task is due today. Enough to exercise the preview UI.
function previewCreated(input: Rule[]): PreviewCreated[] {
  const nextNum = tasks.reduce((m, t) => Math.max(m, Number(t.id) || 0), 0) + 1;
  return input
    .filter((r) => r.trigger === "after_completion")
    .map((r, i) => ({
      filename: `tasks/${String(nextNum + i).padStart(3, "0")}-${slugify(r.title)}.md`,
      marker: r.key,
    }));
}

const STATUSES: Status[] = [
  "pending",
  "in-progress",
  "in-review",
  "completed",
  "blocked",
  "cancelled",
];
const TERMINAL: Status[] = ["completed", "cancelled"];
const OPEN: Status[] = ["pending", "in-progress", "in-review"];
const PRIORITY_SCORE: Record<string, number> = {
  critical: 4,
  high: 3,
  medium: 2,
  low: 1,
};

const today = () => new Date().toISOString().slice(0, 10);

function recomputeReadiness(): void {
  for (const t of tasks) {
    t.blockers = t.dependencies.filter((d) => {
      const dep = tasks.find((x) => x.id === d);
      return dep !== undefined && !TERMINAL.includes(dep.status);
    });
    t.ready = t.blockers.length === 0;
  }
}

function summary(t: TaskDetail): TaskSummary {
  const { body: _body, ...rest } = t;
  return rest;
}

function nextUp(limit: number) {
  return tasks
    .filter((t) => OPEN.includes(t.status) && t.ready)
    .map((t) => {
      const dependents = tasks.filter(
        (o) => o.dependencies.includes(t.id) && !TERMINAL.includes(o.status),
      ).length;
      const score = (PRIORITY_SCORE[t.priority] ?? 0) + dependents;
      const reasons = [
        `${t.priority} priority`,
        ...(dependents > 0 ? [`unblocks ${dependents} task(s)`] : []),
        ...(t.status === "in-progress" ? ["already started"] : []),
      ];
      return { t, score, reasons };
    })
    .sort((a, b) => b.score - a.score)
    .slice(0, limit)
    .map(({ t, score, reasons }, i) => ({
      rank: i + 1,
      id: t.id,
      title: t.title,
      status: t.status,
      priority: t.priority,
      score,
      reasons,
    }));
}

function json(data: unknown, status = 200): Response {
  return Response.json(data, { status });
}

function err(message: string, status: number): Response {
  return json({ error: message }, status);
}

function nextId(): string {
  const max = tasks.reduce((m, t) => Math.max(m, Number(t.id) || 0), 0);
  return String(max + 1).padStart(3, "0");
}

type Body = Record<string, unknown>;

const str = (b: Body, k: string): string | undefined =>
  typeof b[k] === "string" ? (b[k] as string) : undefined;
const strList = (b: Body, k: string): string[] | undefined =>
  Array.isArray(b[k]) ? (b[k] as string[]) : undefined;

function applyPatch(t: TaskDetail, b: Body): void {
  t.title = str(b, "title") ?? t.title;
  t.priority = (str(b, "priority") as TaskDetail["priority"]) ?? t.priority;
  t.effort = (str(b, "effort") as TaskDetail["effort"]) ?? t.effort;
  t.type = (str(b, "type") as TaskDetail["type"]) ?? t.type;
  if ("phase" in b) t.phase = str(b, "phase") ?? null;
  if ("owner" in b) t.owner = str(b, "owner") ?? null;
  t.tags = strList(b, "tags") ?? t.tags;
  t.dependencies = strList(b, "dependencies") ?? t.dependencies;
  t.body = str(b, "body") ?? t.body;
}

async function handleApi(req: Request, url: URL): Promise<Response> {
  const path = url.pathname.replace(/^\/api/, "");
  const method = req.method;

  if (path === "/config" && method === "GET") return json(config);
  if (path === "/next" && method === "GET") {
    return json(nextUp(Number(url.searchParams.get("limit") ?? 5)));
  }
  if (path === "/tasks" && method === "GET") {
    return json({ tasks: tasks.map(summary), invalid });
  }
  if (path === "/tasks" && method === "POST") {
    const b = (await req.json()) as Body;
    const title = str(b, "title")?.trim();
    if (!title) return err("title is required", 422);
    const t: TaskDetail = {
      id: nextId(),
      title,
      status: "pending",
      priority: "medium",
      effort: null,
      type: null,
      phase: null,
      due: null,
      tags: [],
      dependencies: [],
      group: null,
      owner: null,
      parent: null,
      created_at: today(),
      completed_at: null,
      cancelled_at: null,
      recurring: null,
      ai_status: null,
      ai_attempts: null,
      ai_run_started: null,
      ai_last_error: null,
      ready: true,
      blockers: [],
      body: "",
    };
    applyPatch(t, b);
    tasks.push(t);
    recomputeReadiness();
    return json(t, 201);
  }

  if (path === "/rules" && method === "GET") {
    return json({ exists: rulesExist, rules });
  }
  if (path === "/rules" && method === "PUT") {
    const b = (await req.json()) as Body;
    const next = Array.isArray(b.rules) ? (b.rules as Rule[]) : null;
    if (next === null) return err("rules must be a list", 422);
    const invalidReason = validateRules(next);
    if (invalidReason !== null) return err(invalidReason, 422);
    rules = next;
    rulesExist = true;
    return json({ exists: true, rules });
  }
  if (path === "/rules/preview" && method === "POST") {
    const b = (await req.json()) as Body;
    const next = Array.isArray(b.rules) ? (b.rules as Rule[]) : null;
    if (next === null) return err("rules must be a list", 422);
    const invalidReason = validateRules(next);
    if (invalidReason !== null) return err(invalidReason, 422);
    return json({ created: previewCreated(next) });
  }

  const m = path.match(/^\/tasks\/([^/]+)(\/status)?$/);
  if (!m) return err("not found", 404);
  const t = tasks.find((x) => x.id === decodeURIComponent(m[1]));
  if (!t) return err(`no task with id ${m[1]}`, 404);

  if (m[2] === "/status" && method === "POST") {
    const b = (await req.json()) as Body;
    const status = str(b, "status") as Status | undefined;
    if (!status || !STATUSES.includes(status))
      return err("invalid status", 422);
    t.status = status;
    t.completed_at = status === "completed" ? today() : null;
    t.cancelled_at = status === "cancelled" ? today() : null;
    recomputeReadiness();
    return json(t);
  }
  if (m[2] === undefined && method === "GET") return json(t);
  if (m[2] === undefined && method === "PATCH") {
    applyPatch(t, (await req.json()) as Body);
    recomputeReadiness();
    return json(t);
  }
  return err("method not allowed", 405);
}

async function serveStatic(pathname: string): Promise<Response> {
  if (pathname.includes("..")) return err("bad path", 400);
  const rel = pathname === "/" ? "index.html" : pathname.slice(1);
  const file = Bun.file(DIST + rel);
  if (await file.exists()) return new Response(file);
  return new Response("not found (did you run `bun run build`?)", {
    status: 404,
  });
}

const server = Bun.serve({
  port: PORT,
  async fetch(req) {
    const url = new URL(req.url);
    if (url.pathname === "/api" || url.pathname.startsWith("/api/")) {
      try {
        return await handleApi(req, url);
      } catch {
        return err("bad request body", 400);
      }
    }
    return serveStatic(url.pathname);
  },
});

console.log(`karamd web mock: http://localhost:${server.port}`);
