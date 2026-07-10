//! The `karamd web` backend: an axum JSON API over the taskmd library (#008),
//! plus static serving of the pre-built SPA bundle from `--web-dir`.
//!
//! The API is a thin shell — every read and write goes through [`crate::verbs`]
//! and the [`Vault`] store, so files stay taskmd-compatible and custom fields
//! (`recurring:` etc.) round-trip. No task logic is duplicated here.
//!
//! Access model (per #009): bind defaults to `127.0.0.1`; a remote host opts in
//! to its Tailscale IP / `0.0.0.0`. There is no app-level auth — the tailnet and
//! Tailscale ACLs are the security boundary, so a public interface is never the
//! default. The stack is async + WebSocket-capable (axum on tokio) so the
//! embedded-AI follow-up (#010) needs no re-platform.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderValue, StatusCode, header::CACHE_CONTROL},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use chrono::Local;
use serde::{Deserialize, Deserializer, Serialize};
use tower_http::services::{ServeDir, ServeFile};

use crate::next;
use crate::output::TaskView;
use crate::rule::{self, Rule};
use crate::taskmd::{Graph, Status, SystemEntropy, Task, Vault};
use crate::verbs;

/// Treat a stale `running` marker as not-running for display (#048): a dead run
/// (crash/reboot) that the read path self-corrects, so the UI never shows a
/// ghost run while waiting for the next `karamd run` to rewrite the file.
fn clear_stale_run(view: &mut TaskView, task: &Task, timeout_secs: u64) {
    if crate::run::is_running_stale(task, timeout_secs, chrono::Utc::now()) {
        view.ai_status = None;
        view.ai_run_started = None;
    }
}

/// Shared handler state: the vault root plus the command `run` sessions spawn
/// (#010). A fresh [`Vault`] is opened per request (cheap; re-reads config),
/// matching the store's defensive re-read design so a concurrent sync edit is
/// always seen. `pub(crate)` so [`crate::web_terminal`] can read it.
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) root: Arc<PathBuf>,
    pub(crate) run_command: Arc<String>,
    /// Live Claude sessions (#021), shared across requests so a run survives a
    /// detached socket and is killed only explicitly or on shutdown.
    pub(crate) sessions: Arc<crate::web_terminal::SessionRegistry>,
}

/// Error envelope: any handler failure becomes a non-2xx `{ "error": ... }`,
/// the shape the SPA's `api.ts` expects. `pub(crate)` for [`crate::web_terminal`].
pub(crate) struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    /// A 400 with an explicit message (used by [`crate::web_terminal`] for an
    /// unknown agent name).
    pub(crate) fn bad_request(message: impl Into<String>) -> ApiError {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        let message = e.to_string();
        // A genuinely absent task is 404; everything else (bad enum value,
        // dangling dependency, malformed config) is a client/config error.
        let status = if message.contains("no task with id") {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        };
        ApiError { status, message }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

/// Task summary as the SPA expects it: unlike [`TaskView`], optional fields
/// serialize as explicit `null` (never omitted) so the TypeScript shape holds.
#[derive(Serialize)]
struct SummaryOut {
    id: String,
    title: String,
    status: String,
    priority: String,
    effort: Option<String>,
    #[serde(rename = "type")]
    task_type: Option<String>,
    phase: Option<String>,
    due: Option<String>,
    tags: Vec<String>,
    dependencies: Vec<String>,
    group: Option<String>,
    owner: Option<String>,
    parent: Option<String>,
    created_at: Option<String>,
    completed_at: Option<String>,
    cancelled_at: Option<String>,
    recurring: Option<String>,
    // `karamd run` execution state (#044): explicit null when the task has no
    // such marker, matching the SPA's nullable TypeScript shape.
    ai_status: Option<String>,
    ai_attempts: Option<u32>,
    ai_run_started: Option<String>,
    ai_last_error: Option<String>,
    ready: bool,
    blockers: Vec<String>,
}

impl From<&TaskView> for SummaryOut {
    fn from(v: &TaskView) -> Self {
        SummaryOut {
            id: v.id.clone(),
            title: v.title.clone(),
            status: v.status.clone(),
            priority: v.priority.clone(),
            effort: v.effort.clone(),
            task_type: v.task_type.clone(),
            phase: v.phase.clone(),
            due: v.due.clone(),
            tags: v.tags.clone(),
            dependencies: v.dependencies.clone(),
            group: v.group.clone(),
            owner: v.owner.clone(),
            parent: v.parent.clone(),
            created_at: v.created_at.clone(),
            completed_at: v.completed_at.clone(),
            cancelled_at: v.cancelled_at.clone(),
            recurring: v.recurring.clone(),
            ai_status: v.ai_status.clone(),
            ai_attempts: v.ai_attempts,
            ai_run_started: v.ai_run_started.clone(),
            ai_last_error: v.ai_last_error.clone(),
            ready: v.ready,
            blockers: v.blockers.clone(),
        }
    }
}

/// Detail = summary + body (body is always a string, empty when the task has
/// none, matching the SPA's `TaskDetail`).
#[derive(Serialize)]
struct DetailOut {
    #[serde(flatten)]
    summary: SummaryOut,
    body: String,
}

impl From<&TaskView> for DetailOut {
    fn from(v: &TaskView) -> Self {
        DetailOut {
            summary: SummaryOut::from(v),
            body: v.body.clone().unwrap_or_default(),
        }
    }
}

#[derive(Serialize)]
struct InvalidOut {
    path: String,
    reason: String,
}

#[derive(Serialize)]
struct TasksResponse {
    tasks: Vec<SummaryOut>,
    invalid: Vec<InvalidOut>,
}

#[derive(Serialize)]
struct PhaseOut {
    id: Option<String>,
    name: String,
    description: Option<String>,
    due: Option<String>,
}

#[derive(Serialize)]
struct ConfigOut {
    phases: Vec<PhaseOut>,
    workflow: String,
    /// Phase ids the web "Today" tab merges, in render order (config-driven,
    /// with a default when `web.today` is unset).
    today: Vec<String>,
    /// Whether `run.enabled` is set (#043): the `ai-runnable` tag is inert
    /// unless this is true, so the web toggle annotates itself when it is off.
    run_enabled: bool,
    /// `run.max_attempts` (#044): the denominator for the "n/max attempts" the
    /// web shows on ai-runnable tasks, so a parked task is obvious at a glance.
    run_max_attempts: u32,
    /// Configured `run.agents` names (#047): the launchable tools the embedded
    /// terminal can start. Empty falls the frontend back to the default
    /// "Run with Claude" button (the `--run-command`).
    run_agents: Vec<String>,
    /// `run.agent` (#047): the default agent the terminal picker preselects.
    run_default_agent: String,
    /// karamd's own version (from `CARGO_PKG_VERSION`), shown in the web header.
    version: String,
}

#[derive(Serialize)]
struct NextItemOut {
    rank: usize,
    id: String,
    title: String,
    status: String,
    priority: String,
    score: i64,
    reasons: Vec<String>,
}

/// Deserialize a field that may be present-and-null (clear) vs absent (leave).
/// With `#[serde(default)]`, an absent key yields `None`; a present key yields
/// `Some(inner)` where `inner` is `None` for JSON `null`.
fn double_option<'de, D, T>(de: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(de)?))
}

