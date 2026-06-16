//! Security utilities: loopback-only origin check, mutation-token guard.
//!
//! Port of the JS helpers in `src/adapters/cli/architext-cli.mjs`:
//!   `isLoopbackHost`, `sameOriginLoopbackRequest`, `mutationAuthorized`.

use axum::http::{HeaderMap, StatusCode};
use serde_json::json;

/// Port of JS `isLoopbackHost(host)`.
///
/// Accepts "localhost", "::1", and any IPv4 address in the 127.0.0.0/8 range.
/// The `host` argument must already be lowercased and have IPv6 brackets stripped.
pub fn is_loopback_host(host: &str) -> bool {
    let normalized = host.to_lowercase();
    // Strip IPv6 brackets if present: "[::1]" → "::1"
    let normalized = normalized.trim_start_matches('[').trim_end_matches(']');
    if normalized == "localhost" || normalized == "::1" {
        return true;
    }
    // IPv4 only: must be a valid IPv4 starting with "127."
    // Quick check: if it parses as IPv4 and starts with 127.
    if normalized.starts_with("127.") {
        // Validate it is a well-formed IPv4 address
        let parts: Vec<&str> = normalized.split('.').collect();
        if parts.len() == 4 {
            return parts.iter().all(|p| p.parse::<u8>().is_ok());
        }
    }
    false
}

/// Parse host + optional port from a `Host` header value.
/// Returns `(host_lowercase, port_str)` or `None` on parse failure.
pub fn parse_host_header(header: &str) -> Option<(String, String)> {
    // Try parsing as "http://<header>" to let the URL parser handle it.
    let url_str = format!("http://{header}");
    let Ok(parsed) = url::Url::parse(&url_str) else {
        return None;
    };
    let host = parsed.host_str()?.to_lowercase();
    // Url::port() returns None for default ports — preserve the raw port string
    // so same-origin port comparison works (both empty when omitted).
    let port = parsed.port().map(|p| p.to_string()).unwrap_or_default();
    Some((host, port))
}

/// Port of JS `sameOriginLoopbackRequest(request)`.
///
/// Returns true iff:
///   1. The `Host` header is present and resolves to a loopback address.
///   2. If `Origin` is present: it parses, its host is loopback, it matches
///      the `Host` host, and the ports match.
///   3. If `Origin` is absent: accepted (same-tab / non-browser requests).
pub fn same_origin_loopback(headers: &HeaderMap) -> bool {
    // Step 1: Host header must be present and loopback.
    let host_val = match headers.get("host").and_then(|v| v.to_str().ok()) {
        Some(h) => h,
        None => return false,
    };
    let Some((request_host, request_port)) = parse_host_header(host_val) else {
        return false;
    };
    if !is_loopback_host(&request_host) {
        return false;
    }

    // Step 2: If Origin is present, it must match.
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let Some(origin) = origin else {
        // No Origin header — accepted.
        return true;
    };
    let Ok(origin_url) = url::Url::parse(origin) else {
        return false;
    };
    let origin_host = match origin_url.host_str() {
        Some(h) => h.to_lowercase(),
        None => return false,
    };
    let origin_port = origin_url.port().map(|p| p.to_string()).unwrap_or_default();
    is_loopback_host(&origin_host)
        && origin_host == request_host
        && origin_port == request_port
}

/// JS error JSON for a loopback rejection.
pub fn loopback_error_body() -> serde_json::Value {
    json!({ "error": "Architext serve accepts requests only from its loopback origin." })
}

/// Generate a random base64url-encoded 32-byte token (port of JS `randomBytes(32).toString("base64url")`).
pub fn generate_mutation_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64url_encode(&bytes)
}

/// Base64url encode without padding (matches Node's `base64url` encoding).
fn base64url_encode(bytes: &[u8]) -> String {
    // Standard base64 with + → - and / → _ and no padding.
    let b64 = base64_encode(bytes);
    b64.replace('+', "-").replace('/', "_").replace('=', "")
}

/// Simple base64 standard encoding.
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        out.push(TABLE[b0 >> 2] as char);
        out.push(TABLE[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((b1 & 0xf) << 2) | (b2 >> 6)] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[b2 & 0x3f] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Check whether the `x-architext-mutation-token` header matches the expected token.
pub fn mutation_authorized(headers: &HeaderMap, mutation_token: &str) -> bool {
    headers
        .get("x-architext-mutation-token")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == mutation_token)
        .unwrap_or(false)
}

