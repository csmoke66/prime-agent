//! The auth gate and the daemon bridge, against a fake daemon on a Unix socket.

use std::future::Future;
use std::net::SocketAddr;

use bh_web::{
    serve, AuthMode, AuthType, FlowOutcome, FlowRequest, FlowUi, PromptInput, ProviderRow, ShareLinks,
    WebHost, WebOptions,
};
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixListener};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

const TOKEN: &str = "secret-token";

/// A host with fixed rows; its login flow asks for a code and echoes what it received.
#[derive(Default)]
struct FakeHost {
    deleted: std::sync::Mutex<Vec<serde_json::Value>>,
}

impl WebHost for FakeHost {
    fn provider_rows(&self, mode: AuthMode) -> impl Future<Output = Vec<ProviderRow>> + Send {
        std::future::ready(vec![ProviderRow {
            id: "anthropic".into(),
            name: format!("{mode:?}"),
            auth_type: AuthType::Oauth,
            status: None,
            configured: false,
            available: true,
        }])
    }

    async fn run_flow(&self, request: FlowRequest, ui: FlowUi) -> FlowOutcome {
        let FlowRequest::ProviderLogin { provider } = request else {
            return FlowOutcome::Error("unexpected request".into());
        };
        ui.auth_url(&format!("https://login.example/{provider}"), None);
        let input = PromptInput { masked: true, allow_empty: false };
        match ui.prompt("Paste code", input).await {
            Some(code) => FlowOutcome::Status(format!("code {code}")),
            None => FlowOutcome::Cancelled,
        }
    }

    fn harness_entries(&self, session_file: Option<&str>) -> Result<Vec<serde_json::Value>, String> {
        Ok(vec![serde_json::json!({ "scope": "local", "id": session_file })])
    }

    fn delete_harness_entry(&self, _: Option<&str>, entry: &serde_json::Value) -> Result<(), String> {
        self.deleted.lock().unwrap().push(entry.clone());
        Ok(())
    }

    fn share_gist(&self, file: &std::path::Path) -> impl Future<Output = Result<ShareLinks, String>> + Send {
        std::future::ready(std::fs::read_to_string(file).map_err(|error| error.to_string()).map(|html| ShareLinks {
            gist_url: format!("gist:{html}"),
            preview_url: "preview".into(),
        }))
    }
}

/// An authenticated request with an optional JSON body; returns the parsed JSON body.
async fn api(addr: SocketAddr, method: &str, path: &str, body: serde_json::Value) -> serde_json::Value {
    let host = format!("127.0.0.1:{}", addr.port());
    let body = if body.is_null() { String::new() } else { body.to_string() };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nCookie: bh_web_token={TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(body).unwrap()
}

/// Poll a flow until at least `count` events arrived; returns them all.
async fn events_until(addr: SocketAddr, flow: &str, count: usize) -> serde_json::Value {
    let mut seen = Vec::new();
    while seen.len() < count {
        let batch = api(addr, "GET", &format!("{flow}?since={}", seen.len()), serde_json::Value::Null).await;
        seen.extend(batch["events"].as_array().unwrap().clone());
    }
    serde_json::Value::Array(seen)
}

async fn start(socket_path: std::path::PathBuf, agent_dir: std::path::PathBuf) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let options = WebOptions {
        socket_path,
        agent_dir,
        cwd: std::env::temp_dir(),
        token: TOKEN.to_string(),
        allowed_hosts: vec![format!("127.0.0.1:{}", addr.port())],
    };
    tokio::spawn(serve(listener, options, FakeHost::default()));
    addr
}

/// One HTTP/1.1 exchange; returns the status line and header block plus body.
async fn http(addr: SocketAddr, path: &str, extra_headers: &str, host: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{extra_headers}\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

#[tokio::test]
async fn the_gate_requires_the_token_and_a_loopback_host() {
    let dir = tempfile::tempdir().unwrap();
    let addr = start(dir.path().join("none.sock"), dir.path().to_path_buf()).await;
    let host = format!("127.0.0.1:{}", addr.port());

    let anonymous = http(addr, "/api/info", "", &host).await;
    assert!(anonymous.starts_with("HTTP/1.1 401"), "{anonymous}");

    let visit = http(addr, &format!("/?token={TOKEN}"), "", &host).await;
    assert!(
        visit.starts_with("HTTP/1.1 303") || visit.starts_with("HTTP/1.1 307"),
        "{visit}"
    );
    assert!(
        visit.to_lowercase().contains(&format!(
            "set-cookie: bh_web_token={TOKEN}; httponly; samesite=strict"
        )),
        "{visit}"
    );

    let wrong = http(addr, "/?token=nope", "", &host).await;
    assert!(wrong.starts_with("HTTP/1.1 401"), "{wrong}");

    let cookie = format!("Cookie: bh_web_token={TOKEN}\r\n");
    let authed = http(addr, "/api/info", &cookie, &host).await;
    assert!(authed.starts_with("HTTP/1.1 200"), "{authed}");

    let rebound = http(addr, "/api/info", &cookie, "evil.example:80").await;
    assert!(rebound.starts_with("HTTP/1.1 403"), "{rebound}");
}

#[tokio::test]
async fn the_bridge_forwards_json_lines_both_ways_and_refuses_foreign_origins() {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("daemon.sock");
    let daemon = UnixListener::bind(&socket_path).unwrap();
    // Fake daemon: greet, then echo each command line back as a response frame.
    tokio::spawn(async move {
        let (stream, _) = daemon.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        write.write_all(b"{\"type\":\"daemon_hello\",\"protocol\":{\"name\":\"beehive.daemon\",\"version\":7}}\n").await.unwrap();
        let mut lines = BufReader::new(read).lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            let command: serde_json::Value = serde_json::from_str(&line).unwrap();
            let reply = serde_json::json!({ "type": "response", "id": command["id"], "success": true, "data": command["command"] });
            write
                .write_all(format!("{reply}\n").as_bytes())
                .await
                .unwrap();
        }
    });
    let addr = start(socket_path, dir.path().to_path_buf()).await;
    let host = format!("127.0.0.1:{}", addr.port());

    let connect = |origin: String| {
        let mut request = format!("ws://{host}/ws").into_client_request().unwrap();
        request.headers_mut().insert(
            "Cookie",
            HeaderValue::from_str(&format!("bh_web_token={TOKEN}")).unwrap(),
        );
        request
            .headers_mut()
            .insert("Origin", HeaderValue::from_str(&origin).unwrap());
        tokio_tungstenite::connect_async(request)
    };
    assert!(connect("http://evil.example".to_string()).await.is_err());

    let (mut socket, _) = connect(format!("http://{host}")).await.unwrap();
    let hello = socket.next().await.unwrap().unwrap().into_text().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&hello).unwrap()["type"],
        "daemon_hello"
    );

    let command =
        serde_json::json!({ "type": "command", "id": "web_1", "command": { "type": "list" } });
    socket
        .send(Message::text(command.to_string()))
        .await
        .unwrap();
    let reply = socket.next().await.unwrap().unwrap().into_text().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&reply).unwrap(),
        serde_json::json!({ "type": "response", "id": "web_1", "success": true, "data": { "type": "list" } })
    );
}

