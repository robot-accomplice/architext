//! Path traversal guard.
//!
//! Port of `safeJoin(root, requestPath)` from `src/adapters/cli/architext-cli.mjs`.
//!
//! Returns `Some(resolved)` only if the resolved path is inside `root`.
//! Returns `None` on any decode error or path-traversal attempt.
//!
//! JS logic:
//!   ```js
//!   function safeJoin(root, requestPath) {
//!     let decoded;
//!     try { decoded = decodeURIComponent(requestPath); } catch { return ""; }
//!     const resolved = path.resolve(root, decoded.replace(/^\/+/, ""));
//!     if (resolved !== root && !resolved.startsWith(`${root}${path.sep}`)) return "";
//!     return resolved;
//!   }
//!   ```
//!
//! Key behaviour: the JS `path.resolve` is purely LEXICAL (it does not hit the
//! filesystem), and the check is `resolved !== root && !resolved.startsWith(root + sep)`.
//! We replicate exactly that with `lexical_resolve`, matching without a FS call.

use std::path::{Path, PathBuf, MAIN_SEPARATOR};

/// Port of JS `safeJoin(root, requestPath)`.
///
/// - Percent-decodes `request_path`.
/// - Strips leading slashes.
/// - Lexically resolves against `root` (no FS calls — matches JS `path.resolve` semantics).
/// - Returns `None` if the result would escape `root`.
pub fn safe_join(root: &Path, request_path: &str) -> Option<PathBuf> {
    // Percent-decode — return None on decode error (matches JS catch → "")
    let decoded = percent_decode(request_path)?;
    // Strip leading slashes (matches JS `.replace(/^\/+/, "")`)
    let stripped = decoded.trim_start_matches('/');
    // Lexically resolve (JS path.resolve does NOT hit the filesystem)
    let resolved = lexical_resolve(root, stripped);

    // Accept if resolved == root (directory itself) or is a child of root.
    // Matches JS: `resolved !== root && !resolved.startsWith(\`${root}${path.sep}\`)`
    let root_str = root.to_string_lossy();
    let resolved_str = resolved.to_string_lossy();

    let sep_str = MAIN_SEPARATOR.to_string();
    let root_with_sep = format!("{root_str}{sep_str}");

    if resolved_str == root_str || resolved_str.starts_with(root_with_sep.as_str()) {
        Some(resolved)
    } else {
        None
    }
}

/// Percent-decode a URL path component. Returns None if invalid encoding.
fn percent_decode(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hi = hex_nibble(bytes[i + 1])?;
            let lo = hex_nibble(bytes[i + 2])?;
            out.push(char::from(hi << 4 | lo));
            i += 3;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Lexical path resolution: apply `..` and `.` components without hitting the FS.
/// Equivalent to Node.js `path.resolve(root, relative)` when root is absolute.
fn lexical_resolve(root: &Path, relative: &str) -> PathBuf {
    let mut result = root.to_path_buf();
    for component in relative.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                result.pop();
            }
            c => result.push(c),
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // Use an absolute root that doesn't need to exist on disk.
    const ROOT: &str = "/srv/data";

    fn root() -> &'static Path {
        Path::new(ROOT)
    }

    #[test]
    fn normal_path_accepted() {
        let result = safe_join(root(), "foo/bar.json");
        assert!(result.is_some(), "expected Some, got None");
        assert_eq!(result.unwrap(), Path::new("/srv/data/foo/bar.json"));
    }

    #[test]
    fn dot_path_accepted() {
        let result = safe_join(root(), ".");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), Path::new(ROOT));
    }

    #[test]
    fn traversal_dot_dot_rejected() {
        let result = safe_join(root(), "../../etc/passwd");
        assert!(result.is_none(), "traversal should be rejected");
    }

    #[test]
    fn traversal_encoded_rejected() {
        // %2e%2e = ".."
        let result = safe_join(root(), "%2e%2e%2f%2e%2e%2fetc%2fpasswd");
        assert!(result.is_none(), "encoded traversal should be rejected");
    }

    #[test]
    fn percent_encoded_path_decoded() {
        let result = safe_join(root(), "my%20file.json");
        assert!(result.is_some(), "expected Some");
        assert_eq!(result.unwrap(), Path::new("/srv/data/my file.json"));
    }

    #[test]
    fn invalid_percent_encoding_rejected() {
        assert!(safe_join(root(), "%zz").is_none());
        assert!(safe_join(root(), "%2").is_none());
    }

    #[test]
    fn leading_slashes_stripped() {
        let a = safe_join(root(), "data/file.json");
        let b = safe_join(root(), "/data/file.json");
        let c = safe_join(root(), "///data/file.json");
        // All three should yield the same result
        assert_eq!(a, b, "single slash prefix should match no prefix");
        assert_eq!(b, c, "multiple slash prefixes should match");
    }

    #[test]
    fn escaped_traversal_with_safe_prefix_rejected() {
        // Looks like it starts with root but then traverses out
        let result = safe_join(root(), "safe/../../../etc/passwd");
        assert!(result.is_none(), "escaped traversal should be rejected");
    }
}
