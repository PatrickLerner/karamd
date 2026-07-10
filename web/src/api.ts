import type {
  Config,
  NextItem,
  OngoingRun,
  PreviewResponse,
  Rule,
  RulesResponse,
  SessionInfo,
  Status,
  TaskCreate,
  TaskDetail,
  TaskPatch,
  TasksResponse,
} from "./types";

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`/api${path}`, init);
  } catch {
    throw new Error("network error: could not reach the API");
  }
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`;
    try {
      const data: unknown = await res.json();
      if (
        typeof data === "object" &&
        data !== null &&
        "error" in data &&
        typeof (data as { error: unknown }).error === "string"
      ) {
        message = (data as { error: string }).error;
      }
    } catch {
      // non-JSON error body; keep the status line
    }
    throw new Error(message);
  }
  return (await res.json()) as T;
}

function jsonInit(method: string, body: unknown): RequestInit {
  return {
    method,
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  };
}

export const api = {
  tasks: (): Promise<TasksResponse> => request("/tasks"),
  task: (id: string): Promise<TaskDetail> =>
    request(`/tasks/${encodeURIComponent(id)}`),
  createTask: (body: TaskCreate): Promise<TaskDetail> =>
    request("/tasks", jsonInit("POST", body)),
  patchTask: (id: string, patch: TaskPatch): Promise<TaskDetail> =>
    request(`/tasks/${encodeURIComponent(id)}`, jsonInit("PATCH", patch)),
  setStatus: (id: string, status: Status): Promise<TaskDetail> =>
    request(
      `/tasks/${encodeURIComponent(id)}/status`,
      jsonInit("POST", { status }),
    ),
  config: (): Promise<Config> => request("/config"),
  next: (limit = 5): Promise<NextItem[]> => request(`/next?limit=${limit}`),
  rules: (): Promise<RulesResponse> => request("/rules"),
  putRules: (rules: Rule[]): Promise<RulesResponse> =>
    request("/rules", jsonInit("PUT", { rules })),
  previewRules: (rules: Rule[]): Promise<PreviewResponse> =>
    request("/rules/preview", jsonInit("POST", { rules })),
  sessions: (): Promise<SessionInfo[]> => request("/sessions"),
  killSession: (id: string): Promise<void> =>
    fetch(`/api/sessions/${encodeURIComponent(id)}`, { method: "DELETE" }).then(
      () => undefined,
    ),
  runs: (): Promise<OngoingRun[]> => request("/runs"),
  runLog: (id: string): Promise<{ log: string }> =>
    request(`/runs/${encodeURIComponent(id)}/log`),
  cancelRun: (id: string): Promise<{ cancelled: boolean }> =>
    request(`/runs/${encodeURIComponent(id)}/cancel`, jsonInit("POST", {})),
};
