//! `architext-serve` binary — HTTP serve adapter for the Architext viewer.
//!
//! Usage:
//!   architext-serve --data-dir <dir> [--dist <viewer-dist-dir>]
//!                   [--port N] [--host H]
//!
//! Defaults: host=127.0.0.1, port=4317, dist=<package-default>.

use clap::Parser;
use std::path::PathBuf;

use architext_serve::{security::generate_mutation_token, serve, DEFAULT_HOST, DEFAULT_PORT};

#[derive(Parser, Debug)]
#[command(name = "architext-serve", about = "Architext HTTP server (Rust)")]
struct Args {
    /// Path to the Architext data directory (the `docs/architext/data` folder).
    #[arg(long)]
    data_dir: PathBuf,

    /// Path to the viewer dist directory containing index.html and assets.
    #[arg(long, default_value = "viewer/dist")]
    dist: PathBuf,

    /// Port to listen on (will search up to 50 consecutive ports if busy).
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,

    /// Host / bind address (loopback only).
    #[arg(long, default_value = DEFAULT_HOST)]
    host: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let mutation_token = generate_mutation_token();

    if let Err(e) = serve(args.data_dir, args.dist, &args.host, args.port, mutation_token).await {
        eprintln!("architext-serve error: {e}");
        std::process::exit(1);
    }
}
