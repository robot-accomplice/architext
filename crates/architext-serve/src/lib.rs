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
//!   - POST /api/rules    (mutation: update/delete/move/move-before)
//!   - POST /api/notes    (mutation: update/delete + manifest bootstrap)
//!   - Unknown /api/* → 404
//!
//! Extension points for later slices: doctor, sync-repair,
//! release-plans, data-events (SSE).

pub mod content_type;
pub mod farm_state;
pub mod handlers;
pub mod safe_join;
pub mod security;
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
/// All routes go through the loopback-security middleware.
pub fn build_router(state: AppState, farm: Farm) -> Router {
    // Per-request loopback + mutation-token middleware — axum `middleware::from_fn_with_state`
    // can't capture non-Clone state cheaply, so we pass the token via Extension instead.
    let token_for_mw = state.mutation_token.clone();

    let security_layer = middleware::from_fn(move |req: Request, next: Next| {
        let token = token_for_mw.clone();
        async move { security_middleware(req, next, token).await }
    });

    Router::new()
        // API routes — GET
        .route("/api/session", get(handlers::session::get_session))
        .route("/api/plan/:hash", get(handlers::plan::get_plan))
        .route("/api/status", get(handlers::status::get_status))
        .route("/api/config", get(handlers::config_payload::get_config))
        .route("/api/repo-tree", get(handlers::repo_tree::get_repo_tree))
        // API routes — POST (mutations; guarded by security middleware)
        .route("/api/rules", post(handlers::rules::post_rules))
        .route("/api/notes", post(handlers::notes::post_notes))
        // Unknown /api/* fallback (must come after specific /api/* routes)
        .fallback(handlers::api_fallback::api_or_not_found_fallback)
        // Data files
        .route("/data/*path", get(handlers::data::get_data_file))
        // SPA static (root + wildcard fallback)
        .route("/", get(handlers::static_files::get_root))
        .route("/*path", get(handlers::static_files::get_asset))
        // Shared state
        .layer(Extension(farm))
        .layer(Extension(state))
        // Security middleware (runs first — outermost layer)
        .layer(security_layer)
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
    let (listener, bound_port) = bind_listener(host, port)?;

    tracing::info!("Building plan farm from {}", data_dir.display());
    let farm = farm_state::build_farm(&data_dir);

    // Derive target_dir from data_dir: data_dir is <target>/docs/architext/data
    // so target = data_dir.parent().parent().parent().
    let target_dir = data_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| data_dir.clone());

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

    let state = AppState {
        data_dir,
        dist_dir,
        mutation_token: Arc::new(mutation_token),
        target_dir,
        cli_version: Arc::new(env!("CARGO_PKG_VERSION").to_string()),
        schema_dir: resolved_schema_dir,
        write_lock: write_txn::new_write_lock(),
    };

    let router = build_router(state, farm);

    tracing::info!("Architext serve listening on http://{host}:{bound_port}");

    let listener = tokio::net::TcpListener::from_std(listener)?;
    axum::serve(listener, router).await
}
