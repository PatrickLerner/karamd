export type Status =
  | "pending"
  | "in-progress"
  | "in-review"
  | "completed"
  | "blocked"
  | "cancelled";

export type Priority = "low" | "medium" | "high" | "critical";
export type Effort = "small" | "medium" | "large";
export type TaskType = "feature" | "bug" | "improvement" | "chore" | "docs";
export type Workflow = "solo" | "pr-review";

// Filter/display order: active work first, terminal states last (completed last).
export const STATUSES: Status[] = [
  "pending",
  "in-progress",
  "in-review",
  "blocked",
  "cancelled",
  "completed",
];
export const PRIORITIES: Priority[] = ["low", "medium", "high", "critical"];
export const EFFORTS: Effort[] = ["small", "medium", "large"];
export const TASK_TYPES: TaskType[] = [
  "feature",
  "bug",
  "improvement",
  "chore",
  "docs",
];

export interface TaskSummary {
  id: string;
  title: string;
  status: Status;
  priority: Priority;
  effort: Effort | null;
  type: TaskType | null;
  phase: string | null;
  due: string | null;
  tags: string[];
  dependencies: string[];
  group: string | null;
  owner: string | null;
  parent: string | null;
  created_at: string | null;
  completed_at: string | null;
  cancelled_at: string | null;
  recurring: string | null;
  ready: boolean;
  blockers: string[];
}

export interface TaskDetail extends TaskSummary {
  body: string;
}

export interface InvalidTask {
  path: string;
  reason: string;
}

export interface TasksResponse {
  tasks: TaskSummary[];
  invalid: InvalidTask[];
}

export interface Phase {
  id: string | null;
  name: string;
  description: string | null;
  due: string | null;
}

export interface Config {
  phases: Phase[];
  workflow: Workflow;
  // Phase ids the "Today" tab merges, in render order. Server-resolved: it
  // always carries a value (the server applies the default when config omits
  // `web.today`), so the client never hardcodes the grouping.
  today: string[];
  // karamd's version, shown small and dim in the header.
  version: string;
}

export interface NextItem {
  rank: number;
  id: string;
  title: string;
  status: string;
  priority: string;
  score: number;
  reasons: string[];
}

export interface TaskCreate {
  title: string;
  priority?: string;
  effort?: string;
  type?: string;
  phase?: string | null;
  due?: string | null;
  tags?: string[];
  dependencies?: string[];
  body?: string;
}

export type TaskPatch = Partial<{
  title: string;
  priority: string;
  effort: string;
  type: string;
  phase: string | null;
  due: string | null;
  tags: string[];
  dependencies: string[];
  owner: string | null;
  body: string;
}>;

export type Trigger =
  | "after_completion"
  | "calendar"
  | "monthly"
  | "weekly"
  | "nth_weekday";

export const TRIGGERS: Trigger[] = [
  "after_completion",
  "calendar",
  "monthly",
  "weekly",
  "nth_weekday",
];

// Weekday form accepted by weekly / nth_weekday rules.
export type Weekday = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun";

export const WEEKDAYS: Weekday[] = [
  "mon",
  "tue",
  "wed",
  "thu",
  "fri",
  "sat",
  "sun",
];

// nth_weekday `week`: 1-4 or the keyword "last".
export type Week = number | "last";

export interface Rule {
  key: string;
  title: string;
  trigger: Trigger;
  every_days?: number;
  annual?: string;
  day_of_month?: number;
  day_of_week?: Weekday;
  week?: Week;
  lead_days?: number;
  interval?: number;
  anchor?: string;
  phase?: string;
  priority?: string;
  tags?: string[];
  body?: string;
}

export interface RulesResponse {
  exists: boolean;
  rules: Rule[];
}

export interface PreviewCreated {
  filename: string;
  marker: string;
}

export interface PreviewResponse {
  created: PreviewCreated[];
}

export interface SessionInfo {
  id: string;
  title: string;
  running: boolean;
  exit_code: number | null;
}
