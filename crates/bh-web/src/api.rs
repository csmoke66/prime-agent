//! REST helpers for what a browser cannot do through the daemon wire: the settings and keybindings
//! files, path completion, uploads, export downloads, and the bundled changelog.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::AppState;

type ApiResult<T> = Result<T, (StatusCode, String)>;

fn internal(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn bad_request(message: &str) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, message.to_string())
}

fn exports_dir(state: &AppState) -> PathBuf {
    state.scratch.join("exports")
}

fn uploads_dir(state: &AppState) -> PathBuf {
    state.scratch.join("uploads")
}

/// A file name that is safe to join onto a directory: no separators, no dot-only names.
fn plain_file_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0']);
    ok.then_some(name)
}

fn read_json_object(path: &Path) -> ApiResult<Map<String, Value>> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Map::new()),
        Ok(text) => match serde_json::from_str(&text).map_err(internal)? {
            Value::Object(map) => Ok(map),
            _ => Err(internal(format!("{} is not a JSON object", path.display()))),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(internal(error)),
    }
}

pub(crate) async fn info(State(state): State<Arc<AppState>>) -> Json<Value> {
    let options = &state.options;
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "cwd": options.cwd,
        "agentDir": options.agent_dir,
        "socketPath": options.socket_path,
        "exportDir": exports_dir(&state),
        "uploadDir": uploads_dir(&state),
    }))
}

pub(crate) async fn get_settings(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let settings = read_json_object(&state.options.agent_dir.join("settings.json"))?;
    Ok(Json(Value::Object(settings)))
}

/// Shallow merge into `settings.json`: each top-level key of the body replaces the stored one, and a
/// `null` value removes it. The write is atomic (temp file + rename).
pub(crate) async fn put_settings(
    State(state): State<Arc<AppState>>,
    Json(patch): Json<Map<String, Value>>,
) -> ApiResult<Json<Value>> {
    let path = state.options.agent_dir.join("settings.json");
    let mut settings = read_json_object(&path)?;
    for (key, value) in patch {
        if value.is_null() {
            settings.remove(&key);
        } else {
            settings.insert(key, value);
        }
    }
    let text = serde_json::to_string_pretty(&settings).map_err(internal)?;
    let temp = path.with_extension("json.web-tmp");
    std::fs::write(&temp, text).map_err(internal)?;
    std::fs::rename(&temp, &path).map_err(internal)?;
    Ok(Json(Value::Object(settings)))
}

pub(crate) async fn get_keybindings(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let bindings = read_json_object(&state.options.agent_dir.join("keybindings.json"))?;
    Ok(Json(Value::Object(bindings)))
}

#[derive(Deserialize)]
pub(crate) struct FilesQuery {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    prefix: String,
}

const MAX_FILE_COMPLETIONS: usize = 200;

/// Path completion for `@path` tokens: entries of the prefix's directory whose names start with the
/// prefix's last segment, directories first.
pub(crate) async fn list_files(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FilesQuery>,
) -> ApiResult<Json<Value>> {
    let base = query.cwd.map_or_else(|| state.options.cwd.clone(), PathBuf::from);
    let (dir_part, name_prefix) = query.prefix.rsplit_once('/').unwrap_or(("", &query.prefix));
    let dir = if let Some(rest) = query.prefix.strip_prefix("~/") {
        let home = bh_types::platform::dirs::home_dir().ok_or_else(|| bad_request("no home directory"))?;
        home.join(rest.rsplit_once('/').map_or("", |(dir, _)| dir))
    } else if query.prefix.starts_with('/') {
        PathBuf::from(if dir_part.is_empty() { "/" } else { dir_part })
    } else {
        base.join(dir_part)
    };
    let show_hidden = name_prefix.starts_with('.');
    let mut entries: Vec<(bool, String)> = std::fs::read_dir(&dir)
        .map_err(|error| (StatusCode::NOT_FOUND, error.to_string()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let visible = name.starts_with(name_prefix) && (show_hidden || !name.starts_with('.'));
            visible.then(|| (entry.path().is_dir(), name))
        })
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    entries.truncate(MAX_FILE_COMPLETIONS);
    let items: Vec<Value> = entries
        .into_iter()
        .map(|(is_dir, name)| json!({ "name": name, "path": dir.join(&name), "isDir": is_dir }))
        .collect();
    Ok(Json(json!({ "dir": dir, "entries": items })))
}

#[derive(Deserialize)]
pub(crate) struct UploadQuery {
    name: String,
}

static UPLOAD_SEQ: AtomicU64 = AtomicU64::new(0);

/// Store raw bytes under the scratch upload dir and return the on-disk path, so a browser attachment
/// can be referenced from a prompt like any local file.
pub(crate) async fn upload(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let name = plain_file_name(&query.name).ok_or_else(|| bad_request("invalid file name"))?;
    let dir = uploads_dir(&state);
    std::fs::create_dir_all(&dir).map_err(internal)?;
    let path = dir.join(format!("{}-{name}", UPLOAD_SEQ.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&path, &body).map_err(internal)?;
    Ok(Json(json!({ "path": path, "size": body.len() })))
}

/// Serve a file the daemon wrote into the export dir (`export_html`/`export_jsonl` with an
/// `outputPath` under `/api/info`'s `exportDir`) as a browser download.
pub(crate) async fn download(State(state): State<Arc<AppState>>, UrlPath(name): UrlPath<String>) -> ApiResult<Response> {
    let name = plain_file_name(&name).ok_or_else(|| bad_request("invalid file name"))?;
    let data = std::fs::read(exports_dir(&state).join(name)).map_err(|error| (StatusCode::NOT_FOUND, error.to_string()))?;
    let disposition = format!("attachment; filename=\"{}\"", name.replace('"', ""));
    Ok(([(header::CONTENT_DISPOSITION, disposition)], data).into_response())
}

pub(crate) async fn changelog() -> Json<Value> {
    Json(json!({ "markdown": include_str!("../../../CHANGELOG.md") }))
}

fn logs_dir(state: &AppState) -> PathBuf {
    state.options.agent_dir.join("logs")
}

pub(crate) async fn list_logs(State(state): State<Arc<AppState>>) -> Json<Value> {
    let dir = logs_dir(&state);
    let files: Vec<Value> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| json!({ "name": entry.file_name().to_string_lossy(), "size": metadata.len() }))
        })
        .collect();
    Json(json!({ "dir": dir, "files": files }))
}

#[derive(Deserialize)]
pub(crate) struct TailQuery {
    #[serde(default = "default_tail")]
    tail: usize,
}

fn default_tail() -> usize {
    200
}

/// The last `tail` lines of one log file.
pub(crate) async fn read_log(
    State(state): State<Arc<AppState>>,
    UrlPath(name): UrlPath<String>,
    Query(query): Query<TailQuery>,
) -> ApiResult<String> {
    let name = plain_file_name(&name).ok_or_else(|| bad_request("invalid file name"))?;
    let text = std::fs::read_to_string(logs_dir(&state).join(name)).map_err(|error| (StatusCode::NOT_FOUND, error.to_string()))?;
    let lines: Vec<&str> = text.lines().collect();
    Ok(lines[lines.len().saturating_sub(query.tail)..].join("\n"))
}