#[derive(Deserialize)]
struct CreateBody {
    title: String,
    priority: Option<String>,
    effort: Option<String>,
    #[serde(rename = "type")]
    task_type: Option<String>,
    phase: Option<String>,
    due: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    body: Option<String>,
}

#[derive(Deserialize)]
struct PatchBody {
    title: Option<String>,
    priority: Option<String>,
    effort: Option<String>,
    #[serde(rename = "type")]
    task_type: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    phase: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    due: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    owner: Option<Option<String>>,
    tags: Option<Vec<String>>,
    dependencies: Option<Vec<String>>,
    body: Option<String>,
}

#[derive(Deserialize)]
struct StatusBody {
    status: String,
}

#[derive(Deserialize)]
struct NextParams {
    limit: Option<usize>,
}

/// GET /api/tasks — all tasks (summary) plus any broken task files.
async fn list_tasks(State(state): State<AppState>) -> std::result::Result<Response, ApiError> {
    let vault = Vault::open(&state.root)?;
    let scan = vault.scan()?;
    let graph = Graph::build(&scan.tasks);
    let timeout = vault.config.run.timeout_secs;
    let tasks = scan
        .tasks
        .iter()
        .map(|t| {
            let mut view = TaskView::build(t, &graph, false);
            clear_stale_run(&mut view, t, timeout);
            SummaryOut::from(&view)
        })
        .collect();
    let invalid = scan
        .invalid
        .iter()
        .map(|f| InvalidOut {
            path: f.rel_path.to_string_lossy().into_owned(),
            reason: f.reason.clone(),
        })
        .collect();
    Ok(Json(TasksResponse { tasks, invalid }).into_response())
}

/// GET /api/tasks/{id} — one task with its body.
async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let mut view = verbs::show(&state.root, &id)?;
    // Self-correct a ghost `running` marker for display (#048).
    let vault = Vault::open(&state.root)?;
    let task = vault.find(&id)?;
    clear_stale_run(&mut view, &task, vault.config.run.timeout_secs);
    Ok(Json(DetailOut::from(&view)).into_response())
}

/// POST /api/tasks — create a task.
async fn create_task(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> std::result::Result<Response, ApiError> {
    let spec = verbs::CreateSpec {
        title: body.title,
        priority: body.priority,
        effort: body.effort,
        task_type: body.task_type,
        phase: body.phase,
        due: body.due,
        tags: body.tags,
        dependencies: body.dependencies,
        template: None,
        body: body.body,
        force: false,
    };
    let view = verbs::create(
        &state.root,
        &spec,
        Local::now().date_naive(),
        &mut SystemEntropy::default(),
    )?;
    Ok((StatusCode::CREATED, Json(DetailOut::from(&view))).into_response())
}

/// PATCH /api/tasks/{id} — edit fields (not status).
async fn patch_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchBody>,
) -> std::result::Result<Response, ApiError> {
    let spec = verbs::EditSpec {
        title: body.title,
        priority: body.priority,
        effort: body.effort,
        task_type: body.task_type,
        phase: body.phase,
        due: body.due,
        owner: body.owner,
        tags: body.tags,
        dependencies: body.dependencies,
        body: body.body,
    };
    let view = verbs::edit(&state.root, &id, &spec)?;
    Ok(Json(DetailOut::from(&view)).into_response())
}

