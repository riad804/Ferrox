//! `ferrox-service` — Axum-based HTTP media processing service.
//!
//! # Routes
//!
//! | Method | Path       | Description                                          |
//! |--------|------------|------------------------------------------------------|
//! | GET    | /health    | Liveness probe; returns `{"status":"ok"}`.          |
//! | POST   | /probe     | Accept a multipart upload, return stream JSON.       |
//! | POST   | /process   | Accept a JSON job + upload, return processed file.  |
//!
//! # Running
//!
//! ```sh
//! ferrox-service --addr 0.0.0.0:8080
//! ```
//!
//! ## POST /probe
//!
//! Body: `multipart/form-data` with a `file` field.
//!
//! Response: `application/json` with stream metadata (same format as
//! `ferrox probe`).
//!
//! ## POST /process
//!
//! Body: `multipart/form-data` with:
//! - `file`   — the input media file.
//! - `job`    — JSON string describing the processing job (see [`Job`]).
//!
//! Response: the processed file as `application/octet-stream` (Content-Disposition
//! set to `attachment; filename="output.<ext>"`).
//!
//! ### Job schema
//!
//! ```json
//! {
//!   "output_format": "png",
//!   "filter_complex": "blur=2.0,grayscale"
//! }
//! ```
//!
//! `output_format` — one of `png`, `jpg`/`jpeg`.
//! `filter_complex` — optional filtergraph expression (same syntax as
//!   `ferrox filter-apply --filter-complex`).

mod handlers;
mod models;

use std::net::SocketAddr;
use axum::{Router, routing::{get, post}};
use clap::Parser;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "ferrox-service", version, about = "ferrox HTTP media processing service")]
struct Args {
    /// Address to bind to.
    #[arg(long, default_value = "127.0.0.1:8080", env = "FERROX_ADDR")]
    addr: SocketAddr,

    /// Log level (off/error/warn/info/debug/trace).
    #[arg(long, default_value = "info", env = "FERROX_LOG")]
    log_level: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| args.log_level.parse().unwrap_or_default()),
        )
        .with_target(false)
        .compact()
        .init();

    let app = Router::new()
        .route("/health",  get(handlers::health))
        .route("/probe",   post(handlers::probe))
        .route("/process", post(handlers::process))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    info!("ferrox-service listening on {}", args.addr);
    let listener = tokio::net::TcpListener::bind(args.addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
