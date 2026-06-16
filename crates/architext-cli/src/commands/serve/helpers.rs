//! Pure helpers for the serve lifecycle.
//!
//! Port of `isLoopbackServeUrl`, `browserOpenCommand`, and `serveUrl` from
//! `src/adapters/cli/serve-lifecycle.mjs`.

use crate::args::is_loopback_host;

/// Port of `serveUrl(options)`: `http://<host>:<port>/`.
pub fn serve_url(host: &str, port: u16) -> String {
    format!("http://{host}:{port}/")
}

/// Port of `isLoopbackServeUrl(url)`: a parseable `http:` URL whose host is a
/// loopback address.
pub fn is_loopback_serve_url(url: &str) -> bool {
    // Minimal URL parse matching the JS `new URL(url)` use: scheme must be
    // `http:`, then extract the hostname. We avoid pulling a URL crate to keep
    // the helper pure and dependency-light; the JS only inspects protocol +
    // hostname here.
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    // Authority ends at the first '/', '?', or '#'.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return false;
    }
    // Strip userinfo (everything up to and including '@').
    let host_port = match authority.rfind('@') {
        Some(i) => &authority[i + 1..],
        None => authority,
    };
    // Extract host: IPv6 in [brackets], else up to first ':'.
    let host = if let Some(stripped) = host_port.strip_prefix('[') {
        match stripped.find(']') {
            // `i` indexes within `stripped` (after the `[`); the closing `]` is
            // at `i+1` in `host_port`, so keep `[..host..]` = `&host_port[..i+2]`.
            Some(i) => &host_port[..i + 2], // keep brackets for is_loopback_host
            None => return false,
        }
    } else {
        match host_port.find(':') {
            Some(i) => &host_port[..i],
            None => host_port,
        }
    };
    is_loopback_host(host)
}

/// A platform browser-open command (command + args), or `None` if unsupported.
#[derive(Debug, PartialEq, Eq)]
pub struct BrowserOpenCommand {
    pub command: String,
    pub args: Vec<String>,
}

/// Port of `browserOpenCommand(platform, url)`. `platform` is the Node
/// `process.platform` value: `"darwin"`, `"win32"`, `"linux"`.
pub fn browser_open_command(platform: &str, url: &str) -> Option<BrowserOpenCommand> {
    match platform {
        "darwin" => Some(BrowserOpenCommand {
            command: "open".to_string(),
            args: vec![url.to_string()],
        }),
        "win32" => Some(BrowserOpenCommand {
            command: "cmd".to_string(),
            args: vec!["/c".to_string(), "start".to_string(), String::new(), url.to_string()],
        }),
        "linux" => Some(BrowserOpenCommand {
            command: "xdg-open".to_string(),
            args: vec![url.to_string()],
        }),
        _ => None,
    }
}

/// Map Rust's `std::env::consts::OS` to the Node `process.platform` string the
/// JS `browserOpenCommand` expects.
pub fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other, // "linux" etc. line up directly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_serve_url_cases() {
        assert!(is_loopback_serve_url("http://127.0.0.1:4317/"));
        assert!(is_loopback_serve_url("http://localhost:4317/"));
        assert!(is_loopback_serve_url("http://[::1]:4317/"));
        assert!(is_loopback_serve_url("http://127.0.0.2/"));
        // Non-loopback host → false
        assert!(!is_loopback_serve_url("http://0.0.0.0:4317/"));
        assert!(!is_loopback_serve_url("http://example.com/"));
        // Non-http scheme → false (matches JS protocol === "http:")
        assert!(!is_loopback_serve_url("https://127.0.0.1/"));
        assert!(!is_loopback_serve_url("not a url"));
    }

    #[test]
    fn browser_open_command_per_platform() {
        assert_eq!(
            browser_open_command("darwin", "http://x/"),
            Some(BrowserOpenCommand { command: "open".into(), args: vec!["http://x/".into()] })
        );
        assert_eq!(
            browser_open_command("win32", "http://x/"),
            Some(BrowserOpenCommand {
                command: "cmd".into(),
                args: vec!["/c".into(), "start".into(), "".into(), "http://x/".into()]
            })
        );
        assert_eq!(
            browser_open_command("linux", "http://x/"),
            Some(BrowserOpenCommand { command: "xdg-open".into(), args: vec!["http://x/".into()] })
        );
        assert_eq!(browser_open_command("freebsd", "http://x/"), None);
    }
}