/// POST /api/tasks/{id}/status — set an explicit status (full enum).
async fn set_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<StatusBody>,
) -> std::result::Result<Response, ApiError> {
    let status = Status::parse(&body.status).with_context(|| {
        format!(
            "invalid status `{}` (pending, in-progress, in-review, completed, blocked, cancelled)",
            body.status
        )
    })?;
    verbs::set_status(&state.root, &id, status, Local::now().date_naive())?;
    // Re-read for the full detail (body included) the SPA renders after a change.
    let view = verbs::show(&state.root, &id)?;
    Ok(Json(DetailOut::from(&view)).into_response())
}

/// GET /api/config — phases (for grouping) and the completion workflow.
async fn get_config(State(state): State<AppState>) -> std::result::Result<Response, ApiError> {
    let vault = Vault::open(&state.root)?;
    let phases = vault
        .config
        .phases
        .iter()
        .map(|p| PhaseOut {
            id: p.id.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            due: p.due.clone(),
        })
        .collect();
    let workflow = match vault.config.workflow {
        crate::taskmd::Workflow::Solo => "solo",
        crate::taskmd::Workflow::PrReview => "pr-review",
    }
    .to_string();
    let today = vault.config.web.today_phases();
    Ok(Json(ConfigOut {
        phases,
        workflow,
        today,
        run_enabled: vault.config.run.enabled,
        run_max_attempts: vault.config.run.max_attempts,
        run_agents: vault.config.run.agents.keys().cloned().collect(),
        run_default_agent: vault.config.run.agent.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
    .into_response())
}

/// GET /api/next?limit=N — ranked recommendations (subset of the CLI shape).
async fn next_tasks(
    State(state): State<AppState>,
    Query(params): Query<NextParams>,
) -> std::result::Result<Response, ApiError> {
    let vault = Vault::open(&state.root)?;
    let scan = vault.scan()?;
    let phase_order: Vec<String> = vault
        .config
        .phases
        .iter()
        .map(|p| p.key().to_string())
        .collect();
    let opts = next::Options {
        limit: params.limit.unwrap_or(5),
        ..next::Options::default()
    };
    let report = next::recommend(&scan.tasks, &phase_order, &opts);
    let items: Vec<NextItemOut> = report
        .recommendations
        .iter()
        .map(|r| NextItemOut {
            rank: r.rank,
            id: r.id.clone(),
            title: r.title.clone(),
            status: r.status.clone(),
            priority: r.priority.clone(),
            score: r.score,
            reasons: r.reasons.clone(),
        })
        .collect();
    Ok(Json(items).into_response())
}

#[derive(Serialize)]
struct RulesResponse {
    /// Whether a rules file exists yet (an empty vault has none).
    exists: bool,
    rules: Vec<Rule>,
}

#[derive(Deserialize)]
struct RulesInput {
    rules: Vec<Rule>,
}

#[derive(Serialize)]
struct PreviewItem {
    filename: String,
    marker: String,
}

#[derive(Serialize)]
struct PreviewResponse {
    created: Vec<PreviewItem>,
}

/// GET /api/rules — the recurring rules currently on disk (empty if none).
async fn get_rules(State(state): State<AppState>) -> std::result::Result<Response, ApiError> {
    let path = state.root.join(crate::DEFAULT_CONFIG);
    let (exists, rules) = if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        (true, rule::load_rules(&raw)?)
    } else {
        (false, Vec::new())
    };
    Ok(Json(RulesResponse { exists, rules }).into_response())
}

/// PUT /api/rules — replace the whole rule set (covers add/edit/remove). The set
/// is validated before the atomic write, so the file never lands invalid.
async fn put_rules(
    State(state): State<AppState>,
    Json(body): Json<RulesInput>,
) -> std::result::Result<Response, ApiError> {
    rule::validate_all(&body.rules)?;
    let path = state.root.join(crate::DEFAULT_CONFIG);
    rule::write_rules(&path, &body.rules)?;
    Ok(Json(RulesResponse {
        exists: true,
        rules: body.rules,
    })
    .into_response())
}

/// POST /api/rules/preview — dry-run the proposed rules against the vault's
/// current task state; reports what a real run would create, writing nothing.
async fn preview_rules(
    State(state): State<AppState>,
    Json(body): Json<RulesInput>,
) -> std::result::Result<Response, ApiError> {
    let report =
        crate::generate_from_rules(&state.root, &body.rules, Local::now().date_naive(), true)?;
    let created = report
        .created
        .into_iter()
        .map(|c| PreviewItem {
            filename: c.filename,
            marker: c.marker,
        })
        .collect();
    Ok(Json(PreviewResponse { created }).into_response())
}

/// `Cache-Control` for immutable, content-addressed assets: their URL changes
/// whenever their bytes change, so a browser may keep them for a year.
const CACHE_IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// `Cache-Control` for everything else — chiefly `index.html` and the SPA
/// deep-link fallbacks. `no-store` (not `no-cache`) so the browser never keeps a
/// copy and never issues a conditional request: `no-cache` would still store and
/// revalidate via `Last-Modified`, but a Nix store path normalizes every mtime
/// to a constant `1970-01-01`, so revalidation returns `304` and serves a stale
/// index.html that points at asset URLs the new build no longer has (#037). The
/// entrypoint is tiny and refetching per navigation is negligible.
const CACHE_NO_STORE: &str = "no-store";

/// The `Cache-Control` value for a served static path. Content-hashed bundle
/// files (`…-<hash>.js` / `…-<hash>.css`) and the font files never change under
/// a given URL, so they are immutable; every other path revalidates.
fn cache_control_for(path: &str) -> &'static str {
    if path.starts_with("/fonts/") || is_hashed_asset(path) {
        CACHE_IMMUTABLE
    } else {
        CACHE_NO_STORE
    }
}

