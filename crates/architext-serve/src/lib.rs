//! architext-serve — Rust HTTP serve adapter for `architext serve`.
//!
//! Slice 1 + 2c + 3a of the serve-layer port:
//!   - Security middleware (loopback-only, mutation token)
//!   - Static serving (/data/*, /, /*path with SPA fallback)
//!   - GET /api/session
//!   - GET /api/plan/{hash} (native farm lookup)
//!   - GET /api/status    (collect_status → {ok, status})
//!   - GET /api/config    (diagram config payload + field spec)
//!   - GET /api/repo-tree (git ls-files or filesystem walk)
//!   - GET /api/file      (single-file contents, syntax-highlighted)
//!   - POST /api/rules        (mutation: update/delete/move/move-before)
//!   - POST /api/notes        (mutation: update/delete + manifest bootstrap)
//!   - POST /api/config       (mutation: write diagram config layer)
//!   - POST /api/release-plans (mutation: preview/approve/save-draft)
//!   - POST /api/doctor       (mutation: dry-run or apply doctor repairs)
//!   - POST /api/sync-repair  (mutation: non-interactive sync+validate)
//!   - GET /api/data-events (SSE live-reload: watch → validate → broadcast)
//!   - Unknown /api/* → 404

pub mod content_type;
pub mod farm_state;
pub mod handlers;
pub mod safe_join;
pub mod security;
pub mod watch_hub;
pub mod write_txn;

use std::net::{IpAddr, SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;

use write_txn::WriteLock;

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Router,
};
use serde_json::json;

use farm_state::Farm;
use security::{is_mutating_api_request, mutation_authorized, same_origin_loopback};
use watch_hub::WatchHub;

/// Shared application state threaded through every handler via `Extension`.
#[derive(Clone)]
pub struct AppState {
    pub data_dir: PathBuf,
    pub dist_dir: PathBuf,
    pub mutation_token: Arc<String>,
    /// The project root directory (parent of `docs/architext/data`).
    /// Derived from `data_dir` as `data_dir.parent().parent().parent()`.
    /// Used by `/api/status`, `/api/config`, and `/api/repo-tree`.
    pub target_dir: PathBuf,
    /// The CLI version string, embedded at compile time from the package metadata.
    /// Used by `/api/status` to populate `status.cliVersion`.
    pub cli_version: Arc<String>,
    /// Schema directory for the JSON schema validator (`viewer/schema/`).
    /// Used by the mutation handlers (rules/notes) to run `validate_data_dir`.
    pub schema_dir: PathBuf,
    /// Per-process write-lock: serialises concurrent mutation requests.
    pub write_lock: WriteLock,
}

/// Number of candidate ports to try when the preferred port is busy.
/// Port of `servePortSearchLimit = 50` in the JS source.
pub const SERVE_PORT_SEARCH_LIMIT: u16 = 50;

/// Default host: loopback only.
pub const DEFAULT_HOST: &str = "127.0.0.1";
/// Default port.
pub const DEFAULT_PORT: u16 = 4317;

/// Build the axum `Router`.
///
/// All routes go through the loopback-security middleware. `hub` is the live
/// data-watch hub feeding `GET /api/data-events`; `None` leaves that endpoint
/// well-formed but event-less (no data dir is being watched).
pub fn build_router(state: AppState, farm: Farm, hub: Option<WatchHub>) -> Router {
    // Per-request loopback + mutation-token middleware — axum `middleware::from_fn_with_state`
    // can't capture non-Clone state cheaply, so we pass the token via Extension instead.
    let token_for_mw = state.mutation_token.clone();

    let security_layer = middleware::from_fn(move |req: Request, next: Next| {
        let token = token_for_mw.clone();
        async move { security_middleware(req, next, token).await }
    });

    let router = Router::new()
        // API routes — GET
        .route("/api/session", get(handlers::session::get_session))
        .route("/api/plan/:hash", get(handlers::plan::get_plan))
        .route("/api/status", get(handlers::status::get_status))
        .route("/api/config", get(handlers::config_payload::get_config))
        .route("/api/repo-tree", get(handlers::repo_tree::get_repo_tree))
        .route("/api/node-git", get(handlers::node_git::get_node_git))
        .route("/api/file", get(handlers::file_preview::get_file_preview))
        // SSE live-reload stream (watch → validate → broadcast)
        .route("/api/data-events", get(handlers::data_events::get_data_events))
        // API routes — POST (mutations; guarded by security middleware)
        .route("/api/rules", post(handlers::rules::post_rules))
        .route("/api/notes", post(handlers::notes::post_notes))
        .route("/api/config", post(handlers::config_write::post_config))
        .route("/api/release-plans", post(handlers::release_plans::post_release_plans))
        .route("/api/doctor", post(handlers::doctor::post_doctor))
        .route("/api/sync-repair", post(handlers::sync_repair::post_sync_repair))
        // Unknown /api/* fallback (must come after specific /api/* routes)
        .fallback(handlers::api_fallback::api_or_not_found_fallback)
        // Data files
        .route("/data/*path", get(handlers::data::get_data_file))
        // SPA static (root + wildcard fallback)
        .route("/", get(handlers::static_files::get_root))
        .route("/*path", get(handlers::static_files::get_asset))
        // Shared state
        .layer(Extension(farm))
        .layer(Extension(state));

    // The data-watch hub is optional: only layer it when serving a watched data
    // dir. The handler reads `Option<Extension<WatchHub>>`, so an absent layer
    // degrades to an empty (but open) SSE stream.
    let router = match hub {
        Some(hub) => router.layer(Extension(hub)),
        None => router,
    };

    // Security middleware (runs first — outermost layer)
    router.layer(security_layer)
}

