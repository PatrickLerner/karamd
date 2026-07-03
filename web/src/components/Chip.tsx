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
