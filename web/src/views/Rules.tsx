import { useEffect, useState } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { api, errorMessage } from "../api";
import { ErrorBanner } from "../components/Banner";
import {
  PRIORITIES,
  TRIGGERS,
  WEEKDAYS,
  type PreviewCreated,
  type Rule,
  type Trigger,
  type Week,
  type Weekday,
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

// The "every N periods" suffix, shown only when an interval > 1 is set.
function intervalNote(rule: Rule): string {
  return rule.interval && rule.interval > 1 ? `, every ${rule.interval}` : "";
}

// A short human summary of a rule's schedule, shown on the collapsed row.
function scheduleSummary(rule: Rule): string {
  switch (rule.trigger) {
    case "after_completion":
      return rule.every_days === undefined
        ? "every ? days"
        : `every ${rule.every_days} days`;
    case "calendar":
      return `on ${rule.annual ?? "MM-DD"}, ${rule.lead_days ?? 0}d lead`;
    case "monthly":
      return `day ${rule.day_of_month ?? "?"}, ${rule.lead_days ?? 0}d lead${intervalNote(rule)}`;
    case "weekly":
      return `${rule.day_of_week ?? "?"}, weekly${intervalNote(rule)}`;
    case "nth_weekday": {
      const w = rule.week === "last" ? "last" : `#${rule.week ?? "?"}`;
      return `${w} ${rule.day_of_week ?? "?"}, monthly${intervalNote(rule)}`;
    }
  }
}

// Parse the `week` select value back to a `Week` (number, "last", or unset).
function parseWeek(value: string): Week | undefined {
  if (value === "") return undefined;
  return value === "last" ? "last" : Number(value);
}

// Parse a numeric field back to a number, or drop it entirely when the input
// is blank so we never send `NaN` or `""` to the server.
function num(value: string): number | undefined {
  if (value.trim() === "") return undefined;
  const n = Number(value);
  return Number.isNaN(n) ? undefined : n;
}

// The editable fields for one rule (shown when its row is expanded).
function RuleFields({
  rule,
  onChange,
}: {
  rule: Rule;
  onChange: (next: Rule) => void;
}) {
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
    <div className="task-form rule-fields">
      <div className="form-row">
        <label>
          Key
          <input
            type="text"
            value={rule.key}
            onChange={(e) => set({ key: e.target.value })}
            placeholder="e.g. rotate-backups"
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
          placeholder="e.g. Rotate vault backups"
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
            placeholder="e.g. 30"
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
              placeholder="e.g. 12-24"
            />
          </label>
          <label>
            Lead days (before the date)
            <input
              type="number"
              min={0}
              value={rule.lead_days ?? ""}
              onChange={(e) => set({ lead_days: num(e.target.value) })}
              placeholder="e.g. 7"
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
              placeholder="e.g. 1"
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
              placeholder="e.g. 3"
            />
          </label>
        </div>
      )}

      {rule.trigger === "weekly" && (
        <label>
          Day of week
          <select
            value={rule.day_of_week ?? ""}
            onChange={(e) =>
              set({ day_of_week: (e.target.value || undefined) as Weekday | undefined })
            }
          >
            <option value="">unset</option>
            {WEEKDAYS.map((w) => (
              <option key={w} value={w}>
                {w}
              </option>
            ))}
          </select>
        </label>
      )}

      {rule.trigger === "nth_weekday" && (
        <div className="form-row">
          <label>
            Day of week
            <select
              value={rule.day_of_week ?? ""}
              onChange={(e) =>
                set({ day_of_week: (e.target.value || undefined) as Weekday | undefined })
              }
            >
              <option value="">unset</option>
              {WEEKDAYS.map((w) => (
                <option key={w} value={w}>
                  {w}
                </option>
              ))}
            </select>
          </label>
          <label>
            Week of month
            <select
              value={rule.week === undefined ? "" : String(rule.week)}
              onChange={(e) => set({ week: parseWeek(e.target.value) })}
            >
              <option value="">unset</option>
              <option value="1">1st</option>
              <option value="2">2nd</option>
              <option value="3">3rd</option>
              <option value="4">4th</option>
              <option value="last">last</option>
            </select>
          </label>
        </div>
      )}

      {(rule.trigger === "weekly" ||
        rule.trigger === "monthly" ||
        rule.trigger === "nth_weekday") && (
        <div className="form-row">
          <label>
            Interval (every N periods)
            <input
              type="number"
              min={1}
              value={rule.interval ?? ""}
              onChange={(e) => set({ interval: num(e.target.value) })}
              placeholder="1"
            />
          </label>
          <label>
            Anchor (YYYY-MM-DD, optional)
            <input
              type="text"
              value={rule.anchor ?? ""}
              onChange={(e) =>
                set({ anchor: e.target.value === "" ? undefined : e.target.value })
              }
              placeholder="e.g. 2026-07-10"
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
            placeholder="e.g. next"
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
          placeholder="e.g. ops, backups"
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
  );
}