/// Security middleware:
///   1. Reject requests with non-loopback Host/Origin → 403.
///   2. Reject mutating requests lacking the token → 403.
async fn security_middleware(
    req: Request,
    next: Next,
    mutation_token: Arc<String>,
) -> Response {
    let headers = req.headers().clone();
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    // Check 1: loopback origin
    if !same_origin_loopback(&headers) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({
                "error": "Architext serve accepts requests only from its loopback origin."
            })),
        )
            .into_response();
    }

    // Check 2: mutation token
    if is_mutating_api_request(&path, &method)
        && !mutation_authorized(&headers, &mutation_token)
    {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({ "error": "Architext write request is not authorized." })),
        )
            .into_response();
    }

    next.run(req).await
}

/// Bind a TcpListener on `host:port`, trying up to `SERVE_PORT_SEARCH_LIMIT`
/// consecutive ports (matching JS `listenOnPort` + loop semantics).
///
/// Returns `(listener, bound_port)` or an error if no port was available.
pub fn bind_listener(host: &str, port: u16) -> std::io::Result<(TcpListener, u16)> {
    let host_addr: IpAddr = host
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{e}")))?;

    let last_port = if port == 0 {
        port
    } else {
        port.saturating_add(SERVE_PORT_SEARCH_LIMIT - 1)
    };

    for candidate in port..=last_port {
        let addr = SocketAddr::new(host_addr, candidate);
        match TcpListener::bind(addr) {
            Ok(listener) => {
                let bound = listener.local_addr()?.port();
                return Ok((listener, bound));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                if candidate == last_port {
                    return Err(e);
                }
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrInUse,
        format!("No available loopback port from {port} through {last_port}"),
    ))
}

/// Absolutize a possibly-relative path against `cwd`. A relative `data_dir`
/// (e.g. `docs/architext/data`) must become absolute BEFORE we derive the
/// target via `parent().parent().parent()`, or that chain collapses to empty.
fn absolutize(path: PathBuf, cwd: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

/// Derive the target repo root from an (absolute) data_dir laid out as
/// `<target>/docs/architext/data` → `data_dir.parent().parent().parent()`.
/// Falls back to `data_dir` itself if it has fewer than 3 ancestors.
fn target_dir_from(data_dir: &std::path::Path) -> PathBuf {
    data_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| data_dir.to_path_buf())
}

/// Top-level serve function: bind, build router, run.
pub async fn serve(
    data_dir: PathBuf,
    dist_dir: PathBuf,
    host: &str,
    port: u16,
    mutation_token: String,
) -> std::io::Result<()> {
    serve_with_schema_dir(data_dir, dist_dir, host, port, mutation_token, None).await
}

/// Top-level serve function with explicit schema_dir override (for tests).
///
/// If `schema_dir` is `None`, derives it as `<repo-root>/viewer/schema`
/// (same convention used by the validator in `architext-core`).
pub async fn serve_with_schema_dir(
    data_dir: PathBuf,
    dist_dir: PathBuf,
    host: &str,
    port: u16,
    mutation_token: String,
    schema_dir: Option<PathBuf>,
) -> std::io::Result<()> {
    // Absolutize a RELATIVE data_dir against the cwd first (see `absolutize`).
    let data_dir = absolutize(data_dir, std::env::current_dir()?);

    let (listener, bound_port) = bind_listener(host, port)?;

    // Warm the farm in the BACKGROUND so the server binds + listens immediately
    // (a synchronous build blocked startup for seconds on large repos). Lookups
    // miss until it's populated; the viewer falls back to in-process compute.
    tracing::info!("Warming plan farm in the background from {}", data_dir.display());
    let farm = farm_state::empty_farm();

    // Derive target_dir from data_dir: data_dir is <target>/docs/architext/data
    // so target = data_dir.parent().parent().parent().
    let target_dir = target_dir_from(&data_dir);

    // Derive schema_dir: <repo-root>/viewer/schema  (same as core tests).
    // The repo root is where the Cargo.toml workspace lives — i.e. the parent
    // of the `crates/` directory.  When launched from the binary, the data_dir
    // is docs/architext/data inside the repo so we can walk up; but the binary
    // also accepts arbitrary paths, so we use the compile-time manifest path
    // as the fallback anchor.  Tests pass an explicit override.
    let resolved_schema_dir = schema_dir.unwrap_or_else(|| {
        // Walk from data_dir up to the repo root heuristically.
        // Repo root has viewer/schema.  Try data_dir/../../../viewer/schema.
        let candidate = data_dir
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|root| root.join("viewer").join("schema"));
        candidate
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| {
                // Fallback: compile-time anchor (works in tests / cargo run)
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent().unwrap() // crates/
                    .parent().unwrap() // repo root
                    .join("viewer")
                    .join("schema")
            })
    });

    // Start the data-watch hub: it watches `{data_dir}/**/*.json`, debounces,
    // validates, and broadcasts to SSE clients of `/api/data-events`. A watcher
    // start failure is non-fatal — serve still runs, live-reload is just off.
    let hub = match watch_hub::start_watch_hub(data_dir.clone(), resolved_schema_dir.clone()) {
        Ok(hub) => Some(hub),
        Err(e) => {
            tracing::warn!("data-watch hub disabled (live-reload off): {e}");
            None
        }
    };

    // Clone handles for the background farm warm-up before `state`/`router` take
    // ownership of data_dir / farm.
    let warm_farm = farm.clone();
    let warm_data_dir = data_dir.clone();

    let state = AppState {
        data_dir,
        dist_dir,
        mutation_token: Arc::new(mutation_token),
        target_dir,
        cli_version: Arc::new(env!("CARGO_PKG_VERSION").to_string()),
        schema_dir: resolved_schema_dir,
        write_lock: write_txn::new_write_lock(),
    };

    let router = build_router(state, farm, hub);

    // Populate the farm off the async runtime (CPU-heavy synchronous work) —
    // fire-and-forget; `refresh_farm` swaps the populated map in atomically and
    // logs the plan count when done.
    tokio::task::spawn_blocking(move || farm_state::refresh_farm(&warm_farm, &warm_data_dir));

    tracing::info!("Architext serve listening on http://{host}:{bound_port}");

    // tokio requires the std listener to be non-blocking before `from_std`;
    // tokio 1.52+ panics on registering a blocking fd with the runtime. Mirrors
    // the same guard in the CLI's foreground serve path (serve/mod.rs).
    listener.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(listener)?;
    axum::serve(listener, router).await
}

