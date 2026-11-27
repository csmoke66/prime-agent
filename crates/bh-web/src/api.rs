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

pub(crate) fn exports_dir(state: &AppState) -> PathBuf {
    state.scratch.join("exports")
}

fn uploads_dir(state: &AppState) -> PathBuf {
    state.scratch.join("uploads")
}

/// A file name that is safe to join onto a directory: no separators, no dot-only names.
pub(crate) fn plain_file_name(name: &str) -> Option<&str> {
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
    let file = state.options.agent_dir.join("settings.json");
    let mut settings = read_json_object(&file)?;
    for (key, value) in patch {
        if value.is_null() {
            settings.remove(&key);
        } else {
            settings.insert(key, value);
        }
    }
    let text = serde_json::to_string_pretty(&settings).map_err(internal)?;
    let temp = file.with_extension("json.web-tmp");
    std::fs::write(&temp, text).map_err(internal)?;
    std::fs::rename(&temp, &file).map_err(internal)?;
    Ok(Json(Value::Object(settings)))
}

pub(crate) async fn get_keybindings(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let bindings = read_json_object(&state.options.agent_dir.join("keybindings.json"))?;
    Ok(Json(Value::Object(bindings)))
}

/// The TUI agents view's presentation groups: `<agent-dir>/agents-view-groups/<sessionId>` holds
/// the label. Sharing the directory keeps the browser and the TUI on the same groups.
fn groups_dir(state: &AppState) -> PathBuf {
    state.options.agent_dir.join("agents-view-groups")
}

fn valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub(crate) async fn get_groups(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let mut groups = Map::new();
    let entries = match std::fs::read_dir(groups_dir(&state)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Json(Value::Object(groups))),
        Err(e) => return Err(internal(e)),
    };
    for entry in entries {
        let entry = entry.map_err(internal)?;
        let name = entry.file_name();
        let Some(id) = name.to_str().filter(|id| valid_session_id(id)) else {
            continue;
        };
        let label = std::fs::read_to_string(entry.path()).map_err(internal)?;
        if !label.is_empty() {
            groups.insert(id.to_owned(), Value::String(label));
        }
    }
    Ok(Json(Value::Object(groups)))
}

#[derive(Deserialize)]
pub(crate) struct GroupChange {
    #[serde(rename = "sessionId")]
    session_id: String,
    label: String,
}

/// Set (or, with an empty label, clear) one session's group, written atomically like the TUI does.
pub(crate) async fn put_group(
    State(state): State<Arc<AppState>>,
    Json(change): Json<GroupChange>,
) -> ApiResult<Json<Value>> {
    if !valid_session_id(&change.session_id) {
        return Err(bad_request("invalid session id"));
    }
    let dir = groups_dir(&state);
    let target = dir.join(&change.session_id);
    let label = change.label.trim();
    if label.is_empty() {
        match std::fs::remove_file(&target) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(internal(e)),
            _ => {}
        }
    } else {
        std::fs::create_dir_all(&dir).map_err(internal)?;
        let temp = dir.join(format!(".web-{}", std::process::id()));
        std::fs::write(&temp, label).map_err(internal)?;
        std::fs::rename(&temp, &target).map_err(internal)?;
    }
    get_groups(State(state)).await
}

#[derive(Deserialize)]
pub(crate) struct FilesQuery {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    prefix: String,
    /// Search the whole tree under `cwd` for this text instead of listing one directory.
    #[serde(default)]
    search: Option<String>,
}

const MAX_FILE_COMPLETIONS: usize = 200;
const MAX_SEARCH_VISITED: usize = 20_000;
const MAX_SEARCH_DEPTH: usize = 8;
const SEARCH_SKIPPED_DIRS: [&str; 4] = [".git", ".hg", "node_modules", "target"];

/// Rank of `path` for the `@` search: 0 name prefix, 1 name contains, 2 path contains, 3 subsequence.
fn search_rank(path: &str, query: &str) -> Option<u8> {
    let path = path.to_lowercase();
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or(&path);
    if name.starts_with(query) {
        return Some(0);
    }
    if name.contains(query) {
        return Some(1);
    }
    if path.contains(query) {
        return Some(2);
    }
    let mut rest = path.chars();
    query
        .chars()
        .all(|wanted| rest.any(|c| c == wanted))
        .then_some(3)
}