#[tokio::test]
async fn a_login_flow_streams_its_url_and_prompt_and_takes_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let addr = start(dir.path().join("none.sock"), dir.path().to_path_buf()).await;
    let started = api(addr, "POST", "/api/auth/flows", serde_json::json!({ "kind": "providerLogin", "provider": "anthropic" })).await;
    let flow = format!("/api/auth/flows/{}", started["flowId"]);

    let seen = events_until(addr, &flow, 2).await;
    assert_eq!(
        seen,
        serde_json::json!([
            { "type": "authUrl", "url": "https://login.example/anthropic", "instructions": null },
            { "type": "prompt", "prompt": "Paste code", "masked": true, "allowEmpty": false },
        ])
    );

    api(addr, "POST", &format!("{flow}/answer"), serde_json::json!({ "value": "abc" })).await;
    let done = api(addr, "GET", &format!("{flow}?since=2"), serde_json::Value::Null).await;
    assert_eq!(
        done["events"],
        serde_json::json!([{ "type": "done", "outcome": { "kind": "status", "message": "code abc" } }])
    );
}

#[tokio::test]
async fn cancelling_a_flow_ends_it_as_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let addr = start(dir.path().join("none.sock"), dir.path().to_path_buf()).await;
    let started = api(addr, "POST", "/api/auth/flows", serde_json::json!({ "kind": "providerLogin", "provider": "anthropic" })).await;
    let flow = format!("/api/auth/flows/{}", started["flowId"]);
    events_until(addr, &flow, 2).await;
    api(addr, "DELETE", &flow, serde_json::Value::Null).await;
    let done = api(addr, "GET", &format!("{flow}?since=2"), serde_json::Value::Null).await;
    assert_eq!(done["events"], serde_json::json!([{ "type": "done", "outcome": { "kind": "cancelled" } }]));
}

#[tokio::test]
async fn provider_rows_follow_the_requested_mode() {
    let dir = tempfile::tempdir().unwrap();
    let addr = start(dir.path().join("none.sock"), dir.path().to_path_buf()).await;
    let rows = api(addr, "GET", "/api/auth/providers?mode=logout", serde_json::Value::Null).await;
    assert_eq!(
        rows,
        serde_json::json!({ "providers": [{ "id": "anthropic", "name": "Logout", "authType": "oauth", "status": null, "configured": false, "available": true }] })
    );
}

#[tokio::test]
async fn harness_entries_load_and_delete_through_the_host() {
    let dir = tempfile::tempdir().unwrap();
    let addr = start(dir.path().join("none.sock"), dir.path().to_path_buf()).await;
    let listed = api(addr, "GET", "/api/harness?sessionFile=s.jsonl", serde_json::Value::Null).await;
    assert_eq!(listed, serde_json::json!({ "entries": [{ "scope": "local", "id": "s.jsonl" }] }));
    let deleted = api(addr, "DELETE", "/api/harness", serde_json::json!({ "sessionFile": "s.jsonl", "entry": { "id": "m1" } })).await;
    assert_eq!(deleted, serde_json::json!({ "ok": true }));
}

#[tokio::test]
async fn share_publishes_a_file_from_the_export_dir() {
    let dir = tempfile::tempdir().unwrap();
    let addr = start(dir.path().join("none.sock"), dir.path().to_path_buf()).await;
    let info = api(addr, "GET", "/api/info", serde_json::Value::Null).await;
    let exports = std::path::PathBuf::from(info["exportDir"].as_str().unwrap());
    std::fs::create_dir_all(&exports).unwrap();
    std::fs::write(exports.join("share.html"), "<html>").unwrap();
    let links = api(addr, "POST", "/api/share", serde_json::json!({ "file": "share.html" })).await;
    assert_eq!(links, serde_json::json!({ "gistUrl": "gist:<html>", "previewUrl": "preview" }));
}
