import { useEffect, useMemo, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import {
  QueryClient,
  QueryClientProvider,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { api, errorMessage } from "./api";
import { Header } from "./components/Header";
import { Sidebar } from "./components/Sidebar";
import {
  navigate,
  newHref,
  rulesHref,
  runHref,
  runLogHref,
  tabHref,
  useRoute,
  type Pane,
} from "./router";
import {
  buildTabs,
  DEFAULT_TAB,
  DEFAULT_TODAY_PHASES,
  tabFromSlug,
  tabSlug,
  taskInTab,
} from "./tabs";
import type {
  Config,
  InvalidTask,
  NextItem,
  OngoingRun,
  SessionInfo,
  TaskSummary,
} from "./types";
import { Detail } from "./views/Detail";
import { List } from "./views/List";
import { RunLog } from "./views/RunLog";
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
      return (
        <Terminal
          key={`${pane.id}-${pane.agent ?? ""}`}
          id={pane.id}
          tab={tab}
          agent={pane.agent}
        />
      );
    case "runlog":
      return <RunLog key={`runlog-${pane.id}`} id={pane.id} tab={tab} />;
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

  const queryClient = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);

  // Server state lives in the query cache; mutations (in Detail/TaskForm)
  // invalidate these keys so the list and counts refresh without a reload.
  const configQ = useQuery({ queryKey: ["config"], queryFn: () => api.config() });
  const tasksQ = useQuery({ queryKey: ["tasks"], queryFn: () => api.tasks() });
  const nextQ = useQuery({ queryKey: ["next"], queryFn: () => api.next(1000) });
  const sessionsQ = useQuery({
    queryKey: ["sessions"],
    queryFn: () => api.sessions(),
    refetchInterval: 3000,
  });
  const runsQ = useQuery({
    queryKey: ["runs"],
    queryFn: () => api.runs(),
    refetchInterval: 3000,
  });

  const config: Config = configQ.data ?? {
    phases: [],
    workflow: "solo",
    today: DEFAULT_TODAY_PHASES,
    run_enabled: false,
    run_max_attempts: 0,
    run_agents: [],
    run_default_agent: "",
    version: "",
  };
  const tasks: TaskSummary[] | null = tasksQ.data?.tasks ?? null;
  const invalid: InvalidTask[] = tasksQ.data?.invalid ?? [];
  const sessions: SessionInfo[] = sessionsQ.data ?? [];
  const runs: OngoingRun[] = runsQ.data ?? [];
  const rankById = useMemo(
    () => new Map((nextQ.data ?? []).map((n: NextItem) => [n.id, n.rank])),
    [nextQ.data],
  );

  // Surface a task-fetch failure once; the banner stays dismissable.
  useEffect(() => {
    if (tasksQ.error) setError(errorMessage(tasksQ.error));
  }, [tasksQ.error]);

  const tabs = useMemo(() => buildTabs(config, tasks ?? []), [config, tasks]);
  const todayPhases = useMemo(() => new Set(config.today), [config.today]);

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

  const counts = useMemo(() => {
    const list = tasks ?? [];
    return new Map(
      tabs.map((t) => [
        t.key,
        list.filter((x) => taskInTab(x, t.key, todayPhases)).length,
      ]),
    );
  }, [tabs, tasks, todayPhases]);

  const tabForLinks = activeTab ? tabSlug(activeTab) : DEFAULT_TAB;
  const tabName = tabs.find((t) => t.key === activeTab)?.name ?? "";
  const pane = route.pane;
  const paneOpen = pane.kind !== "none";
  const activeSessionId = pane.kind === "run" ? pane.id : null;
  const activeRunId = pane.kind === "runlog" ? pane.id : null;

  async function killSession(id: string) {
    await api.killSession(id).catch(() => {});
    void queryClient.invalidateQueries({ queryKey: ["sessions"] });
  }

  async function cancelRun(id: string) {
    await api.cancelRun(id).catch(() => {});
    void queryClient.invalidateQueries({ queryKey: ["runs"] });
    void queryClient.invalidateQueries({ queryKey: ["tasks"] });
  }

  return (
    <>
      <Header
        date={date}
        version={config.version}
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
            // Carry the session's agent so reattach targets the same tool
            // instead of the default relaunching over it (#051).
            const s = sessions.find((x) => x.id === id);
            navigate(runHref(tabForLinks, id, s?.agent ?? null));
            setMenuOpen(false);
          }}
          onKillSession={(id) => void killSession(id)}
          runs={runs}
          activeRunId={activeRunId}
          onSelectRun={(id) => {
            navigate(runLogHref(tabForLinks, id));
            setMenuOpen(false);
          }}
          onCancelRun={(id) => void cancelRun(id)}
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
              today={config.today}
              invalid={invalid}
              rankById={rankById}
              activeTab={activeTab}
              tabName={tabName}
              newLink={newHref(tabForLinks)}
              runMaxAttempts={config.run_max_attempts}
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

const queryClient = new QueryClient();

const root = document.getElementById("root");
if (root)
  createRoot(root).render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  );
