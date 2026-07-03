import { useEffect, useState } from "react";
import { api, errorMessage } from "../api";
import { ErrorBanner } from "../components/Banner";
import {
  PRIORITIES,
  TRIGGERS,
  type PreviewCreated,
  type Rule,
  type Trigger,
} from "../types";

// A blank rule for the "Add rule" action. after_completion is the simplest
// trigger, so it is the default.
function emptyRule(): Rule {
  return { key: "", title: "", trigger: "after_completion" };
}

function splitList(input: string): string[] {
  return input
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s !== "");
}

// A short human summary of a rule's schedule, shown as a chip on the card.
function scheduleSummary(rule: Rule): string {
  switch (rule.trigger) {
    case "after_completion":
      return rule.every_days === undefined
        ? "every ? days"
        : `every ${rule.every_days} days`;
    case "calendar":
      return `on ${rule.annual ?? "MM-DD"}, ${rule.lead_days ?? 0}d lead`;
    case "monthly":
      return `day ${rule.day_of_month ?? "?"}, ${rule.lead_days ?? 0}d lead`;
  }
}

// Parse a numeric field back to a number, or drop it entirely when the input
// is blank so we never send `NaN` or `""` to the server.
function num(value: string): number | undefined {
  if (value.trim() === "") return undefined;
  const n = Number(value);
  return Number.isNaN(n) ? undefined : n;
}

