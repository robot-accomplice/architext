//! Handler for `GET /api/session`.
//!
//! Returns the per-process mutation token so the SPA can authorize writes.
//! Port of the JS handler in `createViewerRequestHandler`.

use axum::{Extension, Json};
use serde_json::{json, Value};

use crate::AppState;

/// GET /api/session → `{"mutationToken": "<base64url>"}`
pub async fn get_session(Extension(state): Extension<AppState>) -> Json<Value> {
    Json(json!({ "mutationToken": *state.mutation_token }))
}
