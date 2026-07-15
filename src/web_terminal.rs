//! Embedded-terminal glue (#010, #021): spawn the configured run-command
//! (default `claude`) in a PTY rooted at the vault, keep it alive server-side as
//! a **session**, and let clients attach/detach over a WebSocket. One session
//! per task. A session outlives its socket: the child keeps running when the
//! browser tab closes, and dies only on an explicit `DELETE /api/sessions/{id}`
//! or when the server shuts down (registry `Drop`). Output is always drained
//! into a scrollback buffer so the child never blocks with no one attached.
//!
//! This module is the untestable seam — it spawns processes and bridges a
//! blocking PTY to async sockets, so it is **excluded from the coverage gate**
//! (see the `--ignore-filename-regex` in CI). All deterministic logic it relies
//! on (prompt seeding, argv parsing, the scrollback ring) lives in
//! [`crate::terminal`], which is fully covered. Keep this file thin: no business
//! logic that isn't also reachable (and tested) elsewhere.
//!
//! WebSocket protocol (matches the SPA's `Terminal` view):
//! - server -> client: binary frames = raw PTY output (a scrollback replay is
//!   sent first on attach); a text frame `{"type":"exit","code":N}` when the
//!   child exits (or immediately, if attaching to an already-exited session).
//! - client -> server: binary frames = stdin bytes; text frame
//!   `{"type":"resize","cols":C,"rows":R}` to resize the PTY.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::taskmd::Vault;
use crate::terminal::{Scrollback, launch_argv, parse_command, seed_prompt, with_seeded_prompt};
use crate::web::{ApiError, AppState};

/// Control messages the client may send as text frames.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientMsg {
    Resize { cols: u16, rows: u16 },
}

/// socket -> PTY events (stdin / resize).
enum ToPty {
    Data(Vec<u8>),
    Resize(u16, u16),
}

/// Broadcast events fanned out to every attached client of a session.
#[derive(Clone)]
enum TermEvent {
    Data(Vec<u8>),
    Exit(i32),
}

/// Whether a session's child is still running, or the code it exited with.
#[derive(Clone, Copy)]
enum SessionStatus {
    Running,
    Exited(i32),
}

const INITIAL_ROWS: u16 = 24;
const INITIAL_COLS: u16 = 80;
/// Retained output per session, replayed to a (re)attaching client.
const SCROLLBACK_BYTES: usize = 256 * 1024;
/// Live-stream fan-out buffer; a client lagging past this drops old chunks.
const BROADCAST_CAP: usize = 1024;

/// One live PTY session. Handles are shared: the reader/writer threads hold the
/// same `scrollback`/`events`/`status`, and every attached socket subscribes to
/// `events` and can push stdin via `in_tx`.
struct Session {
    title: String,
    /// The configured agent name this session was launched for (#051), or `None`
    /// for the legacy `--run-command` path. Exposed on [`SessionInfo`] so the
    /// sidebar can rebuild the `/run/<agent>` route and reattach to the *same*
    /// agent instead of the default.
    agent: Option<String>,
    /// The argv this session was spawned with; a new attach requesting a
    /// different tool (#047) relaunches rather than reattaching to the old one.
    argv: Vec<String>,
    scrollback: Arc<Mutex<Scrollback>>,
    events: broadcast::Sender<TermEvent>,
    in_tx: std::sync::mpsc::Sender<ToPty>,
    killer: Mutex<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
    status: Arc<Mutex<SessionStatus>>,
}

/// Everything needed to launch (or match) one terminal session: which agent, the
/// program argv (headless flags/placeholders already stripped), how to hand it
/// the prompt, and the seeded prompt itself. Bundled so `get_or_create` and
/// `spawn_session` take one descriptor instead of a long argument list, and the
/// reattach check compares against a single value.
struct LaunchSpec {
    agent: Option<String>,
    argv: Vec<String>,
    prompt_flag: Option<String>,
    prompt: String,
}

/// The set of live sessions, keyed by task id. Cloneable-shareable via `Arc` in
/// [`AppState`]. Dropping it (server shutdown) kills every child.
#[derive(Default)]
pub(crate) struct SessionRegistry {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl SessionRegistry {
    pub(crate) fn new() -> Self {
        SessionRegistry::default()
    }

    /// Attach to the task's session, spawning it if none exists yet. Returns the
    /// shared [`Session`]; the caller subscribes and replays scrollback.
    fn get_or_create(
        &self,
        id: &str,
        title: &str,
        root: PathBuf,
        spec: LaunchSpec,
    ) -> std::result::Result<Arc<Session>, String> {
        let mut map = self.sessions.lock().unwrap();
        if let Some(existing) = map.get(id) {
            // Same agent + tool: reattach to the live session (persistence, #021).
            // A different agent/tool was picked (#047/#051): kill the old one and
            // relaunch so the chosen agent actually starts. Comparing the agent
            // name too means a sidebar reattach that carries the agent lands on
            // the same session instead of the default relaunching over it (#051).
            // The agent name deterministically implies `prompt_flag`, so matching
            // agent + argv is sufficient identity.
            if existing.agent == spec.agent && existing.argv == spec.argv {
                return Ok(existing.clone());
            }
            let _ = existing.killer.lock().unwrap().kill();
            map.remove(id);
        }
        let session = spawn_session(title, root, spec)?;
        map.insert(id.to_string(), session.clone());
        Ok(session)
    }

