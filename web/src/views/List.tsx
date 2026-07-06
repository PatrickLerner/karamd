import { useMemo, useState } from "react";
import { ErrorBanner } from "../components/Banner";
import { PriorityChip, StatusChip } from "../components/Chip";
import { stripWikiLinks } from "../markdown";
import { taskHref } from "../router";
import { DONE_TAB, tabSlug, taskInTab } from "../tabs";
import type { InvalidTask, Phase, TaskSummary } from "../types";

interface Group {
  name: string;
  tasks: TaskSummary[];
}

function TaskRow({ task, tab }: { task: TaskSummary; tab: string }) {
  return (
    <a className="task-row" href={taskHref(tab, task.id)}>
      <span className="task-id">{task.id}</span>
      <span className="task-title">
        {stripWikiLinks(task.title)}
        {!task.ready && task.blockers.length > 0 && (
          <span className="muted blocked-note">
            {" "}
            ⊘ waits on {task.blockers.join(", ")}
          </span>
        )}
      </span>
      <span className="task-chips">
        <StatusChip status={task.status} />
        <PriorityChip priority={task.priority} />
      </span>
    </a>
  );
}

export function List({
  tasks,
  phases,
  today,
  invalid,
  rankById,
  activeTab,
  tabName,
  newLink,
  error,
  onDismissError,
}: {
  tasks: TaskSummary[] | null;
  phases: Phase[];
  today: string[];
  invalid: InvalidTask[];
  rankById: Map<string, number>;
  activeTab: string | null;
  tabName: string;
  newLink: string;
  error: string | null;
  onDismissError: () => void;
}) {
  const [query, setQuery] = useState("");

  const groups = useMemo<Group[] | null>(() => {
    if (tasks === null || activeTab === null) return null;
    const q = query.trim().toLowerCase();
    const todaySet = new Set(today);
    const inTab = tasks.filter((t) => taskInTab(t, activeTab, todaySet));
    const filtered =
      q === ""
        ? inTab
        : inTab.filter(
            (t) =>
              t.title.toLowerCase().includes(q) ||
              t.id.toLowerCase().includes(q),
          );
    const done = activeTab === DONE_TAB;
    const sorted = [...filtered].sort((a, b) => {
      if (done) {
        const da = a.completed_at ?? a.cancelled_at ?? "";
        const db = b.completed_at ?? b.cancelled_at ?? "";
        return db.localeCompare(da) || b.id.localeCompare(a.id);
      }
      const ra = rankById.get(a.id) ?? Number.POSITIVE_INFINITY;
      const rb = rankById.get(b.id) ?? Number.POSITIVE_INFINITY;
      return ra - rb || a.id.localeCompare(b.id);
    });

    // Group by phase, in config order, keeping each phase's headline.
    const byPhase = new Map<string | null, TaskSummary[]>();
    for (const t of sorted) {
      const arr = byPhase.get(t.phase);
      if (arr) arr.push(t);
      else byPhase.set(t.phase, [t]);
    }
    const out: Group[] = [];
    for (const p of phases) {
      if (p.id === null) continue;
      const arr = byPhase.get(p.id);
      if (arr && arr.length > 0) out.push({ name: p.name, tasks: arr });
      byPhase.delete(p.id);
    }
    // Phases not present in the server config land here. Order them
    // deterministically (the Today merge phases first, in their configured
    // sequence; then any other phase alphabetically; "No phase" last) so the
    // group order never depends on task/rank insertion order.
    const rank = (key: string | null): number => {
      if (key === null) return Number.MAX_SAFE_INTEGER;
      const i = today.indexOf(key);
      return i === -1 ? 1_000_000 : i;
    };
    const leftover = [...byPhase.entries()].sort(
      ([a], [b]) => rank(a) - rank(b) || String(a).localeCompare(String(b)),
    );
    for (const [key, arr] of leftover) {
      if (arr.length > 0) out.push({ name: key ?? "No phase", tasks: arr });
    }
    return out;
  }, [tasks, phases, today, activeTab, query, rankById]);

  const total = groups?.reduce((n, g) => n + g.tasks.length, 0) ?? 0;
  // Single-phase tabs don't need a headline echoing the tab name.
  const showHeadings = (groups?.length ?? 0) > 1;

  return (
    <div className="view list-view">
      {error && <ErrorBanner message={error} onDismiss={onDismissError} />}
      <div className="list-head">
        <h1>{tabName}</h1>
        {groups !== null && <span className="list-count">{total}</span>}
        <a href={newLink} className="new-task">
          + New
        </a>
      </div>
      <div className="filter-bar">
        <input
          type="search"
          placeholder="Search title or id"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Search tasks"
        />
      </div>
      {groups === null && !error && <p className="muted">Loading tasks…</p>}
      {groups !== null && total === 0 && <p className="muted">Nothing here.</p>}
      {groups?.map((g) => (
        <section key={g.name} className="phase-group">
          {showHeadings && <h2>{g.name}</h2>}
          <div className="task-list">
            {g.tasks.map((t) => (
              <TaskRow
                key={t.id}
                task={t}
                tab={activeTab ? tabSlug(activeTab) : ""}
              />
            ))}
          </div>
        </section>
      ))}
      {invalid.length > 0 && (
        <section className="invalid-files muted">
          <h2>Invalid task files</h2>
          {invalid.map((f) => (
            <p key={f.path}>
              <code>{f.path}</code> — {f.reason}
            </p>
          ))}
        </section>
      )}
    </div>
  );
}
