//! Reusable mutation plumbing for the editing surfaces.
//!
//! The serve crate authorizes writes with a per-process token: every mutation
//! endpoint (`POST /api/rules`, and future config/notes/release endpoints)
//! requires the `x-architext-mutation-token` header, and the token itself is
//! fetched once from `GET /api/session` (`{ "mutationToken": "<base64url>" }`).
//!
//! This module is the single home for that contract so every future editor
//! reuses it:
//!   - [`fetch_mutation_token`] — `GET /api/session` once at startup.
//!   - [`post_mutation`] — POST a JSON body to a mutation endpoint with the
//!     token header, then unwrap the server envelope.
//!
//! Envelope handling is faithful to the serve handlers: a success body is the
//! action result (e.g. `{ rules: [...], validation: { ok: true } }`); a failure
//! body is `{ ok: false, mode, error, reload }` returned with HTTP 200 (the JS
//! and Rust handlers both reply 200 on a rolled-back write). So a write is an
//! error when the transport fails, the status is non-2xx, OR the body carries
//! `ok: false` — never silently swallowed.

use gloo_net::http::Request;
use serde_json::Value;

/// The mutation-token header the serve security layer checks
/// (`crates/architext-serve/src/security.rs`).
const MUTATION_TOKEN_HEADER: &str = "x-architext-mutation-token";

/// A failed mutation, carrying the server's own message when there is one.
#[derive(Debug, Clone)]
pub struct MutationError {
    pub message: String,
}

impl std::fmt::Display for MutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl MutationError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

/// Fetch the per-process mutation token from `GET /api/session`.
///
/// Best-effort: a missing/failed session (older server, transport error) yields
/// `None`, in which case the editing affordances stay disabled rather than
/// posting an unauthorized write that the server would reject.
pub async fn fetch_mutation_token() -> Option<String> {
    let resp = Request::get("/api/session").send().await.ok()?;
    if !resp.ok() {
        return None;
    }
    let body: Value = resp.json().await.ok()?;
    body.get("mutationToken").and_then(Value::as_str).map(str::to_string)
}

/// POST a JSON `body` to a mutation endpoint with the mutation-token header,
/// then unwrap the server envelope into the success value.
///
/// Treated as an error (never a silent success):
///   - no token (the session fetch failed → not authorized to write);
///   - a transport failure;
///   - a non-2xx HTTP status;
///   - a body that carries `ok: false` (a rejected/rolled-back write — the
///     server's `error` message is surfaced verbatim).
pub async fn post_mutation(
    token: Option<&str>,
    path: &str,
    body: &Value,
) -> Result<Value, MutationError> {
    let token = token.ok_or_else(|| {
        MutationError::new("No mutation token — editing is not authorized in this session.")
    })?;

    let resp = Request::post(path)
        .header("content-type", "application/json")
        .header(MUTATION_TOKEN_HEADER, token)
        .json(body)
        .map_err(|e| MutationError::new(format!("Could not build request: {e}")))?
        .send()
        .await
        .map_err(|e| MutationError::new(format!("Request failed: {e}")))?;

    let status = resp.status();
    let value: Value = resp
        .json()
        .await
        .map_err(|e| MutationError::new(format!("Invalid response JSON: {e}")))?;

    // A non-2xx status is an error regardless of body shape.
    if !(200..300).contains(&status) {
        return Err(MutationError::new(envelope_error(&value).unwrap_or_else(|| {
            format!("Mutation failed (HTTP {status}).")
        })));
    }

    // HTTP 200 but `{ ok: false }` is the rolled-back-write path — fail loud.
    if let Some(message) = envelope_error(&value) {
        return Err(MutationError::new(message));
    }

    Ok(value)
}

/// Pull the error message out of an `{ ok: false, error }` envelope, or `None`
/// when the body is not an error envelope.
fn envelope_error(value: &Value) -> Option<String> {
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        let message = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("The write was rejected.");
        Some(message.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_error_extracts_server_message() {
        let body = json!({ "ok": false, "mode": "rules", "error": "Rule \"x\" is edit protected", "reload": false });
        assert_eq!(envelope_error(&body).as_deref(), Some("Rule \"x\" is edit protected"));
    }

    #[test]
    fn envelope_error_defaults_when_message_absent() {
        let body = json!({ "ok": false });
        assert_eq!(envelope_error(&body).as_deref(), Some("The write was rejected."));
    }

    #[test]
    fn envelope_error_none_for_success_bodies() {
        // The rules success body has no `ok` flag — it is the action result.
        let success = json!({ "rules": [], "validation": { "ok": true } });
        assert_eq!(envelope_error(&success), None);
        // An explicit ok:true is also not an error.
        assert_eq!(envelope_error(&json!({ "ok": true })), None);
    }
}