    /// Kill and forget a session (explicit close). No-op for an unknown id.
    fn kill(&self, id: &str) -> bool {
        let removed = self.sessions.lock().unwrap().remove(id);
        if let Some(session) = removed {
            let _ = session.killer.lock().unwrap().kill();
            true
        } else {
            false
        }
    }

    fn list(&self) -> Vec<SessionInfo> {
        let map = self.sessions.lock().unwrap();
        let mut out: Vec<SessionInfo> = map
            .iter()
            .map(|(id, s)| {
                let (running, exit_code) = match *s.status.lock().unwrap() {
                    SessionStatus::Running => (true, None),
                    SessionStatus::Exited(code) => (false, Some(code)),
                };
                SessionInfo {
                    id: id.clone(),
                    title: s.title.clone(),
                    agent: s.agent.clone(),
                    running,
                    exit_code,
                }
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }
}

impl Drop for SessionRegistry {
    fn drop(&mut self) {
        if let Ok(map) = self.sessions.lock() {
            for session in map.values() {
                if let Ok(mut killer) = session.killer.lock() {
                    let _ = killer.kill();
                }
            }
        }
    }
}

/// Spawn the run-command in a PTY and wire up the reader/writer threads. The
/// reader always drains output into scrollback + the broadcast, so the child is
/// never blocked by a missing consumer.
fn spawn_session(
    title: &str,
    root: PathBuf,
    spec: LaunchSpec,
) -> std::result::Result<Arc<Session>, String> {
    let LaunchSpec {
        agent,
        argv,
        prompt_flag,
        prompt,
    } = spec;
    if argv.is_empty() {
        return Err("run-command is empty".to_string());
    }

    // Seed the task context as the final argument(s) so the session starts
    // working on it immediately: a bare positional for claude (`claude
    // "<prompt>"`), or behind the agent's `terminal_prompt_flag` for opencode
    // (`opencode --prompt "<prompt>"`), which treats a positional as a directory
    // (#051). exec-style args are not shell-parsed, so a multi-line prompt is safe.
    let launch = with_seeded_prompt(argv.clone(), &prompt, prompt_flag.as_deref());

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: INITIAL_ROWS,
            cols: INITIAL_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty failed: {e}"))?;

    let mut cmd = CommandBuilder::new(&launch[0]);
    for arg in &launch[1..] {
        cmd.arg(arg);
    }
    cmd.cwd(&root);

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn failed: {e}"))?;
    drop(pair.slave); // let the child see EOF once the master is gone
    let mut child = child;
    let killer = child.clone_killer();

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("pty reader failed: {e}"))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("pty writer failed: {e}"))?;
    let master = pair.master;

    let scrollback = Arc::new(Mutex::new(Scrollback::new(SCROLLBACK_BYTES)));
    let status = Arc::new(Mutex::new(SessionStatus::Running));
    let (events, _) = broadcast::channel::<TermEvent>(BROADCAST_CAP);
    let (in_tx, in_rx) = std::sync::mpsc::channel::<ToPty>();

    // Reader thread: pump output into scrollback + broadcast, then record exit.
    // Both the push and the broadcast happen under the scrollback lock so an
    // attaching client (which snapshots + subscribes under the same lock) never
    // loses or duplicates a chunk.
    {
        let scrollback = scrollback.clone();
        let status = status.clone();
        let events = events.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = buf[..n].to_vec();
                        let mut sb = scrollback.lock().unwrap();
                        sb.push(&chunk);
                        let _ = events.send(TermEvent::Data(chunk));
                    }
                }
            }
            let code = child
                .wait()
                .ok()
                .map(|s| s.exit_code() as i32)
                .unwrap_or(-1);
            let sb = scrollback.lock().unwrap();
            *status.lock().unwrap() = SessionStatus::Exited(code);
            let _ = events.send(TermEvent::Exit(code));
            drop(sb);
        });
    }

    // Writer thread: apply stdin / resize until every sender drops.
    std::thread::spawn(move || {
        while let Ok(msg) = in_rx.recv() {
            match msg {
                ToPty::Data(bytes) => {
                    if writer.write_all(&bytes).is_err() {
                        break;
                    }
                    let _ = writer.flush();
                }
                ToPty::Resize(cols, rows) => {
                    let _ = master.resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }
            }
        }
    });

    Ok(Arc::new(Session {
        title: title.to_string(),
        agent,
        argv,
        scrollback,
        events,
        in_tx,
        killer: Mutex::new(killer),
        status,
    }))
}

/// Query string for the run WebSocket: an optional configured agent name.
#[derive(Deserialize)]
pub(crate) struct RunParams {
    agent: Option<String>,
}

