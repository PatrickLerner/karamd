import type { Config, Status, TaskSummary } from "./types";

// Navigation is by working view, not by state. "Today" is the default and
// merges the long-running background phase with this week's work (plus any
// unphased open task, so nothing gets lost). The remaining phases each get a
// tab, and every terminal task collects in "Done".
//
// Which phases the Today tab merges, and their render order, is config-driven:
// it comes from `config.today` (server-resolved from `web.today` in
// `.taskmd.yaml`, defaulting to the value below). Renaming a phase id only
// needs a config edit, never a code change.
export const DEFAULT_TODAY_PHASES = ["ongoing", "now"];
export const TODAY_TAB = "__today__";
export const DONE_TAB = "__done__";
export const DEFAULT_TAB = TODAY_TAB;

export interface Tab {
  key: string;
  name: string;
}

// URL slugs: the two sentinel tabs get readable slugs; phase tabs use their id.
export function tabSlug(key: string): string {
  if (key === TODAY_TAB) return "today";
  if (key === DONE_TAB) return "done";
  return key;
}

export function tabFromSlug(slug: string): string {
  if (slug === "today") return TODAY_TAB;
  if (slug === "done") return DONE_TAB;
  return slug;
}

export function isTerminal(status: Status): boolean {
  return status === "completed" || status === "cancelled";
}

export function buildTabs(config: Config, _tasks: TaskSummary[]): Tab[] {
  const today = new Set(config.today);
  const tabs: Tab[] = [{ key: TODAY_TAB, name: "Today" }];
  for (const p of config.phases) {
    if (p.id === null || today.has(p.id)) continue;
    tabs.push({ key: p.id, name: p.name });
  }
  tabs.push({ key: DONE_TAB, name: "Done" });
  return tabs;
}

export function taskInTab(
  t: TaskSummary,
  tabKey: string,
  today: Set<string>,
): boolean {
  if (tabKey === DONE_TAB) return isTerminal(t.status);
  if (isTerminal(t.status)) return false;
  if (tabKey === TODAY_TAB) return t.phase === null || today.has(t.phase);
  return t.phase === tabKey;
}
