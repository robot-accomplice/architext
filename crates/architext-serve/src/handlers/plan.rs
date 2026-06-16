//! Handler for `GET /api/plan/{hash}`.
//!
//! Looks up the precomputed plan JSON in the native farm.
//!   Hit  → 200 `{"plan": <plan json>}` + Cache-Control: no-store
//!   Miss → 200 `{"miss": true}` + Cache-Control: no-store
//!
//! Port of the JS handler in `createViewerRequestHandler`:
//!   `if (url.pathname.startsWith("/api/plan/") && request.method === "GET")`

use axum::{
    extract::{Extension, Path},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

use crate::farm_state::{farm_lookup, Farm};

/// GET /api/plan/:hash
pub async fn get_plan(
    Path(hash): Path<String>,
    Extension(farm): Extension<Farm>,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        "cache-control",
        HeaderValue::from_static("no-store"),
    );
    headers.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );

    // Validate hash shape (64 lowercase hex chars), exactly as JS does.
    let hash_valid = hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());

    if !hash_valid {
        // Invalid hash shape → miss (no entry can match)
        return (
            StatusCode::OK,
            headers,
            r#"{"miss":true}"#.to_string(),
        )
            .into_response();
    }

    match farm_lookup(&farm, &hash) {
        Some(plan_json) => {
            // Hit: wrap in {"plan": <plan_json>} exactly as JS does:
            //   response.end(`{"plan":${stored}}`);
            let body = format!(r#"{{"plan":{plan_json}}}"#);
            (StatusCode::OK, headers, body).into_response()
        }
        None => {
            // Miss: 200 {"miss":true}
            (
                StatusCode::OK,
                headers,
                r#"{"miss":true}"#.to_string(),
            )
                .into_response()
        }
    }
}
