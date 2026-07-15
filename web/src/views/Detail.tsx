import { useState, type ReactNode } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { api, errorMessage } from "../api";
import { ErrorBanner } from "../components/Banner";
import { attemptsLabel, PriorityChip, StatusChip } from "../components/Chip";
import { renderMarkdown, stripWikiLinks } from "../markdown";
import { editHref, runHref, taskHref } from "../router";
import { isTerminal } from "../tabs";
import type { Phase, Status, TaskDetail as Task, Workflow } from "../types";

// The tag `karamd run` selects on: carrying it marks a task AI-executable.
// Keep in sync with `RUNNABLE_TAG` / `FAILED_TAG` in src/run.rs.
const RUNNABLE_TAG = "ai-runnable";
const FAILED_TAG = "ai-failed";

interface Transition {
  label: string;
  to: Status;
}

function transitions(status: Status, workflow: Workflow): Transition[] {
  switch (status) {
    case "pending":
      return [
        { label: "Start", to: "in-progress" },
        { label: "Complete", to: "completed" },
        { label: "Cancel", to: "cancelled" },
      ];
    case "in-progress":
      return [
        ...(workflow === "pr-review"
          ? [{ label: "To review", to: "in-review" } as Transition]
          : []),
        { label: "Complete", to: "completed" },
        { label: "Block", to: "blocked" },
        { label: "Cancel", to: "cancelled" },
      ];
    case "in-review":
      return [
        { label: "Complete", to: "completed" },
        { label: "Rework", to: "in-progress" },
        { label: "Cancel", to: "cancelled" },
      ];
    case "blocked":
      return [
        { label: "Resume", to: "in-progress" },
        { label: "Cancel", to: "cancelled" },
      ];
    case "completed":
    case "cancelled":
      return [{ label: "Reopen", to: "pending" }];
  }
}