/// GET /api/tasks/{id}/run?agent=<name> (WebSocket). Resolves the task and the
/// launch argv first so a bad id or unknown agent is a normal HTTP error; then
/// upgrades and attaches to (or starts) its session. The chosen agent's command
/// (from `run.agents`, #047) drives the spawn; with no `agent` param it falls
/// back to the `--run-command` for back-compat.
pub(crate) async fn run_handler(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(params): Query<RunParams>,
    State(state): State<AppState>,
) -> std::result::Result<Response, ApiError> {
    let vault = Vault::open(&state.root)?;
    let task = vault.find(&id)?;
    let title = task.title().to_string();
    let prompt = seed_prompt(&task);
    let (argv, prompt_flag) =
        resolve_launch_argv(&vault, params.agent.as_deref(), &state.run_command)?;
    let spec = LaunchSpec {
        agent: params.agent.clone(),
        argv,
        prompt_flag,
        prompt,
    };
    let root = state.root.as_ref().clone();
    let sessions = state.sessions.clone();
    Ok(ws.on_upgrade(move |socket| async move {
        let mut socket = socket;
        match sessions.get_or_create(&id, &title, root, spec) {
            Ok(session) => attach(socket, session).await,
            Err(e) => close_with_error(&mut socket, &e).await,
        }
    }))
}

/// Resolve the terminal launch argv **plus how to seed the prompt**: a named
/// `run.agents` entry (with headless placeholders stripped, since the terminal is
/// interactive) when `agent` is given, else the plain `--run-command`. Returns
/// `(argv, prompt_flag)` where `prompt_flag` is the agent's
/// `terminal_prompt_flag` (opencode's `--prompt`, #051), or `None` for a bare
/// positional (claude and the legacy run-command path). An unknown agent name is
/// a client error, never an arbitrary command.
fn resolve_launch_argv(
    vault: &Vault,
    agent: Option<&str>,
    run_command: &str,
) -> std::result::Result<(Vec<String>, Option<String>), ApiError> {
    match agent {
        Some(name) => {
            let spec = vault.config.run.agents.get(name).ok_or_else(|| {
                ApiError::bad_request(format!(
                    "unknown agent `{name}` (not configured in run.agents)"
                ))
            })?;
            Ok((
                launch_argv(&spec.command),
                spec.terminal_prompt_flag.clone(),
            ))
        }
        None => Ok((parse_command(run_command), None)),
    }
}

/// Stream one attached socket: replay scrollback, then relay live events and
/// forward stdin/resize. Detaching (socket close) does **not** kill the child.
async fn attach(mut socket: WebSocket, session: Arc<Session>) {
    // Snapshot + subscribe under the scrollback lock (see the reader thread):
    // guarantees the replay and the live stream neither overlap nor gap.
    let (snapshot, mut rx, status) = {
        let sb = session.scrollback.lock().unwrap();
        let rx = session.events.subscribe();
        let snapshot = sb.snapshot();
        let status = *session.status.lock().unwrap();
        (snapshot, rx, status)
    };

    if !snapshot.is_empty() && socket.send(Message::Binary(snapshot.into())).await.is_err() {
        return;
    }
    // Already finished: tell the client now (the Exit event was broadcast before
    // we subscribed, so it won't arrive on `rx`).
    if let SessionStatus::Exited(code) = status {
        let _ = socket.send(Message::Text(exit_frame(code).into())).await;
    }

    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(TermEvent::Data(bytes)) => {
                    if socket.send(Message::Binary(bytes.into())).await.is_err() {
                        break;
                    }
                }
                Ok(TermEvent::Exit(code)) => {
                    let _ = socket.send(Message::Text(exit_frame(code).into())).await;
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Binary(bytes))) => {
                    let _ = session.in_tx.send(ToPty::Data(bytes.to_vec()));
                }
                Some(Ok(Message::Text(text))) => {
                    if let Ok(ClientMsg::Resize { cols, rows }) =
                        serde_json::from_str::<ClientMsg>(text.as_str())
                    {
                        let _ = session.in_tx.send(ToPty::Resize(cols, rows));
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                _ => {}
            },
        }
    }
    // Detach only: the session (and its child) live on for the next attach.
}

fn exit_frame(code: i32) -> String {
    format!("{{\"type\":\"exit\",\"code\":{code}}}")
}

/// One row in `GET /api/sessions`.
#[derive(Serialize)]
struct SessionInfo {
    id: String,
    title: String,
    /// The agent the session was launched for (#051), or `null` for the legacy
    /// `--run-command` path. The sidebar uses it to reattach to the same agent.
    agent: Option<String>,
    running: bool,
    exit_code: Option<i32>,
}

/// GET /api/sessions — the live/exited sessions, for the sidebar.
pub(crate) async fn list_sessions(State(state): State<AppState>) -> Response {
    axum::Json(state.sessions.list()).into_response()
}

/// DELETE /api/sessions/{id} — explicitly kill and forget a session.
pub(crate) async fn kill_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    state.sessions.kill(&id);
    StatusCode::NO_CONTENT.into_response()
}

/// Send a one-line error to the client and close (used when a session can't be
/// spawned at all).
async fn close_with_error(socket: &mut WebSocket, message: &str) {
    let _ = socket.send(Message::Text(message.to_string().into())).await;
    let _ = socket.send(Message::Close(None)).await;
}
