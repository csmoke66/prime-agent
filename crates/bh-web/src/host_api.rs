//! REST handlers backed by the [`WebHost`]: provider rows, the harness browser, and `/share`.

use std::sync::Arc;

use axum::extract::Query;
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::{exports_dir, plain_file_name};
use crate::host::{AuthMode, WebHost};
use crate::{AppState, HostState};

type ApiResult<T> = Result<T, (StatusCode, String)>;

#[derive(Deserialize)]
pub(crate) struct ProvidersQuery {
    mode: Option<AuthMode>,
}

pub(crate) async fn providers<H: WebHost>(
    Extension(host): Extension<Arc<HostState<H>>>,
    Query(query): Query<ProvidersQuery>,
) -> Json<Value> {
    let rows = host
        .host
        .provider_rows(query.mode.unwrap_or(AuthMode::Login))
        .await;
    Json(json!({ "providers": rows }))
}

#[derive(Deserialize)]
pub(crate) struct HarnessQuery {
    #[serde(default, rename = "sessionFile")]
    session_file: String,
}

fn session_file(path: &str) -> Option<&str> {
    (!path.is_empty()).then_some(path)
}

pub(crate) async fn harness<H: WebHost>(
    Extension(host): Extension<Arc<HostState<H>>>,
    Query(query): Query<HarnessQuery>,
) -> ApiResult<Json<Value>> {
    let entries = host
        .host
        .harness_entries(session_file(&query.session_file))
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(json!({ "entries": entries })))
}

#[derive(Deserialize)]
pub(crate) struct DeleteHarness {
    #[serde(default, rename = "sessionFile")]
    session_file: Option<String>,
    entry: Value,
}

pub(crate) async fn delete_harness<H: WebHost>(
    Extension(host): Extension<Arc<HostState<H>>>,
    Json(body): Json<DeleteHarness>,
) -> ApiResult<Json<Value>> {
    host.host
        .delete_harness_entry(body.session_file.as_deref().and_then(session_file), &body.entry)
        .map_err(|error| (StatusCode::CONFLICT, error))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct ShareRequest {
    /// Name of an HTML export inside the export dir (`/api/info`'s `exportDir`).
    file: String,
}

pub(crate) async fn share<H: WebHost>(
    Extension(host): Extension<Arc<HostState<H>>>,
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(request): Json<ShareRequest>,
) -> ApiResult<Json<Value>> {
    let name = plain_file_name(&request.file)
        .ok_or((StatusCode::BAD_REQUEST, "invalid file name".to_string()))?;
    let links = host
        .host
        .share_gist(&exports_dir(&state).join(name))
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
    Ok(Json(json!(links)))
}
