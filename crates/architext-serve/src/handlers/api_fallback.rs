//! Global router fallback — handles paths not matched by any explicit route.
//!
//! In practice this is only reached if `/{*path}` somehow doesn't match
//! (which shouldn't happen with axum's router). Kept as a defensive safety net.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub async fn api_or_not_found_fallback() -> Response {
    (StatusCode::NOT_FOUND, "Not found").into_response()
}