#[cfg(test)]
mod target_dir_tests {
    use super::{absolutize, target_dir_from};
    use std::path::{Path, PathBuf};

    #[test]
    fn absolutize_joins_relative_to_cwd_and_leaves_absolute_untouched() {
        assert_eq!(
            absolutize(PathBuf::from("docs/architext/data"), PathBuf::from("/repo")),
            PathBuf::from("/repo/docs/architext/data")
        );
        assert_eq!(
            absolutize(PathBuf::from("/abs/docs/architext/data"), PathBuf::from("/repo")),
            PathBuf::from("/abs/docs/architext/data")
        );
    }

    #[test]
    fn target_dir_is_repo_root_for_absolute_data_dir() {
        assert_eq!(
            target_dir_from(Path::new("/repo/docs/architext/data")),
            PathBuf::from("/repo")
        );
    }

    #[test]
    fn regression_relative_data_dir_must_be_absolutized_first() {
        // The bug: parent().parent().parent() of a RELATIVE data_dir collapses,
        // so target_dir is wrong (→ /api/repo-tree walked nothing → 0 files).
        // Bare derivation on the relative path does NOT yield the repo root:
        assert_ne!(
            target_dir_from(Path::new("docs/architext/data")),
            PathBuf::from("/repo")
        );
        // Absolutizing first fixes it:
        let abs = absolutize(PathBuf::from("docs/architext/data"), PathBuf::from("/repo"));
        assert_eq!(target_dir_from(&abs), PathBuf::from("/repo"));
    }
}