/// Breadth-first walk under `root` (bounded in depth and entries, hidden entries only when the
/// query starts with a dot) returning `(is_dir, path relative to root)` matches, best first.
fn search_tree(root: &Path, query: &str) -> Vec<(bool, String)> {
    let query = query.to_lowercase();
    let show_hidden = query.starts_with('.');
    let mut pending = std::collections::VecDeque::from([(root.to_path_buf(), String::new(), 0usize)]);
    let mut ranked: Vec<(u8, bool, String)> = Vec::new();
    let mut visited = 0;
    while let Some((dir, relative, depth)) = pending.pop_front() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.filter_map(Result::ok) {
            visited += 1;
            if visited > MAX_SEARCH_VISITED {
                pending.clear();
                break;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.path().is_dir();
            if (!show_hidden && name.starts_with('.')) || (is_dir && SEARCH_SKIPPED_DIRS.contains(&name.as_str())) {
                continue;
            }
            let path = format!("{relative}{name}{}", if is_dir { "/" } else { "" });
            if let Some(rank) = search_rank(&path, &query) {
                ranked.push((rank, is_dir, path.clone()));
            }
            if is_dir && depth < MAX_SEARCH_DEPTH {
                pending.push_back((entry.path(), path, depth + 1));
            }
        }
    }
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.len().cmp(&b.2.len())).then_with(|| a.2.cmp(&b.2)));
    ranked.into_iter().map(|(_, is_dir, path)| (is_dir, path)).collect()
}

/// Path completion for `@path` tokens. With `search`, the best matches anywhere under `cwd`;
/// otherwise the entries of the prefix's directory whose names start with the prefix's last
/// segment, directories first. Each entry's `value` is the path as the prompt should spell it.
pub(crate) async fn list_files(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FilesQuery>,
) -> ApiResult<Json<Value>> {
    let base = query
        .cwd
        .map_or_else(|| state.options.cwd.clone(), PathBuf::from);
    if let Some(search) = query.search.filter(|text| !text.is_empty()) {
        let items: Vec<Value> = search_tree(&base, &search)
            .into_iter()
            .take(MAX_FILE_COMPLETIONS)
            .map(|(is_dir, value)| json!({ "value": value, "path": base.join(&value), "isDir": is_dir }))
            .collect();
        return Ok(Json(json!({ "dir": base, "entries": items })));
    }
    let (dir_part, name_prefix) = query.prefix.rsplit_once('/').unwrap_or(("", &query.prefix));
    let dir = if let Some(rest) = query.prefix.strip_prefix("~/") {
        let home =
            bh_types::platform::dirs::home_dir().ok_or_else(|| bad_request("no home directory"))?;
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
        .map(|(is_dir, name)| {
            let shown = format!("{}{name}{}", &query.prefix[..query.prefix.len() - name_prefix.len()], if is_dir { "/" } else { "" });
            json!({ "name": name, "value": shown, "path": dir.join(&name), "isDir": is_dir })
        })
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
    let path = dir.join(format!(
        "{}-{name}",
        UPLOAD_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, &body).map_err(internal)?;
    Ok(Json(json!({ "path": path, "size": body.len() })))
}

/// Serve a file the daemon wrote into the export dir (`export_html`/`export_jsonl` with an
/// `outputPath` under `/api/info`'s `exportDir`) as a browser download.
pub(crate) async fn download(
    State(state): State<Arc<AppState>>,
    UrlPath(name): UrlPath<String>,
) -> ApiResult<Response> {
    let name = plain_file_name(&name).ok_or_else(|| bad_request("invalid file name"))?;
    let data = std::fs::read(exports_dir(&state).join(name))
        .map_err(|error| (StatusCode::NOT_FOUND, error.to_string()))?;
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
            metadata.is_file().then(
                || json!({ "name": entry.file_name().to_string_lossy(), "size": metadata.len() }),
            )
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
    let text = std::fs::read_to_string(logs_dir(&state).join(name))
        .map_err(|error| (StatusCode::NOT_FOUND, error.to_string()))?;
    let lines: Vec<&str> = text.lines().collect();
    Ok(lines[lines.len().saturating_sub(query.tail)..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::search_tree;

    #[test]
    fn search_ranks_name_prefix_first_and_skips_build_dirs() {
        let root = tempfile::tempdir().unwrap();
        for dir in ["src/deep", "target", "node_modules"] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        for file in ["src/deep/main.rs", "src/domain.rs", "target/main.o", "node_modules/main.js", "main.toml"] {
            std::fs::write(root.path().join(file), "").unwrap();
        }
        let found: Vec<String> = search_tree(root.path(), "main").into_iter().map(|(_, path)| path).collect();
        assert_eq!(found, ["main.toml", "src/deep/main.rs", "src/domain.rs"]);
    }
}
