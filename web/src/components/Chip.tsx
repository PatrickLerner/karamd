import type { Priority, Status } from "../types";

const STATUS_CLASS: Record<Status, string> = {
  pending: "c-base1",
  "in-progress": "c-blue",
  "in-review": "c-violet",
  blocked: "c-orange",
  completed: "c-green",
  cancelled: "c-base1 strike",
};

const PRIORITY_CLASS: Record<Priority, string> = {
  low: "c-base1",
  medium: "c-base00",
  high: "c-orange",
  critical: "c-red",
};

// `pending` and `medium` are the taskmd defaults; showing them adds noise, so
// those chips render nothing unless a caller opts in with `always`.
export function StatusChip({
  status,
  always = false,
}: {
  status: Status;
  always?: boolean;
}) {
  if (status === "pending" && !always) return null;
  return <span className={`chip ${STATUS_CLASS[status]}`}>{status}</span>;
}

export function PriorityChip({
  priority,
  always = false,
}: {
  priority: Priority;
  always?: boolean;
}) {
  if (priority === "medium" && !always) return null;
  return <span className={`chip ${PRIORITY_CLASS[priority]}`}>{priority}</span>;
}

// `karamd run` execution-state chip (#044). Renders only when a task carries run
// state: actively running, a recorded failure, parked at max attempts, or any
// attempt count. Idle/never-run tasks show nothing so the list stays quiet.
// The attempt count (n/max) rides along whenever present, so a partially-failed
// or parked task shows how close it is to the cap at a glance.
export function RunChip({
  status,
  attempts,
  maxAttempts,
  parked,
}: {
  status: string | null;
  attempts: number | null;
  maxAttempts: number;
  parked: boolean;
}) {
  const running = status === "running";
  const failed = status === "failed";
  if (!running && !failed && !parked && attempts === null) return null;
  const word = running ? "running" : parked ? "parked" : failed ? "failed" : "run";
  const cls = running ? "c-blue" : parked ? "c-red" : "c-orange";
  const count = attempts === null ? "" : ` ${attempts}/${maxAttempts}`;
  const title = parked
    ? `AI execution parked after ${maxAttempts} attempts`
    : running
      ? "AI execution in progress"
      : failed
        ? "last AI execution failed"
        : "AI execution attempts";
  return (
    <span className={`chip run-chip ${cls}`} title={title}>
      {running && <span className="run-dot" aria-hidden="true" />}
      {`${word}${count}`}
    </span>
  );
}