/// Check whether this is a mutating API request (POST to a known mutation path).
pub fn is_mutating_api_request(path: &str, method: &axum::http::Method) -> bool {
    if method != axum::http::Method::POST {
        return false;
    }
    matches!(
        path,
        "/api/doctor"
            | "/api/sync-repair"
            | "/api/release-plans"
            | "/api/rules"
            | "/api/notes"
            | "/api/config"
    )
}

/// Produce the standard error response tuple for loopback rejection.
pub fn reject_non_loopback() -> (StatusCode, axum::Json<serde_json::Value>) {
    (StatusCode::FORBIDDEN, axum::Json(loopback_error_body()))
}

/// Produce the standard error response tuple for mutation token rejection.
pub fn reject_unauthorized_mutation() -> (StatusCode, axum::Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        axum::Json(json!({ "error": "Architext write request is not authorized." })),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    fn headers_with(host: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("host", host.parse().unwrap());
        h
    }

    fn headers_with_origin(host: &str, origin: &str) -> HeaderMap {
        let mut h = headers_with(host);
        h.insert("origin", origin.parse().unwrap());
        h
    }

    // --- is_loopback_host ---

    #[test]
    fn localhost_is_loopback() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
    }

    #[test]
    fn ipv6_loopback_is_loopback() {
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("[::1]"));
    }

    #[test]
    fn ipv4_127_is_loopback() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.0.0.2"));
        assert!(is_loopback_host("127.255.255.255"));
    }

    #[test]
    fn non_loopback_ips_rejected() {
        assert!(!is_loopback_host("192.168.1.1"));
        assert!(!is_loopback_host("10.0.0.1"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("example.com"));
    }

    // --- same_origin_loopback ---

    #[test]
    fn loopback_host_no_origin_accepted() {
        assert!(same_origin_loopback(&headers_with("127.0.0.1:4317")));
        assert!(same_origin_loopback(&headers_with("localhost:4317")));
    }

    #[test]
    fn external_host_rejected() {
        assert!(!same_origin_loopback(&headers_with("example.com:4317")));
        assert!(!same_origin_loopback(&headers_with("192.168.1.1:4317")));
    }

    #[test]
    fn missing_host_rejected() {
        assert!(!same_origin_loopback(&HeaderMap::new()));
    }

    #[test]
    fn matching_origin_accepted() {
        let h = headers_with_origin("127.0.0.1:4317", "http://127.0.0.1:4317");
        assert!(same_origin_loopback(&h));
    }

    #[test]
    fn mismatched_origin_host_rejected() {
        let h = headers_with_origin("127.0.0.1:4317", "http://example.com:4317");
        assert!(!same_origin_loopback(&h));
    }

    #[test]
    fn mismatched_origin_port_rejected() {
        let h = headers_with_origin("127.0.0.1:4317", "http://127.0.0.1:9999");
        assert!(!same_origin_loopback(&h));
    }

    #[test]
    fn invalid_origin_rejected() {
        let h = headers_with_origin("127.0.0.1:4317", "not-a-url");
        assert!(!same_origin_loopback(&h));
    }

    // --- mutation_token ---

    #[test]
    fn mutation_token_correct_accepted() {
        let token = "abc123";
        let mut h = HeaderMap::new();
        h.insert("x-architext-mutation-token", token.parse().unwrap());
        assert!(mutation_authorized(&h, token));
    }

    #[test]
    fn mutation_token_wrong_rejected() {
        let mut h = HeaderMap::new();
        h.insert("x-architext-mutation-token", "wrong".parse().unwrap());
        assert!(!mutation_authorized(&h, "correct"));
    }

    #[test]
    fn mutation_token_absent_rejected() {
        assert!(!mutation_authorized(&HeaderMap::new(), "token"));
    }

    // --- generate_mutation_token ---

    #[test]
    fn token_is_base64url_and_correct_length() {
        let token = generate_mutation_token();
        // 32 bytes in base64url without padding: ceil(32*4/3) = 43 chars
        // (32 / 3 = 10 full triplets + 2 leftover bytes → 10*4 + 3 = 43)
        assert_eq!(token.len(), 43, "expected 43 chars for 32 base64url bytes, got {token}");
        assert!(
            token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "token must be base64url alphabet: {token}"
        );
    }
}
