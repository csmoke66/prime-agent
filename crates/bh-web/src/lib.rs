//! Backend of `beehive web`. See the crate README for scope and non-goals.

mod api;
mod assets;
mod auth;
mod bridge;
mod flows;
mod host;
mod host_api;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, post};
use axum::{Extension, Router};
use tokio::net::TcpListener;

pub use host::{
    AuthMode, AuthType, FlowOutcome, FlowRequest, FlowUi, ProviderRow, PromptInput, ShareLinks,
    WebHost,
};

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

/// The host plus the flows it is running, shared by the host-backed handlers.
pub(crate) struct HostState<H> {
    pub(crate) host: H,
    pub(crate) flows: flows::Flows,
}

/// The full application router: auth gate, bridge, REST helpers, host-backed endpoints, and
/// embedded assets.
pub fn router<H: WebHost>(options: WebOptions, host: H) -> Router {
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
        .route("/api/groups", get(api::get_groups).put(api::put_group))
        .route("/api/files", get(api::list_files))
        .route(
            "/api/upload",
            post(api::upload).layer(DefaultBodyLimit::max(100 << 20)),
        )
        .route("/api/download/{name}", get(api::download))
        .route("/api/changelog", get(api::changelog))
        .route("/api/logs", get(api::list_logs))
        .route("/api/logs/{name}", get(api::read_log))
        .route("/api/auth/providers", get(host_api::providers::<H>))
        .route("/api/auth/flows", post(flows::start::<H>))
        .route("/api/auth/flows/{id}", get(flows::poll::<H>).delete(flows::cancel::<H>))
        .route("/api/auth/flows/{id}/answer", post(flows::answer::<H>))
        .route(
            "/api/harness",
            get(host_api::harness::<H>).delete(host_api::delete_harness::<H>),
        )
        .route("/api/share", post(host_api::share::<H>))
        .route(
            "/api/child-policy",
            get(host_api::child_policy::<H>).put(host_api::set_child_policy::<H>),
        )
        .layer(Extension(Arc::new(HostState {
            host,
            flows: flows::Flows::default(),
        })))
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
pub async fn serve<H: WebHost>(
    listener: TcpListener,
    options: WebOptions,
    host: H,
) -> Result<()> {
    axum::serve(listener, router(options, host)).await?;
    Ok(())
}
