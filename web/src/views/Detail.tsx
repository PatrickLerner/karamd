import { useState, type ReactNode } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { api, errorMessage } from "../api";
import { ErrorBanner } from "../components/Banner";
import { PriorityChip, StatusChip } from "../components/Chip";
import { renderMarkdown, stripWikiLinks } from "../markdown";
import { editHref, runHref, taskHref } from "../router";
import type { Status, TaskDetail as Task, Workflow } from "../types";

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

  const task = taskQ.data ?? null;
  const busy = mutation.isPending || tagMutation.isPending;
  const errorSource =
    mutation.error ?? tagMutation.error ?? (task === null ? taskQ.error : null);
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

  const runnable = task.tags.includes(RUNNABLE_TAG);
  const parked = task.tags.includes(FAILED_TAG);
  const runEnabled = configQ.data?.run_enabled ?? false;
  const runMaxAttempts = configQ.data?.run_max_attempts ?? 0;
  // Show the run-state block whenever the task carries any `karamd run` marker.
  const hasRunState =
    task.ai_status !== null || task.ai_attempts !== null || parked;
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
        <a className="btn" href={runHref(tab, id)}>
          Run with Claude
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
              {task.ai_status ?? (parked ? "parked" : null)}
            </Field>
            <Field label="attempts">
              {task.ai_attempts !== null
                ? `${task.ai_attempts} / ${runMaxAttempts}${
                    parked ? " (parked)" : ""
                  }`
                : null}
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
