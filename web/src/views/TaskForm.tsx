import { useEffect, useState, type FormEvent } from "react";
import { api, errorMessage } from "../api";
import { ErrorBanner } from "../components/Banner";
import { navigate, taskHref, tabHref } from "../router";
import {
  EFFORTS,
  PRIORITIES,
  TASK_TYPES,
  type Phase,
  type TaskCreate,
  type TaskPatch,
} from "../types";

function splitList(input: string): string[] {
  return input
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s !== "");
}

export function TaskForm({ id, tab }: { id?: string; tab: string }) {
  const editing = id !== undefined;
  const [phases, setPhases] = useState<Phase[]>([]);
  const [loading, setLoading] = useState(editing);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [title, setTitle] = useState("");
  const [priority, setPriority] = useState("");
  const [effort, setEffort] = useState("");
  const [type, setType] = useState("");
  const [phase, setPhase] = useState("");
  const [due, setDue] = useState("");
  const [tags, setTags] = useState("");
  const [dependencies, setDependencies] = useState("");
  const [body, setBody] = useState("");

  useEffect(() => {
    api
      .config()
      .then((c) => setPhases(c.phases))
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (id === undefined) return;
    api
      .task(id)
      .then((t) => {
        setTitle(t.title);
        setPriority(t.priority);
        setEffort(t.effort ?? "");
        setType(t.type ?? "");
        setPhase(t.phase ?? "");
        setDue(t.due ?? "");
        setTags(t.tags.join(", "));
        setDependencies(t.dependencies.join(", "));
        setBody(t.body);
        setLoading(false);
      })
      .catch((e: unknown) => {
        setError(errorMessage(e));
        setLoading(false);
      });
  }, [id]);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (title.trim() === "") {
      setError("Title is required");
      return;
    }
    setBusy(true);
    setError(null);
    const common = {
      title: title.trim(),
      ...(priority !== "" && { priority }),
      ...(effort !== "" && { effort }),
      ...(type !== "" && { type }),
      phase: phase === "" ? null : phase,
      due: due === "" ? null : due,
      tags: splitList(tags),
      dependencies: splitList(dependencies),
      body,
    };
    try {
      const task = editing
        ? await api.patchTask(id, common satisfies TaskPatch)
        : await api.createTask(common satisfies TaskCreate);
      navigate(taskHref(tab, task.id).replace(/^#/, ""));
    } catch (err: unknown) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  if (loading) {
    return (
      <div className="view">
        <p className="muted">Loading task…</p>
      </div>
    );
  }

  return (
    <div className="view">
      {error && <ErrorBanner message={error} onDismiss={() => setError(null)} />}
      <h1>{editing ? `Edit ${id}` : "New task"}</h1>
      <form className="task-form" onSubmit={(e) => void onSubmit(e)}>
        <label>
          Title
          <input
            type="text"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            required
            autoFocus
          />
        </label>
        <div className="form-row">
          <label>
            Priority
            <select
              value={priority}
              onChange={(e) => setPriority(e.target.value)}
            >
              <option value="">unset</option>
              {PRIORITIES.map((p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
            </select>
          </label>
          <label>
            Effort
            <select value={effort} onChange={(e) => setEffort(e.target.value)}>
              <option value="">unset</option>
              {EFFORTS.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="form-row">
          <label>
            Type
            <select value={type} onChange={(e) => setType(e.target.value)}>
              <option value="">unset</option>
              {TASK_TYPES.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </label>
          <label>
            Phase
            <select value={phase} onChange={(e) => setPhase(e.target.value)}>
              <option value="">no phase</option>
              {phases
                .filter((p) => p.id !== null)
                .map((p) => (
                  <option key={p.id} value={p.id ?? ""}>
                    {p.name}
                  </option>
                ))}
            </select>
          </label>
        </div>
        <label>
          Due date
          <input
            type="date"
            value={due}
            onChange={(e) => setDue(e.target.value)}
          />
        </label>
        <label>
          Tags (comma-separated)
          <input
            type="text"
            value={tags}
            onChange={(e) => setTags(e.target.value)}
            placeholder="cli, config"
          />
        </label>
        <label>
          Dependencies (comma-separated ids)
          <input
            type="text"
            value={dependencies}
            onChange={(e) => setDependencies(e.target.value)}
            placeholder="008, 011"
          />
        </label>
        <label>
          Body
          <textarea
            value={body}
            onChange={(e) => setBody(e.target.value)}
            rows={12}
            spellCheck={false}
          />
        </label>
        <div className="actions">
          <button type="submit" disabled={busy}>
            {editing ? "Save" : "Create"}
          </button>
          <a className="btn" href={editing ? taskHref(tab, id) : tabHref(tab)}>
            Cancel
          </a>
        </div>
      </form>
    </div>
  );
}
