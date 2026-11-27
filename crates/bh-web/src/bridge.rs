//! Browser <-> daemon bridge. One WebSocket is one daemon socket connection: every daemon JSONL line
//! becomes one text frame and every text frame becomes one line. The daemon's own `daemon_hello`
//! reaches the browser unchanged, so the browser performs the same handshake as the TUI.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use bh_types::platform::transport::connect_transport;
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::AppState;

pub(crate) async fn upgrade(State(state): State<Arc<AppState>>, ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(64 << 20)
        .on_upgrade(move |socket| async move {
            let _ = bridge(socket, &state.options.socket_path).await;
        })
}

/// Pump frames until either side closes. A daemon that cannot be reached closes the socket, which the
/// browser treats as a lost connection and retries.
pub(crate) async fn bridge(socket: WebSocket, socket_path: &std::path::Path) -> anyhow::Result<()> {
    let daemon = connect_transport(socket_path).await?;
    let (daemon_read, mut daemon_write) = daemon.split();
    let (mut ws_tx, mut ws_rx) = socket.split();

    let to_browser = async {
        let mut lines = BufReader::new(daemon_read);
        let mut line = String::new();
        loop {
            line.clear();
            if lines.read_line(&mut line).await? == 0 {
                break;
            }
            let text = line.trim_end_matches(['\n', '\r']);
            if !text.is_empty() {
                ws_tx.send(Message::text(text)).await?;
            }
        }
        anyhow::Ok(())
    };
    let to_daemon = async {
        while let Some(message) = ws_rx.next().await {
            match message? {
                Message::Text(text) => {
                    // One frame is one line: a compact re-serialization also rejects non-JSON input.
                    let value: serde_json::Value = serde_json::from_str(&text)?;
                    let mut payload = serde_json::to_vec(&value)?;
                    payload.push(b'\n');
                    daemon_write.write_all(&payload).await?;
                }
                Message::Close(_) => break,
                Message::Binary(_) | Message::Ping(_) | Message::Pong(_) => {}
            }
        }
        anyhow::Ok(())
    };
    tokio::select! {
        result = to_browser => result,
        result = to_daemon => result,
    }
}
