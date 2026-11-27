//! The auth gate and the daemon bridge, against a fake daemon on a Unix socket.

use std::net::SocketAddr;

use bh_web::{serve, WebOptions};
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixListener};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

const TOKEN: &str = "secret-token";

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
    tokio::spawn(serve(listener, options));
    addr
}

/// One HTTP/1.1 exchange; returns the status line and header block plus body.
async fn http(addr: SocketAddr, path: &str, extra_headers: &str, host: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{extra_headers}\r\n");
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
    assert!(visit.starts_with("HTTP/1.1 303") || visit.starts_with("HTTP/1.1 307"), "{visit}");
    assert!(visit.to_lowercase().contains(&format!("set-cookie: bh_web_token={TOKEN}; httponly; samesite=strict")), "{visit}");

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
            write.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
        }
    });
    let addr = start(socket_path, dir.path().to_path_buf()).await;
    let host = format!("127.0.0.1:{}", addr.port());

    let connect = |origin: String| {
        let mut request = format!("ws://{host}/ws").into_client_request().unwrap();
        request.headers_mut().insert("Cookie", HeaderValue::from_str(&format!("bh_web_token={TOKEN}")).unwrap());
        request.headers_mut().insert("Origin", HeaderValue::from_str(&origin).unwrap());
        tokio_tungstenite::connect_async(request)
    };
    assert!(connect("http://evil.example".to_string()).await.is_err());

    let (mut socket, _) = connect(format!("http://{host}")).await.unwrap();
    let hello = socket.next().await.unwrap().unwrap().into_text().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&hello).unwrap()["type"], "daemon_hello");

    let command = serde_json::json!({ "type": "command", "id": "web_1", "command": { "type": "list" } });
    socket.send(Message::text(command.to_string())).await.unwrap();
    let reply = socket.next().await.unwrap().unwrap().into_text().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&reply).unwrap(),
        serde_json::json!({ "type": "response", "id": "web_1", "success": true, "data": { "type": "list" } })
    );
}
