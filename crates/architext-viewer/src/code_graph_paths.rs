//! `file` is not guaranteed repo-relative (Item 2).
//!
//! Magma has an open Go-side defect where build-cache stubs carry an
//! ABSOLUTE path (e.g. a Go build-cache path under the analysing user's home
//! directory) instead of a path relative to the repo root. That's their fix,
//! not ours — but the viewer must treat `file` as not guaranteed
//! repo-relative and present a non-relative path honestly rather than
//! rendering a raw absolute path as if it were source you could open, and
//! never by silently hiding the node — it is real data.

/// Whether a code-graph `file` path looks repo-relative rather than an
/// absolute filesystem path. This is a syntactic check (leading `/`, a `~`
/// home shorthand, or a Windows drive letter) — the viewer has no access to
/// the analysed repo's root to check membership against, so it can only
/// recognise the SHAPE of an absolute path, not confirm a relative-looking
/// one is actually correct.
pub fn is_repo_relative_path(path: &str) -> bool {
    if path.starts_with('/') || path.starts_with('~') {
        return false;
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return false; // Windows drive letter, e.g. "C:\..." or "C:/...".
    }
    true
}

/// Shorten an absolute path to its last two segments, prefixed with an
/// ellipsis, so a home-directory path doesn't dump its full length into a
/// narrow inspector column. Kept at two segments (not one) because a Go
/// build-cache stub's basename alone (`8162…-d`) is an opaque hash with no
/// context; the parent segment is what says "this is a cache shard", e.g.
/// `.../81/8162…-d`.
fn shorten_absolute_path(path: &str) -> String {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if parts.len() <= 2 {
        path.to_string()
    } else {
        format!(".../{}", parts[parts.len() - 2..].join("/"))
    }
}

/// Honest display for a code-graph `file:line` location. A non-repo-relative
/// path is never rendered as if it were a normal source path: it is
/// shortened and explicitly marked as not a repository file, rather than
/// showing a raw home-directory path as though it were something the reader
/// could open, or silently dropping the node — the node is real data.
pub fn format_location(file: &str, line: u32) -> String {
    if is_repo_relative_path(file) {
        format!("{file}:{line}")
    } else {
        format!(
            "{}:{line} — not a repository file (absolute path outside the repo)",
            shorten_absolute_path(file)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_relative_paths_are_recognised() {
        assert!(is_repo_relative_path("crates/roboticus-agent/src/loop.rs"));
        assert!(is_repo_relative_path("main.go"));
        assert!(is_repo_relative_path("a/b/c.rs"));
    }

    #[test]
    fn absolute_and_home_paths_are_rejected() {
        // The real defect: a Go build-cache stub under the analysing user's home.
        assert!(!is_repo_relative_path(
            "/Users/jmachen/Library/Caches/go-build/81/8162abc-d"
        ));
        assert!(!is_repo_relative_path("~/Library/Caches/go-build/x"));
        assert!(!is_repo_relative_path("C:\\Users\\jon\\AppData\\x.go"));
        assert!(!is_repo_relative_path("C:/Users/jon/x.go"));
    }

    #[test]
    fn format_location_renders_relative_paths_unchanged() {
        assert_eq!(format_location("main.go", 7), "main.go:7");
        assert_eq!(
            format_location("crates/roboticus-agent/src/loop.rs", 23),
            "crates/roboticus-agent/src/loop.rs:23"
        );
    }

    #[test]
    fn format_location_marks_absolute_paths_as_not_a_repository_file() {
        // WHY: "detect a non-repo-relative path and present it honestly ...
        // rather than rendering a raw absolute path as if it were source you
        // could open. Do not silently hide the node." — the node must still
        // render, just honestly.
        let loc = format_location(
            "/Users/jmachen/Library/Caches/go-build/81/8162abc-d",
            42,
        );
        assert!(loc.contains(":42"));
        assert!(loc.to_lowercase().contains("not a repository file"));
        // Shortened, not the full raw home-directory path.
        assert!(!loc.starts_with("/Users/jmachen"), "must not render the raw absolute path: {loc}");
        assert!(loc.contains("8162abc-d"), "should keep enough context to be meaningful: {loc}");
    }

    #[test]
    fn shorten_absolute_path_keeps_last_two_segments() {
        assert_eq!(
            shorten_absolute_path("/Users/jmachen/Library/Caches/go-build/81/8162abc-d"),
            ".../81/8162abc-d"
        );
        // Short paths (<=2 segments) pass through unchanged — nothing to shorten.
        assert_eq!(shorten_absolute_path("/tmp"), "/tmp");
    }
}
