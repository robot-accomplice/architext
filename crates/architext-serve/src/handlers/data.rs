//! Handler for `GET /data/{*path}`.
//!
//! Serves files from the data directory with `Cache-Control: no-store` and
//! correct content-type. Port of the JS handler in `createViewerRequestHandler`:
//!   `if (url.pathname.startsWith("/data/"))`
//!
//! Uses `safe_join` to guard against path traversal.

use axum::{
    extract::{Extension, Path},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

use crate::content_type::content_type_for_path;
use crate::safe_join::safe_join;
use crate::AppState;

/// GET /data/*path
pub async fn get_data_file(
    Path(sub_path): Path<String>,
    Extension(state): Extension<AppState>,
) -> Response {
    let Some(file_path) = safe_join(&state.data_dir, &sub_path) else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };

    // Check it's a regular file
    let meta = match tokio::fs::metadata(&file_path).await {
        Ok(m) if m.is_file() => m,
        _ => return (StatusCode::NOT_FOUND, "Not found").into_response(),
    };
    let _ = meta;

    let body = match tokio::fs::read(&file_path).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::NOT_FOUND, "Not found").into_response(),
    };

    let content_type = content_type_for_path(&file_path);
    let mut response = Response::new(axum::body::Body::from(body));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        "content-type",
        HeaderValue::from_static(content_type),
    );
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("no-store"),
    );
    response
}
