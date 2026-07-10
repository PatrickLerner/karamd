import type { Tab } from "../tabs";
import type { OngoingRun, SessionInfo } from "../types";

export function Sidebar({
  tabs,
  counts,
  activeTab,
  onSelectTab,
  sessions,
  activeSessionId,
  onSelectSession,
  onKillSession,
  runs,
  activeRunId,
  onSelectRun,
  onCancelRun,
  onOpenRules,
  rulesActive,
  open,
  onClose,
}: {
  tabs: Tab[];
  counts: Map<string, number>;
  activeTab: string | null;
  onSelectTab: (key: string) => void;
  sessions: SessionInfo[];
  activeSessionId: string | null;
  onSelectSession: (id: string) => void;
  onKillSession: (id: string) => void;
  runs: OngoingRun[];
  activeRunId: string | null;
  onSelectRun: (id: string) => void;
  onCancelRun: (id: string) => void;
  onOpenRules: () => void;
  rulesActive: boolean;
  open: boolean;
  onClose: () => void;
}) {
  return (
    <>
      {open && <div className="scrim" onClick={onClose} aria-hidden="true" />}
      <aside className={`sidebar${open ? " open" : ""}`}>
        <nav className="side-nav">
          <p className="side-label">Views</p>
          {tabs.map((t) => (
            <button
              key={t.key}
              type="button"
              className={`nav-tab${t.key === activeTab ? " active" : ""}`}
              onClick={() => onSelectTab(t.key)}
            >
              <span className="nav-name">{t.name}</span>
              {(counts.get(t.key) ?? 0) > 0 && (
                <span className="nav-count">{counts.get(t.key)}</span>
              )}
            </button>
          ))}

          {sessions.length > 0 && (
            <>
              <p className="side-label">Sessions</p>
              {sessions.map((s) => (
                <div
                  key={s.id}
                  className={`nav-tab session${
                    s.id === activeSessionId ? " active" : ""
                  }`}
                >
                  <button
                    type="button"
                    className="session-open"
                    onClick={() => onSelectSession(s.id)}
                    title={s.title}
                  >
                    <span
                      className={`session-dot${s.running ? " live" : " done"}`}
                      aria-hidden="true"
                    />
                    <span className="session-id">{s.id}</span>
                    <span className="session-title">{s.title}</span>
                  </button>
                  <button
                    type="button"
                    className="session-kill"
                    aria-label={`Close session ${s.id}`}
                    title="Kill session"
                    onClick={() => onKillSession(s.id)}
                  >
                    ×
                  </button>
                </div>
              ))}
            </>
          )}

          {runs.length > 0 && (
            <>
              <p className="side-label">AI runs</p>
              {runs.map((r) => (
                <div
                  key={r.id}
                  className={`nav-tab session${
                    r.id === activeRunId ? " active" : ""
                  }`}
                >
                  <button
                    type="button"
                    className="session-open"
                    onClick={() => onSelectRun(r.id)}
                    title={r.title}
                  >
                    <span
                      className="session-dot live"
                      aria-hidden="true"
                    />
                    <span className="session-id">{r.id}</span>
                    <span className="session-title">{r.title}</span>
                  </button>
                  <button
                    type="button"
                    className="session-kill"
                    aria-label={`Cancel run ${r.id}`}
                    title="Cancel run"
                    onClick={() => onCancelRun(r.id)}
                  >
                    ×
                  </button>
                </div>
              ))}
            </>
          )}
        </nav>

        <div className="side-footer">
          <button
            type="button"
            className={`nav-tab${rulesActive ? " active" : ""}`}
            onClick={onOpenRules}
          >
            <span className="nav-name">Settings</span>
          </button>
        </div>
      </aside>
    </>
  );
}
