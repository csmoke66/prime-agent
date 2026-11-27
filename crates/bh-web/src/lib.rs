//! Backend of `beehive web`. See the crate README for scope and non-goals.

mod api;
mod assets;
mod auth;
mod bridge;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tokio::net::TcpListener;

/// Everything the server needs from the launching command.
#[derive(Debug, Clone)]
pub struct WebOptions {
    /// Daemon supervisor socket the browser connections are bridged to.
    pub socket_path: PathBuf,
    /// Agent dir holding `settings.json` and `keybindings.json`.
    pub agent_dir: PathBuf,
    /// Working directory `beehive web` was launched in (default cwd of new sessions).
    pub cwd: PathBuf,
    /// Per-launch secret; every request except the first `/?token=` visit must carry it.
    pub token: String,
    /// Host header values accepted for this listener (anti DNS-rebinding).
    pub allowed_hosts: Vec<String>,
}

pub(crate) struct AppState {
    pub(crate) options: WebOptions,
    pub(crate) scratch: PathBuf,
}

/// The full application router: auth gate, bridge, REST helpers, and embedded assets.
pub fn router(options: WebOptions) -> Router {
    let scratch = std::env::temp_dir().join(format!("beehive-web-{}", std::process::id()));
    let state = Arc::new(AppState { options, scratch });
    Router::new()
        .route("/ws", get(bridge::upgrade))
        .route("/api/info", get(api::info))
        .route(
            "/api/settings",
            get(api::get_settings).put(api::put_settings),
        )
        .route("/api/keybindings", get(api::get_keybindings))
        .route("/api/files", get(api::list_files))
        .route(
            "/api/upload",
            post(api::upload).layer(DefaultBodyLimit::max(100 << 20)),
        )
        .route("/api/download/{name}", get(api::download))
        .route("/api/changelog", get(api::changelog))
        .route("/api/logs", get(api::list_logs))
        .route("/api/logs/{name}", get(api::read_log))
        .fallback(assets::serve)
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth::gate,
        ))
        .with_state(state)
}

/// Serve `router(options)` on `listener` until the process ends.
///
/// # Errors
///
/// Returns an error when the server loop fails.
pub async fn serve(listener: TcpListener, options: WebOptions) -> Result<()> {
    axum::serve(listener, router(options)).await?;
    Ok(())
}