// Task bodies conventionally start with `# <title>`; the detail view already
// shows the title as its own heading, so drop the duplicate before rendering.
function bodyWithoutTitle(body: string, title: string): string {
  const text = body.replace(/\r\n/g, "\n").trimStart();
  const nl = text.indexOf("\n");
  const first = (nl === -1 ? text : text.slice(0, nl)).trim();
  const heading = first.match(/^#\s+(.*)$/);
  if (heading && stripWikiLinks(heading[1]).trim() === stripWikiLinks(title).trim()) {
    return nl === -1 ? "" : text.slice(nl + 1);
  }
  return body;
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  if (children === null || children === undefined || children === "")
    return null;
  return (
    <>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </>
  );
}

export function Detail({ id, tab }: { id: string; tab: string }) {
  const queryClient = useQueryClient();
  const [dismissed, setDismissed] = useState(false);
  // Which configured agent the "Run with…" control launches; null = the
  // config default (#047).
  const [pickedAgent, setPickedAgent] = useState<string | null>(null);

  const taskQ = useQuery({
    queryKey: ["task", id],
    queryFn: () => api.task(id),
  });
  const configQ = useQuery({ queryKey: ["config"], queryFn: () => api.config() });
  const workflow: Workflow = configQ.data?.workflow ?? "solo";

  const mutation = useMutation({
    mutationFn: (to: Status) => api.setStatus(id, to),
    onMutate: () => setDismissed(false),
    onSuccess: (updated: Task) => {
      // Refresh this task and invalidate the shared list/ranking so a
      // completed task drops out of the list immediately.
      queryClient.setQueryData(["task", id], updated);
      void queryClient.invalidateQueries({ queryKey: ["tasks"] });
      void queryClient.invalidateQueries({ queryKey: ["next"] });
    },
  });

  // Toggle the `ai-runnable` tag. `edit --tag` replaces the whole set, so we
  // re-send every existing tag with the marker added or removed — never
  // dropping the task's other tags.
  const tagMutation = useMutation({
    mutationFn: (tags: string[]) => api.patchTask(id, { tags }),
    onMutate: () => setDismissed(false),
    onSuccess: (updated: Task) => {
      queryClient.setQueryData(["task", id], updated);
      void queryClient.invalidateQueries({ queryKey: ["tasks"] });
      void queryClient.invalidateQueries({ queryKey: ["next"] });
    },
  });

  // One-click phase assignment (#052): PATCH the phase, then refresh this task
  // and the shared list/ranking so the change lands everywhere at once.
  const phaseMutation = useMutation({
    mutationFn: (phase: string) => api.patchTask(id, { phase }),
    onMutate: () => setDismissed(false),
    onSuccess: (updated: Task) => {
      queryClient.setQueryData(["task", id], updated);
      void queryClient.invalidateQueries({ queryKey: ["tasks"] });
      void queryClient.invalidateQueries({ queryKey: ["next"] });
    },
  });

  const task = taskQ.data ?? null;
  const busy =
    mutation.isPending || tagMutation.isPending || phaseMutation.isPending;
  const errorSource =
    mutation.error ??
    tagMutation.error ??
    phaseMutation.error ??
    (task === null ? taskQ.error : null);
  const error = dismissed ? null : errorSource ? errorMessage(errorSource) : null;
  const apply = (to: Status) => mutation.mutate(to);

  if (error && task === null) {
    return (
      <div className="view">
        <ErrorBanner message={error} onDismiss={() => setDismissed(true)} />
      </div>
    );
  }
  if (task === null) {
    return (
      <div className="view">
        <p className="muted">Loading task…</p>
      </div>
    );
  }

  const runAgents = configQ.data?.run_agents ?? [];
  const defaultAgent = configQ.data?.run_default_agent ?? "";
  // The agent the run control targets: the explicit pick if still valid, else
  // the config default when configured, else the first listed agent.
  const selectedAgent =
    pickedAgent && runAgents.includes(pickedAgent)
      ? pickedAgent
      : runAgents.includes(defaultAgent)
        ? defaultAgent
        : (runAgents[0] ?? null);

  // Configured phases a task can be assigned to (the null "no phase" entry
  // excluded). Quick-assign only shows for an unphased, still-open task (#052).
  const assignablePhases = (configQ.data?.phases ?? []).filter(
    (p): p is Phase & { id: string } => p.id !== null,
  );
  const canAssignPhase =
    task.phase === null &&
    !isTerminal(task.status) &&
    assignablePhases.length > 0;

  const runnable = task.tags.includes(RUNNABLE_TAG);
  const parked = task.tags.includes(FAILED_TAG);
  const runEnabled = configQ.data?.run_enabled ?? false;
  const runMaxAttempts = configQ.data?.run_max_attempts ?? 0;
  // Show the run-state block whenever the task carries any `karamd run` marker,
  // including a recorded error or start time with no status (hand-edited or a
  // future run path), so the data is never silently hidden.
  const hasRunState =
    task.ai_status !== null ||
    task.ai_attempts !== null ||
    task.ai_run_started !== null ||
    task.ai_last_error !== null ||
    parked;
  const toggleRunnable = () =>
    tagMutation.mutate(
      runnable
        ? task.tags.filter((t) => t !== RUNNABLE_TAG)
        : [...task.tags, RUNNABLE_TAG],
    );

  const depLinks = task.dependencies.map((d, i) => (
    <span key={d}>
      {i > 0 && ", "}
      <a href={taskHref(tab, d)}>{d}</a>
    </span>
  ));

  return (
    <div className="view">
      {error && (
        <ErrorBanner message={error} onDismiss={() => setDismissed(true)} />
      )}
      <p className="task-id detail-id">{task.id}</p>
      <h1>{stripWikiLinks(task.title)}</h1>
      <p className="detail-chips">
        <StatusChip status={task.status} />
        <PriorityChip priority={task.priority} />
        {!task.ready && task.blockers.length > 0 && (
          <span className="muted blocked-note">
            ⊘ waits on {task.blockers.join(", ")}
          </span>
        )}
      </p>
      <div className="actions">
        {transitions(task.status, workflow).map((t) => (
          <button
            key={t.to + t.label}
            type="button"
            disabled={busy}
            onClick={() => void apply(t.to)}
          >
            {t.label}
          </button>
        ))}
        <a className="btn" href={editHref(tab, id)}>
          Edit
        </a>
        {runAgents.length > 1 && (
          <select
            className="run-agent-select"
            aria-label="AI tool to launch"
            value={selectedAgent ?? ""}
            onChange={(e) => setPickedAgent(e.target.value)}
          >
            {runAgents.map((a) => (
              <option key={a} value={a}>
                {a}
              </option>
            ))}
          </select>
        )}
        <a className="btn" href={runHref(tab, id, selectedAgent)}>
          {selectedAgent ? `Run with ${selectedAgent}` : "Run in terminal"}
        </a>
        <button
          type="button"
          className="toggle"
          disabled={busy}
          aria-pressed={runnable}
          onClick={toggleRunnable}
          title={
            runEnabled
              ? runnable
                ? "AI execution on: karamd run picks this task up"
                : "Mark this task for AI execution by karamd run"
              : "run is disabled in this vault (set run.enabled); the tag has no effect yet"
          }
        >
          {runnable ? "🤖 AI-runnable ✓" : "🤖 AI-runnable"}
        </button>
      </div>
      {canAssignPhase && (
        <div
          className="actions phase-assign"
          role="group"
          aria-label="Assign phase"
        >
          <span className="phase-assign-label">Assign phase:</span>
          {assignablePhases.map((p) => (
            <button
              key={p.id}
              type="button"
              disabled={busy}
              onClick={() => phaseMutation.mutate(p.id)}
            >
              {p.name}
            </button>
          ))}
        </div>
      )}
      {runnable && !runEnabled && (
        <p className="muted run-hint">
          Tagged for AI execution, but <code>run</code> is disabled in this
          vault. Set <code>run.enabled: true</code> for <code>karamd run</code>{" "}
          to pick it up.
        </p>
      )}
      {hasRunState && (
        <div className="run-state">
          <h2>AI run state</h2>
          <dl className="frontmatter">
            <Field label="status">
              {task.ai_status === "running"
                ? "running"
                : parked
                  ? "parked"
                  : task.ai_status}
            </Field>
            <Field label="attempts">
              {attemptsLabel(task.ai_attempts, runMaxAttempts) || null}
            </Field>
            <Field label="started">{task.ai_run_started}</Field>
            <Field label="last error">
              {task.ai_last_error ? (
                <code className="run-error">{task.ai_last_error}</code>
              ) : null}
            </Field>
          </dl>
        </div>
      )}
      <dl className="frontmatter">
        <Field label="effort">{task.effort}</Field>
        <Field label="type">{task.type}</Field>
        <Field label="phase">{task.phase}</Field>
        <Field label="due">{task.due}</Field>
        <Field label="tags">
          {task.tags.length > 0 ? task.tags.join(", ") : null}
        </Field>
        <Field label="dependencies">
          {task.dependencies.length > 0 ? depLinks : null}
        </Field>
        <Field label="group">{task.group}</Field>
        <Field label="owner">{task.owner}</Field>
        <Field label="parent">
          {task.parent ? (
            <a href={taskHref(tab, task.parent)}>{task.parent}</a>
          ) : null}
        </Field>
        <Field label="created">{task.created_at}</Field>
        <Field label="completed">{task.completed_at}</Field>
        <Field label="cancelled">{task.cancelled_at}</Field>
        <Field label="recurring">{task.recurring}</Field>
      </dl>
      {bodyWithoutTitle(task.body, task.title).trim() !== "" && (
        <div
          className="markdown"
          dangerouslySetInnerHTML={{
            __html: renderMarkdown(bodyWithoutTitle(task.body, task.title)),
          }}
        />
      )}
    </div>
  );
}
