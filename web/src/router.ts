import { useEffect, useState } from "react";

// The URL encodes two orthogonal things: which list tab is showing (left/middle
// columns) and what's open in the detail column. Both survive a reload and are
// back/forward navigable. Shape: #/view/<tab>[/task/<id>[/edit|/run] | /new | /rules]
export type Pane =
  | { kind: "none" }
  | { kind: "detail"; id: string }
  | { kind: "edit"; id: string }
  | { kind: "run"; id: string }
  | { kind: "new" }
  | { kind: "rules" };

export interface Route {
  tab: string | null;
  pane: Pane;
}

export function parseHash(hash: string): Route {
  const h = hash.replace(/^#/, "");
  const view = h.match(/^\/view\/([^/]+)(\/.*)?$/);
  if (!view) return { tab: null, pane: { kind: "none" } };
  const tab = decodeURIComponent(view[1]);
  const rest = view[2] ?? "";

  if (rest === "" || rest === "/") return { tab, pane: { kind: "none" } };
  if (rest === "/new") return { tab, pane: { kind: "new" } };
  if (rest === "/rules") return { tab, pane: { kind: "rules" } };
  const run = rest.match(/^\/task\/([^/]+)\/run$/);
  if (run) return { tab, pane: { kind: "run", id: decodeURIComponent(run[1]) } };
  const edit = rest.match(/^\/task\/([^/]+)\/edit$/);
  if (edit)
    return { tab, pane: { kind: "edit", id: decodeURIComponent(edit[1]) } };
  const detail = rest.match(/^\/task\/([^/]+)$/);
  if (detail)
    return { tab, pane: { kind: "detail", id: decodeURIComponent(detail[1]) } };
  return { tab, pane: { kind: "none" } };
}

export function useRoute(): Route {
  const [route, setRoute] = useState<Route>(() => parseHash(location.hash));
  useEffect(() => {
    const onChange = () => setRoute(parseHash(location.hash));
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);
  return route;
}

export function navigate(path: string): void {
  location.hash = path;
}

const enc = encodeURIComponent;

export function tabHref(tab: string): string {
  return `#/view/${enc(tab)}`;
}
export function taskHref(tab: string, id: string): string {
  return `#/view/${enc(tab)}/task/${enc(id)}`;
}
export function editHref(tab: string, id: string): string {
  return `#/view/${enc(tab)}/task/${enc(id)}/edit`;
}
export function runHref(tab: string, id: string): string {
  return `#/view/${enc(tab)}/task/${enc(id)}/run`;
}
export function newHref(tab: string): string {
  return `#/view/${enc(tab)}/new`;
}
export function rulesHref(tab: string): string {
  return `#/view/${enc(tab)}/rules`;
}
