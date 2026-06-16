//! architext-serve — Rust HTTP serve adapter for `architext serve`.
//!
//! Slice 1 of the serve-layer port:
//!   - Security middleware (loopback-only, mutation token)
//!   - Static serving (/data/*, /, /*path with SPA fallback)
//!   - GET /api/session
//!   - GET /api/plan/{hash} (native farm lookup)
//!   - Unknown /api/* → 404
//!
//! Extension points for later slices: config, status, doctor, sync-repair,
//! release-plans, rules, notes, data-events (SSE).

pub mod content_type;
pub mod farm_state;
pub mod handlers;
pub mod safe_join;
pub mod security;

use std::net::{IpAddr, SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
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
        // API routes
        .route("/api/session", get(handlers::session::get_session))
        .route(
            "/api/plan/:hash",
            get(handlers::plan::get_plan),
        )
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
    let (listener, bound_port) = bind_listener(host, port)?;

    tracing::info!("Building plan farm from {}", data_dir.display());
    let farm = farm_state::build_farm(&data_dir);

    let state = AppState {
        data_dir,
        dist_dir,
        mutation_token: Arc::new(mutation_token),
    };

    let router = build_router(state, farm);

    tracing::info!("Architext serve listening on http://{host}:{bound_port}");

    let listener = tokio::net::TcpListener::from_std(listener)?;
    axum::serve(listener, router).await
}
