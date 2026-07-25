//! MIME type map for static file serving.
//!
//! Port of the `contentTypes` map in `src/adapters/cli/architext-cli.mjs`.

use std::path::Path;

/// Return the `Content-Type` value for a file path, based on the JS `contentTypes` map plus
/// `wasm`/`woff2` (missing from the original JS map — the viewer serves both, and their absence
/// here forces browsers off the `WebAssembly.instantiateStreaming` fast path and mislabels
/// vendored fonts).
/// Falls back to `application/octet-stream` for unknown extensions.
pub fn content_type_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn css_maps_correctly() {
        assert_eq!(content_type_for_path(Path::new("foo.css")), "text/css; charset=utf-8");
    }

    #[test]
    fn html_maps_correctly() {
        assert_eq!(content_type_for_path(Path::new("index.html")), "text/html; charset=utf-8");
    }

    #[test]
    fn js_maps_correctly() {
        assert_eq!(content_type_for_path(Path::new("app.js")), "text/javascript; charset=utf-8");
    }

    #[test]
    fn json_maps_correctly() {
        assert_eq!(content_type_for_path(Path::new("data.json")), "application/json; charset=utf-8");
    }

    #[test]
    fn svg_maps_correctly() {
        assert_eq!(content_type_for_path(Path::new("icon.svg")), "image/svg+xml; charset=utf-8");
    }

    #[test]
    fn wasm_maps_correctly() {
        assert_eq!(
            content_type_for_path(Path::new("architext-viewer_bg.wasm")),
            "application/wasm"
        );
    }

    #[test]
    fn woff2_maps_correctly() {
        assert_eq!(content_type_for_path(Path::new("hanken-grotesk-400.woff2")), "font/woff2");
    }

    #[test]
    fn unknown_extension_fallback() {
        assert_eq!(content_type_for_path(Path::new("file.bin")), "application/octet-stream");
        assert_eq!(content_type_for_path(Path::new("file")), "application/octet-stream");
    }
}