// One rule as a compact, clickable row that expands to the editable fields.
function RuleItem({
  rule,
  open,
  onToggle,
  onChange,
  onRemove,
}: {
  rule: Rule;
  open: boolean;
  onToggle: () => void;
  onChange: (next: Rule) => void;
  onRemove: () => void;
}) {
  return (
    <section className={`rule-item${open ? " open" : ""}`}>
      <div className="rule-summary">
        <button
          type="button"
          className="rule-summary-btn"
          aria-expanded={open}
          onClick={onToggle}
        >
          <span className="rule-caret" aria-hidden="true">
            {open ? "▾" : "▸"}
          </span>
          <span className="chip c-blue">{rule.trigger}</span>
          <span className="rule-key">{rule.key || "(no key)"}</span>
          <span className="rule-summary-title">
            {rule.title || "untitled"}
          </span>
          <span className="chip c-base1 rule-sched">
            {scheduleSummary(rule)}
          </span>
        </button>
        <button
          type="button"
          className="rule-remove"
          onClick={onRemove}
          aria-label="Remove rule"
        >
          Remove
        </button>
      </div>
      {open && <RuleFields rule={rule} onChange={onChange} />}
    </section>
  );
}

function PreviewResult({ created }: { created: PreviewCreated[] }) {
  if (created.length === 0) {
    return <p className="muted rule-preview-empty">Nothing due today.</p>;
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
  const queryClient = useQueryClient();
  const rulesQ = useQuery({ queryKey: ["rules"], queryFn: () => api.rules() });

  // The rules are edited as a local draft, then PUT as a whole set on Save.
  const [draft, setDraft] = useState<Rule[] | null>(null);
  const [openIdx, setOpenIdx] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  // Seed the draft once the server responds.
  useEffect(() => {
    if (rulesQ.data) setDraft(rulesQ.data.rules);
  }, [rulesQ.data]);
  useEffect(() => {
    if (rulesQ.error) setError(errorMessage(rulesQ.error));
  }, [rulesQ.error]);

  const previewM = useMutation({
    mutationFn: (rules: Rule[]) => api.previewRules(rules),
    onError: (e: unknown) => setError(errorMessage(e)),
  });
  const saveM = useMutation({
    mutationFn: (rules: Rule[]) => api.putRules(rules),
    onSuccess: (res) => {
      queryClient.setQueryData(["rules"], res);
      setDraft(res.rules);
      setSaved(true);
      previewM.reset();
    },
    onError: (e: unknown) => setError(errorMessage(e)),
  });
  const busy = saveM.isPending || previewM.isPending;

  // Any edit invalidates the last save/preview result.
  function touch() {
    setSaved(false);
    setError(null);
    previewM.reset();
  }

  function update(index: number, next: Rule) {
    setDraft((prev) =>
      prev === null ? prev : prev.map((r, i) => (i === index ? next : r)),
    );
    touch();
  }

  function remove(index: number) {
    setDraft((prev) =>
      prev === null ? prev : prev.filter((_, i) => i !== index),
    );
    setOpenIdx((cur) =>
      cur === null ? cur : cur === index ? null : cur > index ? cur - 1 : cur,
    );
    touch();
  }

  function add() {
    setDraft((prev) => {
      const next = [...(prev ?? []), emptyRule()];
      setOpenIdx(next.length - 1); // open the new rule for editing
      return next;
    });
    touch();
  }

  const preview = previewM.data?.created ?? null;

  if (draft === null && error === null) {
    return (
      <div className="view">
        <p className="muted">Loading rules…</p>
      </div>
    );
  }

  const rules = draft ?? [];

  return (
    <div className="view">
      {error && <ErrorBanner message={error} onDismiss={() => setError(null)} />}
      <h1>Recurring rules</h1>
      {saved && <p className="rule-saved">Saved.</p>}

      {rules.length === 0 && (
        <p className="muted">No rules yet. Add one below.</p>
      )}

      <div className="rule-list">
        {rules.map((rule, i) => (
          <RuleItem
            key={i}
            rule={rule}
            open={openIdx === i}
            onToggle={() => setOpenIdx((cur) => (cur === i ? null : i))}
            onChange={(next) => update(i, next)}
            onRemove={() => remove(i)}
          />
        ))}
      </div>

      <div className="actions">
        <button type="button" onClick={add} disabled={busy}>
          + Add rule
        </button>
        <button
          type="button"
          onClick={() => previewM.mutate(rules)}
          disabled={busy}
        >
          Preview
        </button>
        <button
          type="button"
          className="rule-save"
          onClick={() => saveM.mutate(rules)}
          disabled={busy}
        >
          Save
        </button>
      </div>

      {preview !== null && <PreviewResult created={preview} />}
    </div>
  );
}
