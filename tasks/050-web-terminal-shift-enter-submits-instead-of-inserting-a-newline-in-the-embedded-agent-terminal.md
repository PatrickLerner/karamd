---
id: '050'
title: 'web terminal: Shift+Enter submits instead of inserting a newline in the embedded agent terminal'
status: completed
created_at: 2026-07-15
priority: medium
type: bug
tags:
- web
- terminal
completed_at: 2026-07-15
---

## Description

In the embedded web terminal (`karamd web`, the Terminal view running an AI agent
over the PTY/WebSocket), pressing **Shift+Enter** submits the current line instead of
inserting a newline. Claude Code (and other REPL-style agents) can't compose a
multi-line prompt in the browser terminal, whereas Shift+Enter works fine in a native
terminal via `/terminal-setup`.

## Root cause

`web/src/views/Terminal.tsx` wires xterm's `term.onData` straight to the WebSocket:

```ts
const inputSub = term.onData((data: string) => {
  if (socket.readyState === WebSocket.OPEN) {
    socket.send(encoder.encode(data));
  }
});
```

xterm.js emits a bare `\r` for **both** Enter and Shift+Enter, so the PTY only ever
sees a carriage return, which Claude Code's REPL treats as "submit". There is no
key-event interception to distinguish the two.

## Fix

Intercept Enter with a
shift/alt modifier via `attachCustomKeyEventHandler` and send `ESC + CR` (`\x1b\r`),
the sequence iTerm2 sends for Option+Enter and that `/terminal-setup` maps Shift+Enter
to, which Claude Code interprets as "newline without submit":

```ts
// Register BEFORE the onData forwarder.
term.attachCustomKeyEventHandler((event) => {
  if (event.type === "keydown" && event.key === "Enter" && (event.shiftKey || event.altKey)) {
    event.preventDefault();
    if (socket.readyState === WebSocket.OPEN) {
      socket.send(encoder.encode("\x1b\r"));
    }
    return false;
  }
  return true;
});
```

Note vs. the orchestrator source: karamd sends **binary** frames
(`socket.send(encoder.encode(...))`), so the interceptor must encode `"\x1b\r"` the
same way rather than sending a raw string. Handle Alt+Enter too, for parity.

## Acceptance Criteria

- [ ] Shift+Enter in the embedded terminal inserts a newline in the agent's prompt
      instead of submitting.
- [ ] Alt/Option+Enter behaves the same.
- [ ] Plain Enter still submits (unchanged `\r`).