function RuleCard({
  rule,
  onChange,
  onRemove,
}: {
  rule: Rule;
  onChange: (next: Rule) => void;
  onRemove: () => void;
}) {
  // Local text state for the tags input so the user can type commas freely;
  // it is committed to the rule on every change via splitList.
  const set = (patch: Partial<Rule>) => onChange({ ...rule, ...patch });

  function setTrigger(trigger: Trigger) {
    // Clear fields that do not belong to the new trigger, so we never PUT
    // stale schedule values the server would reject.
    onChange({
      key: rule.key,
      title: rule.title,
      trigger,
      phase: rule.phase,
      priority: rule.priority,
      tags: rule.tags,
      body: rule.body,
    });
  }

  return (
    <section className="rule-card">
      <div className="rule-card-head">
        <span className="chip c-blue">{rule.trigger}</span>
        <span className="chip c-base00">{rule.key || "(no key)"}</span>
        <span className="chip c-base1">{scheduleSummary(rule)}</span>
        <button
          type="button"
          className="rule-remove"
          onClick={onRemove}
          aria-label="Remove rule"
        >
          Remove
        </button>
      </div>
      <div className="task-form rule-fields">
        <div className="form-row">
          <label>
            Key
            <input
              type="text"
              value={rule.key}
              onChange={(e) => set({ key: e.target.value })}
              placeholder="rotate-backups"
            />
          </label>
          <label>
            Trigger
            <select
              value={rule.trigger}
              onChange={(e) => setTrigger(e.target.value as Trigger)}
            >
              {TRIGGERS.map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </select>
          </label>
        </div>
        <label>
          Title
          <input
            type="text"
            value={rule.title}
            onChange={(e) => set({ title: e.target.value })}
            placeholder="Rotate vault backups"
          />
        </label>

        {rule.trigger === "after_completion" && (
          <label>
            Every (days after last completion)
            <input
              type="number"
              min={1}
              value={rule.every_days ?? ""}
              onChange={(e) => set({ every_days: num(e.target.value) })}
              placeholder="30"
            />
          </label>
        )}

        {rule.trigger === "calendar" && (
          <div className="form-row">
            <label>
              Annual date (MM-DD)
              <input
                type="text"
                value={rule.annual ?? ""}
                onChange={(e) => set({ annual: e.target.value })}
                placeholder="12-24"
              />
            </label>
            <label>
              Lead days (before the date)
              <input
                type="number"
                min={0}
                value={rule.lead_days ?? ""}
                onChange={(e) => set({ lead_days: num(e.target.value) })}
                placeholder="7"
              />
            </label>
          </div>
        )}

        {rule.trigger === "monthly" && (
          <div className="form-row">
            <label>
              Day of month (1-31)
              <input
                type="number"
                min={1}
                max={31}
                value={rule.day_of_month ?? ""}
                onChange={(e) => set({ day_of_month: num(e.target.value) })}
                placeholder="1"
              />
            </label>
            <label>
              Lead days (0-27)
              <input
                type="number"
                min={0}
                max={27}
                value={rule.lead_days ?? ""}
                onChange={(e) => set({ lead_days: num(e.target.value) })}
                placeholder="3"
              />
            </label>
          </div>
        )}

        <div className="form-row">
          <label>
            Priority
            <select
              value={rule.priority ?? ""}
              onChange={(e) =>
                set({
                  priority: e.target.value === "" ? undefined : e.target.value,
                })
              }
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
            Phase
            <input
              type="text"
              value={rule.phase ?? ""}
              onChange={(e) =>
                set({
                  phase: e.target.value === "" ? undefined : e.target.value,
                })
              }
              placeholder="next"
            />
          </label>
        </div>

        <label>
          Tags (comma-separated)
          <input
            type="text"
            value={(rule.tags ?? []).join(", ")}
            onChange={(e) => {
              const list = splitList(e.target.value);
              set({ tags: list.length > 0 ? list : undefined });
            }}
            placeholder="ops, backups"
          />
        </label>

        <label>
          Body (optional; replaces the default TODO stub)
          <textarea
            value={rule.body ?? ""}
            onChange={(e) =>
              set({ body: e.target.value === "" ? undefined : e.target.value })
            }
            rows={4}
            spellCheck={false}
          />
        </label>
      </div>
    </section>
  );
}

function PreviewResult({ created }: { created: PreviewCreated[] }) {
  if (created.length === 0) {
    return (
      <p className="muted rule-preview-empty">Nothing due today.</p>
    );
  }
  return (
    <section className="rule-preview">
      <h2>Would create today</h2>
      {created.map((c) => (
        <div key={c.filename} className="rule-preview-item">
          <code>{c.filename}</code>
          <span className="chip c-base1">{c.marker}</span>
        </div>
      ))}
    </section>
  );
}

export function Rules() {
  const [rules, setRules] = useState<Rule[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  // null means "not previewed yet"; an array (possibly empty) is a result.
  const [preview, setPreview] = useState<PreviewCreated[] | null>(null);

  useEffect(() => {
    api
      .rules()
      .then((r) => setRules(r.rules))
      .catch((e: unknown) => {
        setError(errorMessage(e));
        setRules([]);
      });
  }, []);

  function update(index: number, next: Rule) {
    setRules((prev) =>
      prev === null ? prev : prev.map((r, i) => (i === index ? next : r)),
    );
    setSaved(false);
    setPreview(null);
  }

  function remove(index: number) {
    setRules((prev) =>
      prev === null ? prev : prev.filter((_, i) => i !== index),
    );
    setSaved(false);
    setPreview(null);
  }

  function add() {
    setRules((prev) => [...(prev ?? []), emptyRule()]);
    setSaved(false);
    setPreview(null);
  }

  async function onPreview() {
    if (rules === null) return;
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      const res = await api.previewRules(rules);
      setPreview(res.created);
    } catch (e: unknown) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function onSave() {
    if (rules === null) return;
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      const res = await api.putRules(rules);
      setRules(res.rules);
      setSaved(true);
    } catch (e: unknown) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  if (rules === null && error === null) {
    return (
      <div className="view">
        <p className="muted">Loading rules…</p>
      </div>
    );
  }

  return (
    <div className="view">
      {error && <ErrorBanner message={error} onDismiss={() => setError(null)} />}
      <h1>Recurring rules</h1>
      {saved && <p className="rule-saved">Saved.</p>}

      {rules !== null && rules.length === 0 && (
        <p className="muted">No rules yet. Add one below.</p>
      )}

      {(rules ?? []).map((rule, i) => (
        <RuleCard
          key={i}
          rule={rule}
          onChange={(next) => update(i, next)}
          onRemove={() => remove(i)}
        />
      ))}

      <div className="actions">
        <button type="button" onClick={add} disabled={busy}>
          + Add rule
        </button>
        <button
          type="button"
          onClick={() => void onPreview()}
          disabled={busy}
        >
          Preview
        </button>
        <button
          type="button"
          className="rule-save"
          onClick={() => void onSave()}
          disabled={busy}
        >
          Save
        </button>
      </div>

      {preview !== null && <PreviewResult created={preview} />}
    </div>
  );
}