/// True when `path`'s final segment looks like a content-hashed bundle file,
/// i.e. `<name>-<hash>.js` or `<name>-<hash>.css` where `<hash>` is at least
/// six alphanumerics. Matches both bun's entry hash and the stylesheet hash the
/// web build appends. A plain `main.js` (no hash) is deliberately not matched,
/// so a build that ever forgets to hash never gets cached forever by mistake.
fn is_hashed_asset(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    let Some(stem) = file
        .strip_suffix(".js")
        .or_else(|| file.strip_suffix(".css"))
    else {
        return false;
    };
    match stem.rsplit_once('-') {
        Some((name, hash)) => {
            !name.is_empty() && hash.len() >= 6 && hash.bytes().all(|b| b.is_ascii_alphanumeric())
        }
        None => false,
    }
}

/// Middleware that stamps [`cache_control_for`] onto statically served assets.
/// Applied to the static file service only, so JSON responses and the
/// WebSocket upgrade are never touched.
async fn static_cache_control(req: Request, next: Next) -> Response {
    let cc = cache_control_for(req.uri().path());
    let mut resp = next.run(req).await;
    resp.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(cc));
    resp
}

/// Build the axum app: the JSON API plus static serving of the SPA bundle.
/// Unknown paths fall back to `index.html` so the SPA's client-side routing
/// works on deep links / refresh.
fn app(root: PathBuf, web_dir: PathBuf, run_command: String) -> Router {
    let index = web_dir.join("index.html");
    let static_service = ServeDir::new(web_dir).fallback(ServeFile::new(index));
    // Cache-Control is stamped on the static assets only (see
    // [`static_cache_control`]): a nested router carries the layer, so the API
    // routes and the WebSocket upgrade below are never wrapped.
    let static_router = Router::new()
        .fallback_service(static_service)
        .layer(middleware::from_fn(static_cache_control));
    Router::new()
        .route("/api/tasks", get(list_tasks).post(create_task))
        .route("/api/tasks/{id}", get(get_task).patch(patch_task))
        .route("/api/tasks/{id}/status", post(set_status))
        .route("/api/tasks/{id}/run", get(crate::web_terminal::run_handler))
        .route("/api/sessions", get(crate::web_terminal::list_sessions))
        .route(
            "/api/sessions/{id}",
            delete(crate::web_terminal::kill_session),
        )
        .route("/api/config", get(get_config))
        .route("/api/next", get(next_tasks))
        .route("/api/rules", get(get_rules).put(put_rules))
        .route("/api/rules/preview", post(preview_rules))
        .fallback_service(static_router)
        .with_state(AppState {
            root: Arc::new(root),
            run_command: Arc::new(run_command),
            sessions: Arc::new(crate::web_terminal::SessionRegistry::new()),
        })
}

/// If the SPA bundle is missing (no `index.html` under `web_dir`), return a
/// warning explaining why `/` will 404 and how to fix it. The API still works;
/// only the UI is absent. Returns `None` when the bundle is present.
fn spa_missing_hint(web_dir: &std::path::Path) -> Option<String> {
    if web_dir.join("index.html").exists() {
        return None;
    }
    Some(format!(
        "karamd web: no SPA bundle at {} (index.html missing). The JSON API is \
         up, but the UI will 404. Build it with `cd web && bun run build`, then \
         run with `--web-dir web/dist` (or set KARAMD_WEB_DIR).",
        web_dir.display()
    ))
}

/// A `()`-yielding shutdown trigger. Boxed (not `impl Future`) so [`run_server`]
/// and [`run_blocking`] have a single monomorphization — otherwise the ctrl-C
/// instantiation (which only ever early-returns on bind failure in tests) would
/// leave the success path uncovered for that copy.
type Shutdown = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

/// Bind and serve until `shutdown` resolves. Split out so tests drive it with an
/// immediate shutdown (deterministic, no signals).
async fn run_server(
    bind: SocketAddr,
    root: PathBuf,
    web_dir: PathBuf,
    run_command: String,
    shutdown: Shutdown,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    let addr = listener
        .local_addr()
        .with_context(|| "reading local addr")?;
    println!("karamd web: serving on http://{addr}");
    // A missing bundle is the common "it loads nothing" surprise: the API is up
    // but `/` 404s. Say so loudly with the fix, rather than leaving a silent 404.
    spa_missing_hint(&web_dir)
        .iter()
        .for_each(|hint| eprintln!("{hint}"));
    axum::serve(listener, app(root, web_dir, run_command))
        .with_graceful_shutdown(shutdown)
        .await
        .context("web server error")?;
    Ok(())
}

