import { useEffect, useState, type ReactNode } from "react";
import { api, errorMessage } from "../api";
import { ErrorBanner } from "../components/Banner";
import { PriorityChip, StatusChip } from "../components/Chip";
import { renderMarkdown, stripWikiLinks } from "../markdown";
import { editHref, runHref, taskHref } from "../router";
import type { Status, TaskDetail as Task, Workflow } from "../types";

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
  const [task, setTask] = useState<Task | null>(null);
  const [workflow, setWorkflow] = useState<Workflow>("solo");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setTask(null);
    setError(null);
    api
      .task(id)
      .then(setTask)
      .catch((e: unknown) => setError(errorMessage(e)));
    api
      .config()
      .then((c) => setWorkflow(c.workflow))
      .catch(() => {});
  }, [id]);

  async function apply(to: Status) {
    setBusy(true);
    setError(null);
    try {
      setTask(await api.setStatus(id, to));
    } catch (e: unknown) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  if (error && task === null) {
    return (
      <div className="view">
        <ErrorBanner message={error} onDismiss={() => setError(null)} />
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

  const depLinks = task.dependencies.map((d, i) => (
    <span key={d}>
      {i > 0 && ", "}
      <a href={taskHref(tab, d)}>{d}</a>
    </span>
  ));

  return (
    <div className="view">
      {error && <ErrorBanner message={error} onDismiss={() => setError(null)} />}
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
      </div>
      <dl className="frontmatter">
        <Field label="effort">{task.effort}</Field>
        <Field label="type">{task.type}</Field>
        <Field label="phase">{task.phase}</Field>
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
