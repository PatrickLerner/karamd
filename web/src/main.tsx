import { useEffect, useMemo, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { api, errorMessage } from "./api";
import { Header } from "./components/Header";
import { Sidebar } from "./components/Sidebar";
import {
  navigate,
  newHref,
  rulesHref,
  runHref,
  tabHref,
  useRoute,
  type Pane,
} from "./router";
import {
  buildTabs,
  DEFAULT_TAB,
  tabFromSlug,
  tabSlug,
  taskInTab,
} from "./tabs";
import type {
  Config,
  InvalidTask,
  NextItem,
  SessionInfo,
  TaskSummary,
} from "./types";
import { Detail } from "./views/Detail";
import { List } from "./views/List";
import { Rules } from "./views/Rules";
import { TaskForm } from "./views/TaskForm";
import { Terminal } from "./views/Terminal";

function paneFor(pane: Pane, tab: string): ReactNode {
  switch (pane.kind) {
    case "detail":
      return <Detail key={pane.id} id={pane.id} tab={tab} />;
    case "edit":
      return <TaskForm key={`edit-${pane.id}`} id={pane.id} tab={tab} />;
    case "run":
      return <Terminal key={pane.id} id={pane.id} tab={tab} />;
    case "new":
      return <TaskForm key="new" tab={tab} />;
    case "rules":
      return <Rules />;
    case "none":
      return null;
  }
}

function today(): string {
  return new Intl.DateTimeFormat(undefined, {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: "numeric",
  }).format(new Date());
}

function App() {
  const route = useRoute();
  const date = useMemo(today, []);

  const [config, setConfig] = useState<Config>({ phases: [], workflow: "solo" });
  const [tasks, setTasks] = useState<TaskSummary[] | null>(null);
  const [invalid, setInvalid] = useState<InvalidTask[]>([]);
  const [rankById, setRankById] = useState<Map<string, number>>(new Map());
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);

  const tabs = useMemo(() => buildTabs(config, tasks ?? []), [config, tasks]);

  // The active tab comes from the URL. A bare `#/` (or an unknown tab) redirects
  // to the default view so the URL is always canonical and reload-safe.
  const urlTab = route.tab ? tabFromSlug(route.tab) : null;
  const activeTab =
    urlTab && tabs.some((t) => t.key === urlTab) ? urlTab : null;
  useEffect(() => {
    if (tabs.length === 0) return;
    if (activeTab) return;
    const fallback = tabs.find((t) => t.key === DEFAULT_TAB) ?? tabs[0];
    navigate(tabHref(tabSlug(fallback.key)));
  }, [tabs, activeTab]);

  useEffect(() => {
    api.config().then(setConfig).catch(() => {});
  }, []);

  // Tasks + ranks refresh on every navigation, so the list reflects edits and
  // status changes made in the detail pane. Cheap against a local vault.
  const routeKey = `${route.tab}|${route.pane.kind}|${
    "id" in route.pane ? route.pane.id : ""
  }`;
  useEffect(() => {
    api
      .tasks()
      .then((r) => {
        setTasks(r.tasks);
        setInvalid(r.invalid);
      })
      .catch((e: unknown) => setError(errorMessage(e)));
    api
      .next(1000)
      .then((items: NextItem[]) =>
        setRankById(new Map(items.map((n) => [n.id, n.rank]))),
      )
      .catch(() => {});
  }, [routeKey]);

  // Sessions poll so the sidebar dot flips live -> exited without a reload.
  useEffect(() => {
    let alive = true;
    const load = () =>
      api
        .sessions()
        .then((s) => {
          if (alive) setSessions(s);
        })
        .catch(() => {});
    load();
    const timer = setInterval(load, 3000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  const counts = useMemo(() => {
    const list = tasks ?? [];
    return new Map(
      tabs.map((t) => [t.key, list.filter((x) => taskInTab(x, t.key)).length]),
    );
  }, [tabs, tasks]);

  const tabForLinks = activeTab ? tabSlug(activeTab) : DEFAULT_TAB;
  const tabName = tabs.find((t) => t.key === activeTab)?.name ?? "";
  const pane = route.pane;
  const paneOpen = pane.kind !== "none";
  const activeSessionId = pane.kind === "run" ? pane.id : null;

  async function killSession(id: string) {
    await api.killSession(id).catch(() => {});
    api
      .sessions()
      .then(setSessions)
      .catch(() => {});
  }

  return (
    <>
      <Header
        date={date}
        newLink={newHref(tabForLinks)}
        onToggleMenu={() => setMenuOpen((o) => !o)}
      />
      <div className="app-body">
        <Sidebar
          tabs={tabs}
          counts={counts}
          activeTab={activeTab}
          onSelectTab={(key) => {
            navigate(tabHref(tabSlug(key)));
            setMenuOpen(false);
          }}
          sessions={sessions}
          activeSessionId={activeSessionId}
          onSelectSession={(id) => {
            navigate(runHref(tabForLinks, id));
            setMenuOpen(false);
          }}
          onKillSession={(id) => void killSession(id)}
          onOpenRules={() => {
            navigate(rulesHref(tabForLinks));
            setMenuOpen(false);
          }}
          rulesActive={pane.kind === "rules"}
          open={menuOpen}
          onClose={() => setMenuOpen(false)}
        />
        <div className={`panes${paneOpen ? " pane-open" : ""}`}>
          <main className="list-col">
            <List
              tasks={tasks}
              phases={config.phases}
              invalid={invalid}
              rankById={rankById}
              activeTab={activeTab}
              tabName={tabName}
              error={error}
              onDismissError={() => setError(null)}
            />
          </main>
          <section className="detail-col">
            {paneOpen ? (
              paneFor(pane, tabForLinks)
            ) : (
              <p className="detail-empty muted">Select a task</p>
            )}
          </section>
        </div>
      </div>
    </>
  );
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<App />);
