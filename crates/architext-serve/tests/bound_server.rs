//! Integration test: a server started via the public `serve_with_schema_dir`
//! entry point must actually accept connections and answer a request.
//!
//! Regression guard for the tokio non-blocking-socket panic: `bind_listener`
//! returns a *blocking* std `TcpListener`, and tokio 1.52+ panics when such an
//! fd is registered with the runtime via `from_std`. The fix sets the listener
//! non-blocking before `from_std`. A server that hits that panic binds the port
//! but never accepts a connection, so "drive a real request and get HTTP 200"
//! is exactly the behavior that breaks without the fix.

use std::net::TcpListener as StdTcpListener;
use std::time::{Duration, Instant};

use architext_serve::serve_with_schema_dir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Find a currently-free loopback port by binding `:0`, reading the assigned
/// port, then releasing it so the server under test can claim it.
fn free_loopback_port() -> u16 {
    let probe = StdTcpListener::bind("127.0.0.1:0").expect("bind ephemeral probe port");
    probe.local_addr().expect("probe local_addr").port()
}

/// Issue an HTTP/1.1 GET over a raw socket and return the numeric status code,
/// or `None` if the connection or read failed. Mirrors the CLI's reachability
/// probe so the test pulls in no HTTP-client dependency.
async fn http_get_status(host: &str, port: u16, path: &str) -> Option<u16> {
    let mut stream = TcpStream::connect((host, port)).await.ok()?;
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await.ok()?;
    let mut buf = Vec::with_capacity(256);
    stream.read_to_end(&mut buf).await.ok()?;
    let head = String::from_utf8_lossy(&buf);
    // "HTTP/1.1 200 OK" → second whitespace token is the status code.
    head.lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse::<u16>()
        .ok()
}

#[tokio::test]
async fn bound_server_answers_a_request() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data_dir = tmp.path().join("data");
    let dist_dir = tmp.path().join("dist");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&dist_dir).unwrap();

    let host = "127.0.0.1";
    let port = free_loopback_port();

    // Run the real serve entry point on a background task. Before the fix this
    // task panics during runtime registration and never accepts connections.
    let server = tokio::spawn(async move {
        serve_with_schema_dir(
            data_dir,
            dist_dir,
            host,
            port,
            "test-token".to_string(),
            None,
        )
        .await
    });

    // Poll until the server is reachable (or give up). `/api/session` needs only
    // a loopback Host header and no data files, so it answers 200 on any target.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_status = None;
    while Instant::now() < deadline {
        if server.is_finished() {
            // The serve future returned/panicked before binding — surface it.
            break;
        }
        last_status = http_get_status(host, port, "/api/session").await;
        if last_status == Some(200) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    server.abort();

    assert_eq!(
        last_status,
        Some(200),
        "server bound on {host}:{port} but never answered /api/session with 200 \
         (got {last_status:?}); the listener was likely registered with the tokio \
         runtime while still in blocking mode"
    );
}
