import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm, type ITheme } from "@xterm/xterm";
import { taskHref } from "../router";

type ConnState = "connecting" | "connected" | "closed";

// Solarized Dark palette for xterm. The rest of the SPA is Solarized Light,
// but a terminal reads better dark, so this view flips the surface.
const solarizedDark: ITheme = {
  background: "#002b36", // base03
  foreground: "#839496", // base0
  cursor: "#93a1a1", // base1
  cursorAccent: "#002b36", // base03
  selectionBackground: "#073642", // base02
  black: "#073642",
  red: "#dc322f",
  green: "#859900",
  yellow: "#b58900",
  blue: "#268bd2",
  magenta: "#d33682",
  cyan: "#2aa198",
  white: "#eee8d5",
  brightBlack: "#002b36",
  brightRed: "#cb4b16",
  brightGreen: "#586e75",
  brightYellow: "#657b83",
  brightBlue: "#839496",
  brightMagenta: "#6c71c4",
  brightCyan: "#93a1a1",
  brightWhite: "#fdf6e3",
};

function socketUrl(id: string): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}/api/tasks/${encodeURIComponent(id)}/run`;
}

interface ExitFrame {
  type: "exit";
  code: number;
}

function parseExit(text: string): ExitFrame | null {
  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    return null;
  }
  if (
    typeof data === "object" &&
    data !== null &&
    "type" in data &&
    (data as { type: unknown }).type === "exit" &&
    "code" in data &&
    typeof (data as { code: unknown }).code === "number"
  ) {
    return { type: "exit", code: (data as { code: number }).code };
  }
  return null;
}

export function Terminal({ id, tab }: { id: string; tab: string }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<ConnState>("connecting");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new XTerm({
      convertEol: true,
      cursorBlink: true,
      fontFamily:
        "'Fira Code', ui-monospace, SFMono-Regular, Menlo, monospace",
      fontSize: 13,
      lineHeight: 1.2,
      theme: solarizedDark,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);

    const doFit = () => {
      try {
        fit.fit();
      } catch {
        // container not measurable yet; ignore
      }
    };
    doFit();

    // Refit once the webfont is ready: cell metrics change when Fira Code
    // swaps in, so rows/cols would otherwise be measured against the fallback.
    let cancelled = false;
    document.fonts.ready.then(() => {
      if (!cancelled) doFit();
    });

    let exited = false;
    let opened = false;
    const socket = new WebSocket(socketUrl(id));
    socket.binaryType = "arraybuffer";
    const encoder = new TextEncoder();

    const sendResize = () => {
      if (socket.readyState !== WebSocket.OPEN) return;
      socket.send(
        JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows }),
      );
    };

    socket.onopen = () => {
      opened = true;
      setState("connected");
      doFit();
      sendResize();
    };

    socket.onmessage = (ev: MessageEvent) => {
      if (typeof ev.data === "string") {
        const exit = parseExit(ev.data);
        if (exit) {
          exited = true;
          term.write(`\r\n\x1b[2mprocess exited (code ${exit.code})\x1b[0m\r\n`);
          socket.close();
        }
        return;
      }
      if (ev.data instanceof ArrayBuffer) {
        term.write(new Uint8Array(ev.data));
      }
    };

    socket.onerror = () => {
      if (!exited) setError("connection error");
    };

    socket.onclose = () => {
      setState("closed");
      if (!opened && !exited) {
        setError("could not connect to the run stream");
      }
    };

    const inputSub = term.onData((data: string) => {
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(encoder.encode(data));
      }
    });

    const onResize = () => {
      doFit();
      sendResize();
    };
    window.addEventListener("resize", onResize);

    return () => {
      cancelled = true;
      window.removeEventListener("resize", onResize);
      inputSub.dispose();
      socket.onopen = null;
      socket.onmessage = null;
      socket.onerror = null;
      socket.onclose = null;
      try {
        socket.close();
      } catch {
        // already closed
      }
      term.dispose();
    };
  }, [id]);

  const statusLabel: Record<ConnState, string> = {
    connecting: "connecting…",
    connected: "connected",
    closed: "closed",
  };

  return (
    <div className="view terminal-view">
      <div className="terminal-bar">
        <a href={taskHref(tab, id)}>← Back to task</a>
        <span className="muted terminal-status">
          {error ?? statusLabel[state]}
        </span>
      </div>
      <div className="terminal-host" ref={hostRef} />
    </div>
  );
}
