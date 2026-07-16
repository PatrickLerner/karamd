import { useEffect, useRef, useState, type ReactNode } from "react";
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
// localStorage key remembering the last agent picked in the split Run button.
const RUN_AGENT_KEY = "karamd.runAgent";

interface Transition {
  label: string;
  to: Status;
}

function transitions(status: Status, workflow: Workflow): Transition[] {
  switch (status) {
    case "pending":
      return [
        { label: "Complete", to: "completed" },
        { label: "Start", to: "in-progress" },
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
  // Which agent the split Run button launches. Seeded from localStorage so the
  // last pick sticks across reloads and tasks (#047/#051); null falls back to
  // the config default. `agentMenuOpen` drives the little switcher on the
  // button's caret.
  const [pickedAgent, setPickedAgent] = useState<string | null>(() => {
    try {
      return localStorage.getItem(RUN_AGENT_KEY);
    } catch {
      return null;
    }
  });
  const [agentMenuOpen, setAgentMenuOpen] = useState(false);
  const runSplitRef = useRef<HTMLDivElement | null>(null);
  // The phase switcher (#054): a dropdown to move the task to any configured
  // phase, styled like the run switcher. Replaces #052's flat quick-assign row.
  const [phaseMenuOpen, setPhaseMenuOpen] = useState(false);
  const phaseSplitRef = useRef<HTMLDivElement | null>(null);
  // The status switcher (#055): a dropdown over the available status
  // transitions, styled like the phase/run switchers.
  const [statusMenuOpen, setStatusMenuOpen] = useState(false);
  const statusSplitRef = useRef<HTMLDivElement | null>(null);

  function chooseAgent(agent: string) {
    setPickedAgent(agent);
    try {
      localStorage.setItem(RUN_AGENT_KEY, agent);
    } catch {
      // private mode / disabled storage: the pick still applies this session.
    }
    setAgentMenuOpen(false);
  }

  // Close the agent switcher on an outside click or Escape.
  useEffect(() => {
    if (!agentMenuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (runSplitRef.current && !runSplitRef.current.contains(e.target as Node))
        setAgentMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setAgentMenuOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [agentMenuOpen]);

  // Close the phase switcher on an outside click or Escape (mirrors the agent
  // switcher above).
  useEffect(() => {
    if (!phaseMenuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (
        phaseSplitRef.current &&
        !phaseSplitRef.current.contains(e.target as Node)
      )
        setPhaseMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setPhaseMenuOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [phaseMenuOpen]);

  // Close the status switcher on an outside click or Escape.
  useEffect(() => {
    if (!statusMenuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (
        statusSplitRef.current &&
        !statusSplitRef.current.contains(e.target as Node)
      )
        setStatusMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setStatusMenuOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [statusMenuOpen]);

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
      setStatusMenuOpen(false);
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

  // Phase switch (#054): PATCH the phase, then refresh this task and the shared
  // list/ranking so the change lands everywhere at once, and close the menu.
  const phaseMutation = useMutation({
    mutationFn: (phase: string) => api.patchTask(id, { phase }),
    onMutate: () => setDismissed(false),
    onSuccess: (updated: Task) => {
      queryClient.setQueryData(["task", id], updated);
      void queryClient.invalidateQueries({ queryKey: ["tasks"] });
      void queryClient.invalidateQueries({ queryKey: ["next"] });
      setPhaseMenuOpen(false);
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

  // Configured phases a task can be moved to (the null "no phase" entry
  // excluded). The switcher shows for any open task with at least one phase;
  // unlike #052 it is not gated on the task currently being unphased (#054).
  const assignablePhases = (configQ.data?.phases ?? []).filter(
    (p): p is Phase & { id: string } => p.id !== null,
  );
  const canChangePhase = !isTerminal(task.status) && assignablePhases.length > 0;
  // Available status transitions. 2+ collapse into a dropdown (#055); a single
  // one (Reopen) stays a plain button.
  const statusTransitions = transitions(task.status, workflow);
  // Label for the trigger: the current phase's name, its raw id if it is not a
  // configured phase, or "No phase" when unset.
  const currentPhaseName =
    task.phase === null
      ? "No phase"
      : (assignablePhases.find((p) => p.id === task.phase)?.name ?? task.phase);

  const runnable = task.tags.includes(RUNNABLE_TAG);
  const parked = task.tags.includes(FAILED_TAG);
  // Tags shown as plain text: everything except ai-runnable, which the inline
  // toggle in the tags row owns.
  const otherTags = task.tags.filter((t) => t !== RUNNABLE_TAG);
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
        {statusTransitions.length === 1 ? (
          <button
            type="button"
            disabled={busy}
            onClick={() => void apply(statusTransitions[0].to)}
          >
            {statusTransitions[0].label}
          </button>
        ) : (
          <div className="phase-split" ref={statusSplitRef}>
            <button
              type="button"
              className="phase-split-toggle"
              aria-haspopup="menu"
              aria-expanded={statusMenuOpen}
              aria-label="Change status"
              disabled={busy}
              onClick={() => setStatusMenuOpen((o) => !o)}
            >
              Status: {task.status}{" "}
              <span className="phase-split-caret" aria-hidden="true">
                ▾
              </span>
            </button>
            {statusMenuOpen && (
              <ul className="run-split-menu" role="menu">
                {statusTransitions.map((t) => (
                  <li key={t.to + t.label} role="none">
                    <button
                      type="button"
                      role="menuitem"
                      disabled={busy}
                      onClick={() => void apply(t.to)}
                    >
                      <span className="run-split-check" aria-hidden="true" />
                      {t.label}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
        <a className="btn" href={editHref(tab, id)}>
          Edit
        </a>
        <div className="run-split" ref={runSplitRef}>
          <a className="btn run-split-main" href={runHref(tab, id, selectedAgent)}>
            {selectedAgent ? `Run with ${selectedAgent}` : "Run in terminal"}
          </a>
          {runAgents.length > 1 && (
            <button
              type="button"
              className="run-split-toggle"
              aria-haspopup="menu"
              aria-expanded={agentMenuOpen}
              aria-label="Choose AI tool"
              onClick={() => setAgentMenuOpen((o) => !o)}
            >
              ▾
            </button>
          )}
          {agentMenuOpen && (
            <ul className="run-split-menu" role="menu">
              {runAgents.map((a) => (
                <li key={a} role="none">
                  <button
                    type="button"
                    role="menuitemradio"
                    aria-checked={a === selectedAgent}
                    onClick={() => chooseAgent(a)}
                  >
                    <span className="run-split-check" aria-hidden="true">
                      {a === selectedAgent ? "✓" : ""}
                    </span>
                    {a}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
        {canChangePhase && (
          <div className="phase-split" ref={phaseSplitRef}>
            <button
              type="button"
              className="phase-split-toggle"
              aria-haspopup="menu"
              aria-expanded={phaseMenuOpen}
              aria-label="Change phase"
              disabled={busy}
              onClick={() => setPhaseMenuOpen((o) => !o)}
            >
              Phase: {currentPhaseName}{" "}
              <span className="phase-split-caret" aria-hidden="true">
                ▾
              </span>
            </button>
            {phaseMenuOpen && (
              <ul className="run-split-menu" role="menu">
                {assignablePhases.map((p) => {
                  const active = p.id === task.phase;
                  return (
                    <li key={p.id} role="none">
                      <button
                        type="button"
                        role="menuitemradio"
                        aria-checked={active}
                        disabled={busy || active}
                        onClick={() => phaseMutation.mutate(p.id)}
                      >
                        <span className="run-split-check" aria-hidden="true">
                          {active ? "✓" : ""}
                        </span>
                        {p.name}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        )}
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
          <span className="tags-value">
            {otherTags.length > 0 && <span>{otherTags.join(", ")}</span>}
            <button
              type="button"
              className={`tag-toggle${runnable ? " on" : ""}`}
              disabled={busy}
              aria-pressed={runnable}
              onClick={toggleRunnable}
              title={
                runEnabled
                  ? runnable
                    ? "AI execution on: karamd run picks this task up (click to remove)"
                    : "Mark this task for AI execution by karamd run"
                  : "run is disabled in this vault (set run.enabled); the tag has no effect yet"
              }
            >
              {runnable ? "ai-runnable" : "+ai-runnable"}
            </button>
          </span>
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