/// Synchronous CLI entry point: spin up a Tokio runtime, serve until Ctrl-C,
/// and report success once the server shuts down gracefully. `run_command` is
/// what a task's "run" session spawns (default `claude`).
pub fn serve_blocking(
    bind: SocketAddr,
    root: PathBuf,
    web_dir: PathBuf,
    run_command: String,
) -> Result<ExitCode> {
    run_blocking(
        bind,
        root,
        web_dir,
        run_command,
        Box::pin(shutdown_signal()),
    )
}

/// The runtime-owning core of [`serve_blocking`], split out so tests drive it
/// with an immediate shutdown (deterministic, no signals or servers-forever).
fn run_blocking(
    bind: SocketAddr,
    root: PathBuf,
    web_dir: PathBuf,
    run_command: String,
    shutdown: Shutdown,
) -> Result<ExitCode> {
    let rt = tokio::runtime::Runtime::new().context("starting the async runtime")?;
    rt.block_on(run_server(bind, root, web_dir, run_command, shutdown))?;
    Ok(ExitCode::SUCCESS)
}

/// Resolve when the process receives Ctrl-C. The await never completes under
/// test (no signal is sent), so the body is kept on one line: the covered await
/// expression and the function's implicit return share a source line, leaving
/// no uncoverable standalone brace.
#[rustfmt::skip]
async fn shutdown_signal() { let _ = tokio::signal::ctrl_c().await; }

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use serde_json::Value;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tower::ServiceExt; // oneshot

    fn tempdir() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = (std::process::id() as u64) << 20 | N.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("karamd-web-{uniq}"));
        fs::create_dir_all(base.join("tasks")).unwrap();
        base
    }

    fn write_task(root: &std::path::Path, rel: &str, content: &str) {
        fs::write(root.join("tasks").join(rel), content).unwrap();
    }

    /// Send a request through the app and return (status, parsed JSON body).
    async fn call(root: &std::path::Path, req: Request<Body>) -> (StatusCode, Value) {
        let router = app(
            root.to_path_buf(),
            root.join("web-dist"),
            "claude".to_string(),
        );
        let res = router.oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        // Non-JSON (e.g. an empty body) falls back to Null; every API endpoint
        // here returns JSON, so this only guards the helper.
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    fn get(path: &str) -> Request<Body> {
        Request::builder().uri(path).body(Body::empty()).unwrap()
    }

    fn json_req(method: &str, path: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn seed(root: &std::path::Path) {
        write_task(
            root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: First\nstatus: completed\npriority: high\nphase: v1\n---\n\n# First\n\ndone\n",
        );
        write_task(
            root,
            "002-b.md",
            "---\nid: \"002\"\ntitle: Second\nstatus: pending\ndependencies: [\"001\"]\n---\n\n# Second\n\nbody\n",
        );
    }

    #[tokio::test]
    async fn list_returns_tasks_and_invalid() {
        let root = tempdir();
        seed(&root);
        write_task(
            &root,
            "003-broken.md",
            "---\nid: \"003\"\nstatus: pending\n---\n",
        );
        let (status, body) = call(&root, get("/api/tasks")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["tasks"].as_array().unwrap().len(), 2);
        assert_eq!(body["invalid"].as_array().unwrap().len(), 1);
        // Optional absent fields are explicit null, not omitted.
        let second = &body["tasks"][1];
        assert_eq!(second["id"], "002");
        assert!(second["effort"].is_null());
        // 002 depends on the completed 001, so it is ready with no blockers.
        assert!(second["blockers"].as_array().unwrap().is_empty());
        assert_eq!(second["ready"], true);
    }

    #[tokio::test]
    async fn stale_running_marker_shows_as_not_running() {
        // A running marker far in the past is a dead run: the read path blanks
        // ai_status/ai_run_started for display (#048), while a fresh run stays.
        let root = tempdir();
        write_task(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: Ghost\nstatus: pending\ntags: [ai-runnable]\nai_status: running\nai_run_started: 2020-01-01T00:00:00Z\nai_attempts: 1\n---\n\n# Ghost\n",
        );
        // Default timeout (900s): the 2020 marker is stale.
        let (_, list) = call(&root, get("/api/tasks")).await;
        assert!(list["tasks"][0]["ai_status"].is_null());
        assert!(list["tasks"][0]["ai_run_started"].is_null());
        // Attempts are untouched (only the live-run markers are cleared).
        assert_eq!(list["tasks"][0]["ai_attempts"], 1);
        let (_, detail) = call(&root, get("/api/tasks/001")).await;
        assert!(detail["ai_status"].is_null());
    }

    #[tokio::test]
    async fn fresh_running_marker_is_preserved() {
        // A huge timeout means even an old marker is not yet stale, so a genuine
        // in-flight run still reads as running.
        let root = tempdir();
        fs::write(
            root.join(".taskmd.yaml"),
            "run:\n  enabled: true\n  timeout_secs: 100000000000\n",
        )
        .unwrap();
        write_task(
            &root,
            "001-a.md",
            "---\nid: \"001\"\ntitle: Live\nstatus: pending\ntags: [ai-runnable]\nai_status: running\nai_run_started: 2020-01-01T00:00:00Z\n---\n\n# Live\n",
        );
        let (_, list) = call(&root, get("/api/tasks")).await;
        assert_eq!(list["tasks"][0]["ai_status"], "running");
        let (_, detail) = call(&root, get("/api/tasks/001")).await;
        assert_eq!(detail["ai_status"], "running");
    }

    #[tokio::test]
    async fn get_one_returns_detail_with_body() {
        let root = tempdir();
        seed(&root);
        let (status, body) = call(&root, get("/api/tasks/001")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"], "001");
        assert!(body["body"].as_str().unwrap().contains("done"));
    }

    #[tokio::test]
    async fn get_missing_is_404() {
        let root = tempdir();
        let (status, body) = call(&root, get("/api/tasks/404")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body["error"].as_str().unwrap().contains("no task with id"));
    }

    #[tokio::test]
    async fn create_writes_a_task() {
        let root = tempdir();
        let req = json_req(
            "POST",
            "/api/tasks",
            serde_json::json!({
                "title": "New thing",
                "priority": "high",
                "type": "bug",
                "due": "2026-08-01",
                "tags": ["x"],
                "body": "the body"
            }),
        );
        let (status, body) = call(&root, req).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["id"], "001");
        assert_eq!(body["priority"], "high");
        assert_eq!(body["type"], "bug");
        assert_eq!(body["due"], "2026-08-01");
        assert_eq!(body["body"], "the body");
        assert!(root.join("tasks/001-new-thing.md").exists());
    }

    #[tokio::test]
    async fn create_bad_input_is_400() {
        let root = tempdir();
        let req = json_req("POST", "/api/tasks", serde_json::json!({ "title": "  " }));
        let (status, body) = call(&root, req).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("title"));
    }

    #[tokio::test]
    async fn patch_edits_fields_and_clears_phase() {
        let root = tempdir();
        seed(&root);
        let req = json_req(
            "PATCH",
            "/api/tasks/001",
            serde_json::json!({ "title": "Renamed", "phase": null, "owner": "me", "due": "2026-08-01" }),
        );
        let (status, body) = call(&root, req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["title"], "Renamed");
        assert!(body["phase"].is_null());
        assert_eq!(body["owner"], "me");
        assert_eq!(body["due"], "2026-08-01");

        // due can be cleared with an explicit null.
        let req = json_req(
            "PATCH",
            "/api/tasks/001",
            serde_json::json!({ "due": null }),
        );
        let (_, body) = call(&root, req).await;
        assert!(body["due"].is_null());
    }

    #[tokio::test]
    async fn patch_missing_is_404() {
        let root = tempdir();
        let req = json_req(
            "PATCH",
            "/api/tasks/404",
            serde_json::json!({ "title": "x" }),
        );
        let (status, _) = call(&root, req).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn set_status_transitions_and_returns_detail() {
        let root = tempdir();
        seed(&root);
        let req = json_req(
            "POST",
            "/api/tasks/002/status",
            serde_json::json!({ "status": "in-progress" }),
        );
        let (status, body) = call(&root, req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "in-progress");
        assert!(body["body"].as_str().unwrap().contains("body"));
    }

    #[tokio::test]
    async fn set_status_rejects_bad_value() {
        let root = tempdir();
        seed(&root);
        let req = json_req(
            "POST",
            "/api/tasks/002/status",
            serde_json::json!({ "status": "done" }),
        );
        let (status, body) = call(&root, req).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("invalid status"));
    }

    #[tokio::test]
    async fn config_reports_phases_and_workflow() {
        let root = tempdir();
        fs::write(
            root.join(".taskmd.yaml"),
            "workflow: pr-review\nrun:\n  enabled: true\n  max_attempts: 5\n  agent: opencode\n  agents:\n    claude:\n      command: [claude]\n    opencode:\n      command: [opencode, run]\nphases:\n  - id: v1\n    name: One\n  - name: Two\nweb:\n  today:\n    - v1\n",
        )
        .unwrap();
        let (status, body) = call(&root, get("/api/config")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["workflow"], "pr-review");
        assert_eq!(body["run_enabled"], true);
        assert_eq!(body["run_max_attempts"], 5);
        // Launchable agents (#047): names, sorted, plus the default selection.
        assert_eq!(body["run_agents"][0], "claude");
        assert_eq!(body["run_agents"][1], "opencode");
        assert_eq!(body["run_default_agent"], "opencode");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["phases"][0]["id"], "v1");
        assert_eq!(body["phases"][1]["name"], "Two");
        // Today grouping comes from `web.today`, verbatim.
        assert_eq!(body["today"][0], "v1");
        assert_eq!(body["today"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn config_defaults_when_no_file() {
        let root = tempdir();
        let (status, body) = call(&root, get("/api/config")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["workflow"], "solo");
        assert!(body["phases"].as_array().unwrap().is_empty());
        // No config: run defaults off, so the web toggle annotates itself.
        assert_eq!(body["run_enabled"], false);
        // No agents configured: the picker falls back to the default button.
        assert!(body["run_agents"].as_array().unwrap().is_empty());
        assert_eq!(body["run_default_agent"], "claude");
        // No config: Today falls back to the built-in default.
        assert_eq!(body["today"][0], "ongoing");
        assert_eq!(body["today"][1], "now");
    }

    #[tokio::test]
    async fn next_recommends_with_limit() {
        let root = tempdir();
        // A configured phase exercises the phase-order path.
        fs::write(
            root.join(".taskmd.yaml"),
            "phases:\n  - id: v1\n    name: One\n",
        )
        .unwrap();
        seed(&root);
        let (status, body) = call(&root, get("/api/next?limit=1")).await;
        assert_eq!(status, StatusCode::OK);
        let items = body.as_array().unwrap();
        assert_eq!(items.len(), 1);
        // 001 is completed; only 002 is actionable.
        assert_eq!(items[0]["id"], "002");
        assert!(items[0]["score"].as_i64().unwrap() > 0);
    }

    #[tokio::test]
    async fn next_defaults_limit_when_absent() {
        let root = tempdir();
        seed(&root);
        let (status, body) = call(&root, get("/api/next")).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.as_array().unwrap().len() <= 5);
    }

    #[tokio::test]
    async fn rules_empty_when_no_file() {
        let root = tempdir();
        let (status, body) = call(&root, get("/api/rules")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["exists"], false);
        assert!(body["rules"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn rules_put_then_get_round_trips() {
        let root = tempdir();
        let put = json_req(
            "PUT",
            "/api/rules",
            serde_json::json!({
                "rules": [
                    {"key": "checkin", "title": "Reach out", "trigger": "after_completion", "every_days": 18},
                    {"key": "bday", "title": "Birthday", "trigger": "calendar", "annual": "07-20", "lead_days": 10}
                ]
            }),
        );
        let (status, body) = call(&root, put).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["exists"], true);
        assert_eq!(body["rules"].as_array().unwrap().len(), 2);
        // Persisted and re-parseable.
        assert!(root.join(".taskmd.recurring.yaml").exists());
        let (_, got) = call(&root, get("/api/rules")).await;
        assert_eq!(got["rules"][0]["key"], "checkin");
        assert_eq!(got["rules"][1]["annual"], "07-20");
    }

    #[tokio::test]
    async fn rules_put_rejects_invalid() {
        let root = tempdir();
        // after_completion without every_days is invalid.
        let put = json_req(
            "PUT",
            "/api/rules",
            serde_json::json!({ "rules": [{"key": "k", "title": "t", "trigger": "after_completion"}] }),
        );
        let (status, body) = call(&root, put).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("every_days"));
        // Nothing was written.
        assert!(!root.join(".taskmd.recurring.yaml").exists());
    }

    #[tokio::test]
    async fn rules_get_surfaces_malformed_file() {
        let root = tempdir();
        fs::write(root.join(".taskmd.recurring.yaml"), "key: : :").unwrap();
        let (status, _) = call(&root, get("/api/rules")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rules_get_surfaces_unreadable_file() {
        // The rules path is a directory: it "exists" but reading it fails.
        let root = tempdir();
        fs::create_dir(root.join(".taskmd.recurring.yaml")).unwrap();
        let (status, _) = call(&root, get("/api/rules")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rules_preview_reports_due_without_writing() {
        let root = tempdir();
        // A never-run after_completion rule is due today.
        let preview = json_req(
            "POST",
            "/api/rules/preview",
            serde_json::json!({ "rules": [{"key": "checkin", "title": "Reach out", "trigger": "after_completion", "every_days": 18}] }),
        );
        let (status, body) = call(&root, preview).await;
        assert_eq!(status, StatusCode::OK);
        let created = body["created"].as_array().unwrap();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0]["marker"], "checkin");
        assert!(created[0]["filename"].as_str().unwrap().ends_with(".md"));
        // No task files were written.
        assert!(fs::read_dir(root.join("tasks")).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn rules_preview_rejects_invalid() {
        let root = tempdir();
        let preview = json_req(
            "POST",
            "/api/rules/preview",
            serde_json::json!({ "rules": [{"key": "k", "title": "t", "trigger": "calendar"}] }),
        );
        let (status, _) = call(&root, preview).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn malformed_config_surfaces_as_400() {
        let root = tempdir();
        fs::write(root.join(".taskmd.yaml"), "dir: [unclosed\n").unwrap();
        let (status, _) = call(&root, get("/api/tasks")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn static_bundle_is_served_with_spa_fallback() {
        let root = tempdir();
        let dist = root.join("web-dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("index.html"), "<!doctype html>hello spa").unwrap();
        let router = app(root.clone(), dist, "claude".to_string());
        // A deep link with no matching file falls back to index.html.
        let res = router.oneshot(get("/some/deep/link")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        // The fallback serves index.html, which must never be stored.
        assert_eq!(res.headers().get(CACHE_CONTROL).unwrap(), CACHE_NO_STORE);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("hello spa"));
    }

    #[test]
    fn api_error_bad_request_is_400() {
        // Used by web_terminal (excluded from coverage) for an unknown agent;
        // exercised here so the constructor stays covered.
        let e = ApiError::bad_request("nope");
        assert_eq!(e.status, StatusCode::BAD_REQUEST);
        assert_eq!(e.message, "nope");
    }

    #[test]
    fn cache_control_classifies_paths() {
        // Content-hashed bundles and fonts are immutable.
        assert_eq!(cache_control_for("/main-abcdef12.js"), CACHE_IMMUTABLE);
        assert_eq!(cache_control_for("/styles-deadbeef.css"), CACHE_IMMUTABLE);
        assert_eq!(
            cache_control_for("/fonts/iAWriterQuattroS-Regular.woff2"),
            CACHE_IMMUTABLE
        );
        // The no-store class must actually send `no-store`: the Nix constant-mtime
        // bug (#037) hinged on `no-cache` still storing and revalidating.
        assert_eq!(CACHE_NO_STORE, "no-store");
        // HTML, the root, unhashed assets, and the favicon are never stored.
        assert_eq!(cache_control_for("/index.html"), CACHE_NO_STORE);
        assert_eq!(cache_control_for("/"), CACHE_NO_STORE);
        assert_eq!(cache_control_for("/favicon.svg"), CACHE_NO_STORE);
        assert_eq!(cache_control_for("/main.js"), CACHE_NO_STORE);
        assert_eq!(cache_control_for("/styles.css"), CACHE_NO_STORE);
    }

    #[test]
    fn is_hashed_asset_matches_only_hashed_bundles() {
        assert!(is_hashed_asset("/main-abcdef12.js"));
        assert!(is_hashed_asset("/styles-deadbeef.css"));
        assert!(is_hashed_asset("/chunk-a1b2c3.js")); // 6-char hash is the floor
        assert!(is_hashed_asset("main-abcdef12.js")); // no leading slash still classifies
        assert!(!is_hashed_asset("/main.js")); // unhashed
        assert!(!is_hashed_asset("/main-1234.js")); // hash too short (<6)
        assert!(!is_hashed_asset("/-abcdef12.js")); // empty name
        assert!(!is_hashed_asset("/main-abcd_f12.js")); // non-alphanumeric hash
        assert!(!is_hashed_asset("/favicon.svg")); // not js/css
        assert!(!is_hashed_asset("/fonts/x.woff2")); // not js/css
    }

    #[tokio::test]
    async fn hashed_assets_are_immutable_and_html_is_no_store() {
        let root = tempdir();
        let dist = root.join("web-dist");
        fs::create_dir_all(dist.join("fonts")).unwrap();
        fs::write(dist.join("index.html"), "<!doctype html>hi").unwrap();
        fs::write(dist.join("main-abcdef12.js"), "1").unwrap();
        fs::write(dist.join("styles-deadbeef.css"), "body{}").unwrap();
        fs::write(dist.join("favicon.svg"), "<svg/>").unwrap();
        fs::write(dist.join("fonts").join("x.woff2"), "font").unwrap();

        async fn cc_of(root: &std::path::Path, dist: &std::path::Path, path: &str) -> String {
            let router = app(root.to_path_buf(), dist.to_path_buf(), "claude".to_string());
            let res = router.oneshot(get(path)).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{path} should be served");
            res.headers()
                .get(CACHE_CONTROL)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        }

        assert_eq!(
            cc_of(&root, &dist, "/main-abcdef12.js").await,
            CACHE_IMMUTABLE
        );
        assert_eq!(
            cc_of(&root, &dist, "/styles-deadbeef.css").await,
            CACHE_IMMUTABLE
        );
        assert_eq!(cc_of(&root, &dist, "/fonts/x.woff2").await, CACHE_IMMUTABLE);
        assert_eq!(cc_of(&root, &dist, "/index.html").await, CACHE_NO_STORE);
        assert_eq!(cc_of(&root, &dist, "/favicon.svg").await, CACHE_NO_STORE);
    }

    #[tokio::test]
    async fn run_server_binds_and_shuts_down() {
        let root = tempdir();
        // Ephemeral port + already-resolved shutdown: binds, serves, returns.
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        run_server(
            addr,
            root.clone(),
            root.join("web-dist"),
            "claude".to_string(),
            Box::pin(std::future::ready(())),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn run_server_reports_bind_failure() {
        let root = tempdir();
        // TEST-NET-1 (192.0.2.0/24) is never assigned to the host, so binding
        // fails with EADDRNOTAVAIL regardless of privileges.
        let addr: SocketAddr = "192.0.2.1:8787".parse().unwrap();
        let err = run_server(
            addr,
            root.clone(),
            root.join("web-dist"),
            "claude".to_string(),
            Box::pin(std::future::ready(())),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("binding"));
    }

    #[tokio::test]
    async fn shutdown_signal_pends_until_fired() {
        // `biased` polls the ctrl-c future first (covers its body, pends), then
        // the ready arm wins — deterministic, no real signal.
        tokio::select! {
            biased;
            () = shutdown_signal() => unreachable!(),
            () = async {} => {}
        }
    }

    #[test]
    fn spa_missing_hint_flags_absent_bundle() {
        let root = tempdir();
        let dist = root.join("web-dist");
        // No index.html yet: a helpful hint with the fix.
        let hint = spa_missing_hint(&dist).unwrap();
        assert!(hint.contains("no SPA bundle"));
        assert!(hint.contains("bun run build"));
        // Once the bundle exists, no hint.
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("index.html"), "<!doctype html>").unwrap();
        assert!(spa_missing_hint(&dist).is_none());
    }

    #[test]
    fn run_blocking_serves_and_returns_success() {
        // Owns its runtime, so it must not run inside one (plain #[test]).
        // An immediate shutdown makes the server stop before any connection.
        let root = tempdir();
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        run_blocking(
            addr,
            root.clone(),
            root.join("web-dist"),
            "claude".to_string(),
            Box::pin(std::future::ready(())),
        )
        .unwrap();
    }
}
