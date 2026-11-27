//! MCP catalog e2e: the daemon surface serves the RESOLVED catalog, the
//! paste flow installs a token service end-to-end, and the disconnect removes
//! it. The disk-cache path feeds the catalog (the fetch lane's snapshot
//! envelope; the compiled fallback is covered by the bh-core verifiers).
#![allow(clippy::large_futures)]
// 64-bit-only targets; the narrowing casts sit at bounded OS boundaries.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
// Fn length is a style gate, not correctness.
#![allow(clippy::too_many_lines)]
// API-shape opinions, not defects; the surfaces are deliberate.
#![allow(
    clippy::unnecessary_wraps,
    clippy::zero_sized_map_values,
    clippy::struct_excessive_bools,
    clippy::struct_field_names
)]
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

struct Daemon {
    child: Child,
    #[allow(dead_code)]
    socket: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The kernel Python with prime-agent-runtime installed; skipped (with a note) without a live
/// install.
fn kernel_python() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("PA_E2E_KERNEL_PYTHON") {
        let explicit = PathBuf::from(explicit);
        assert!(
            explicit.exists(),
            "PA_E2E_KERNEL_PYTHON {} not found",
            explicit.display()
        );
        return Some(explicit);
    }
    let candidate = PathBuf::from(std::env::var("HOME").map_or_else(
        |_| "/home/ubuntu/.prime/agent/kernel-venv/bin/python".to_string(),
        |home| format!("{home}/.prime/agent/kernel-venv/bin/python"),
    ));
    if candidate.exists() {
        return Some(candidate);
    }
    eprintln!(
        "kernel python {} not found; skipping live MCP catalog e2e",
        candidate.display()
    );
    None
}

/// The tests each supervise a daemon + a session worker; serializing them keeps the harness out of
/// parallel-spawn races.
static DAEMON_E2E: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[allow(clippy::zombie_processes)]
fn spawn_supervisor(socket: &Path, agent_dir: &Path, kernel_python: &Path) -> Daemon {
    let binary = env!("CARGO_BIN_EXE_bh-daemon");
    let child = Command::new(binary)
        .arg("supervisor")
        .arg("--socket")
        .arg(socket)
        .arg("--agent-dir")
        .arg(agent_dir)
        .env("PRIME_AGENT_KERNEL_PYTHON", kernel_python)
        .env("PRIME_AGENT_CODING_AGENT_DIR", agent_dir)
        // Every outbound request fails at a closed local proxy: token verification
        // fails closed with the network category and never leaves the machine.
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env(
            bh_daemon::worker::WORKER_SUPERVISOR_LOST_EXIT_MS_ENV,
            "15000",
        )
        .spawn()
        .expect("spawn bh-daemon supervisor");
    Daemon {
        child,
        socket: socket.to_path_buf(),
    }
}

fn wait_socket_ready(socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if UnixStream::connect(socket).is_ok() {
            return;
        }
        assert!(Instant::now() < deadline, "supervisor socket never came up");
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Client {
    fn connect(socket: &Path) -> (Self, Value) {
        let stream = UnixStream::connect(socket).expect("connect supervisor");
        let write_stream = stream.try_clone().expect("clone socket");
        let mut client = Client {
            reader: BufReader::new(stream),
            writer: write_stream,
        };
        let hello = client.read_line();
        (client, hello)
    }

    fn send(&mut self, value: &Value) {
        let mut line = serde_json::to_string(value).expect("serialize command");
        line.push('\n');
        self.writer.write_all(line.as_bytes()).expect("send");
        self.writer.flush().expect("flush");
    }

    fn send_command(&mut self, id: &str, command: &Value) {
        self.send(&json!({
            "type": "command",
            "id": id,
            "protocol": { "name": "prime-agent.daemon", "version": 7 },
            "command": command,
        }));
    }

    fn read_line(&mut self) -> Value {
        let mut line = String::new();
        let deadline = Instant::now() + Duration::from_mins(2);
        self.reader
            .get_mut()
            .set_read_timeout(Some(Duration::from_millis(100)))
            .expect("set timeout");
        loop {
            match self.reader.read_line(&mut line) {
                Ok(0) => panic!("supervisor closed the connection"),
                Ok(_) if line.trim().is_empty() => line.clear(),
                Ok(_) => return serde_json::from_str(line.trim()).expect("parse response line"),
                Err(error) => {
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for supervisor line: {error}"
                    );
                }
            }
        }
    }

    fn read_response(&mut self, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_mins(4);
        loop {
            assert!(Instant::now() < deadline, "no response for id {id}");
            let line = self.read_line();
            if line.get("id").and_then(Value::as_str) == Some(id) {
                return line;
            }
        }
    }

    /// Graceful supervisor stop: routes `shutdown` to the worker, then exits (Drop SIGKILL is
    /// the safety net).
    fn shutdown_daemon(&mut self) {
        self.send_command("shutdown", &json!({ "type": "shutdown" }));
        let _ = self.read_response("shutdown");
    }
}

/// TS parity (byte-exact, the daemon wire renders it on the card): the
/// pinned-definition hint.
const PINNED_FROM_RECORD_HINT: &str =
    "This service's catalog source is unavailable; its connection keeps the pinned definition.";

#[test]
fn catalog_surfaces_and_paste_installs_through_the_daemon() {
    let Some(kernel_python) = kernel_python() else {
        return;
    };
    let _serial = DAEMON_E2E.lock().unwrap();
    let dir = tempfile::tempdir().expect("tempdir");
    let agent_dir = dir.path().join("agent");
    std::fs::create_dir_all(&agent_dir).expect("agent dir");
    let sessions_dir = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions_dir).expect("sessions dir");

    let socket = dir.path().join("mcp.sock");
    let _daemon = spawn_supervisor(&socket, &agent_dir, &kernel_python);
    wait_socket_ready(&socket);
    let (mut client, hello) = Client::connect(&socket);
    assert_eq!(hello["type"], "daemon_hello");

    client.send_command(
        "c1",
        &json!({
            "type": "create",
            "config": {
                "cwd": dir.path().to_string_lossy(),
                "sessionDir": sessions_dir.to_string_lossy(),
            },
        }),
    );
    let created = client.read_response("c1");
    assert_eq!(created["success"], true, "create failed: {created}");
    let session_id = created["data"]["activeSessionId"]
        .as_str()
        .or_else(|| created["data"]["id"].as_str())
        .expect("active session id")
        .to_string();

    client.send_command(
        "m1",
        &json!({ "type": "get_mcp_connections", "activeSessionId": session_id }),
    );
    let roster = client.read_response("m1");
    assert_eq!(
        roster["success"], true,
        "get_mcp_connections failed: {roster}"
    );
    let services = roster["data"]["services"]
        .as_array()
        .unwrap_or_else(|| panic!("services array: {roster}"));
    assert!(
        services.len() >= 69,
        "the compiled-in catalog surfaces: {services:?}"
    );
    let linear = services
        .iter()
        .find(|service| service["serviceId"] == "linear")
        .expect("linear discovery row");
    assert_eq!(linear["connectionStatus"], "not_connected");
    let slack = services
        .iter()
        .find(|service| service["serviceId"] == "slack")
        .expect("compiled Slack connector row");
    assert_eq!(
        (&slack["connectionStatus"], &slack["connectable"]),
        (&json!("not_connected"), &json!(true)),
    );
    assert_eq!(
        linear["connectable"], true,
        "the metadata-reviewed OAuth builtin is connectable"
    );
    let paste = services
        .iter()
        .find(|service| service["serviceId"] == "pagerduty")
        .expect("paste fixture row");
    assert_eq!(paste["connectionStatus"], "setup_required");
    assert_eq!(
        paste["pasteToken"], true,
        "the paste marker drives the paste panel"
    );
    let github = services
        .iter()
        .find(|service| service["serviceId"] == "github")
        .expect("github discovery row");
    assert_eq!(
        github["pasteToken"], true,
        "github is pasteable (alias pair)"
    );
    // The api-key credential section serves the stored-key rows the view
    // manages alongside the connections: the web-search entry, honestly
    // unconfigured in the fresh agent dir.
    let credentials = roster["data"]["credentials"]
        .as_array()
        .unwrap_or_else(|| panic!("credentials array: {roster}"));
    let serper = credentials
        .iter()
        .find(|credential| credential["id"] == "serper")
        .expect("the web-search credential row");
    assert_eq!(serper["label"], "Serper (web search)");
    assert_eq!(
        serper["configured"], false,
        "a fresh agent dir holds no serper key"
    );

    // The paste flow installs end-to-end: the credential is stored bound to the endpoint;
    // verification against the unreachable endpoint fails closed with the network category.
    client.send_command(
        "p1",
        &json!({
            "type": "set_mcp_static_token",
            "activeSessionId": session_id,
            "server": "pagerduty",
            "token": "fixture-pasted-token",
        }),
    );
    let installed = client.read_response("p1");
    assert_eq!(
        installed["success"], true,
        "set_mcp_static_token failed: {installed}"
    );
    assert_eq!(
        installed["data"]["endpoint"],
        "https://mcp.pagerduty.com/mcp"
    );
    assert_eq!(
        installed["data"]["verified"], false,
        "the unreachable endpoint never verifies"
    );
    assert!(
        installed["data"]["error"]
            .as_str()
            .is_some_and(|error| error != "credential-changed"),
        "a fixed network category, not a guard discard: {installed}"
    );
    let auth: Value = serde_json::from_str(
        &std::fs::read_to_string(agent_dir.join("auth.json")).expect("auth.json"),
    )
    .expect("auth json");
    assert_eq!(auth["mcp:pagerduty"]["type"], "mcp_static_token");
    assert_eq!(auth["mcp:pagerduty"]["bearer"], "fixture-pasted-token");
    assert_eq!(
        auth["mcp:pagerduty"]["endpoint"],
        "https://mcp.pagerduty.com/mcp"
    );
    let records: Value = serde_json::from_str(
        &std::fs::read_to_string(agent_dir.join("mcp-connections.json"))
            .expect("mcp-connections.json"),
    )
    .expect("records json");
    let record = &records["connections"]["pagerduty"];
    assert_eq!(record["serviceId"], "pagerduty");
    assert_eq!(record["endpoint"], "https://mcp.pagerduty.com/mcp");

    client.send_command(
        "m2",
        &json!({ "type": "get_mcp_connections", "activeSessionId": session_id }),
    );
    let roster = client.read_response("m2");
    let services = roster["data"]["services"]
        .as_array()
        .unwrap_or_else(|| panic!("services array: {roster}"));
    let paste = services
        .iter()
        .find(|service| service["serviceId"] == "pagerduty")
        .expect("paste fixture row after install");
    assert_eq!(paste["connectionStatus"], "pending", "pending: {paste}");
    assert_eq!(
        paste["connectionIds"],
        json!(["pagerduty"]),
        "the account id joins the view"
    );
    let connections = roster["data"]["connections"]
        .as_array()
        .expect("connections");
    assert!(
        connections
            .iter()
            .any(|connection| connection["server"] == "pagerduty"),
        "the roster lists the installed connection: {connections:?}"
    );

    // The disconnect: the credential and the record leave together.
    client.send_command(
        "r1",
        &json!({
            "type": "remove_mcp_connection",
            "activeSessionId": session_id,
            "server": "pagerduty",
        }),
    );
    let removed = client.read_response("r1");
    assert_eq!(
        removed["success"], true,
        "remove_mcp_connection failed: {removed}"
    );
    let auth: Value = serde_json::from_str(
        &std::fs::read_to_string(agent_dir.join("auth.json")).expect("auth.json"),
    )
    .expect("auth json");
    assert!(
        auth.get("mcp:pagerduty").is_none(),
        "the credential is gone: {auth}"
    );
    client.send_command(
        "m3",
        &json!({ "type": "get_mcp_connections", "activeSessionId": session_id }),
    );
    let roster = client.read_response("m3");
    let services = roster["data"]["services"]
        .as_array()
        .unwrap_or_else(|| panic!("services array: {roster}"));
    let paste = services
        .iter()
        .find(|service| service["serviceId"] == "pagerduty")
        .expect("paste fixture row after removal");
    assert_eq!(
        paste["connectionStatus"], "setup_required",
        "back to discovery: {paste}"
    );
    assert_eq!(
        paste["connectionIds"],
        json!([]),
        "no account after the disconnect"
    );
    client.shutdown_daemon();
}

/// The pinned-definition hint (TS parity): a record whose service is not in the catalog
/// keeps its pinned definition and says so.
#[test]
fn a_record_outside_the_catalog_keeps_its_pinned_definition() {
    let Some(kernel_python) = kernel_python() else {
        return;
    };
    let _serial = DAEMON_E2E.lock().unwrap();
    let dir = tempfile::tempdir().expect("tempdir");
    let agent_dir = dir.path().join("agent");
    std::fs::create_dir_all(&agent_dir).expect("agent dir");
    // The compiled-in catalog does not define the service; the durable record pins it.
    let record = json!({
        "version": 1,
        "connections": {
            "vanished-service": {
                "connectionId": "vanished-service",
                "serviceId": "vanished-service",
                "endpoint": "https://vanished.example/mcp",
                "label": "Vanished",
                "status": "pending",
                "createdAt": 1u64,
                "updatedAt": 1u64,
            }
        }
    });
    std::fs::write(agent_dir.join("mcp-connections.json"), record.to_string())
        .expect("write records");

    let socket = dir.path().join("pinned.sock");
    let _daemon = spawn_supervisor(&socket, &agent_dir, &kernel_python);
    wait_socket_ready(&socket);
    let (mut client, _hello) = Client::connect(&socket);
    let view = pinned_service_view(&mut client, dir.path(), "pinned");
    client.shutdown_daemon();
    assert_eq!(
        view["connectionStatus"], "not_connected",
        "the pinned card: {view}"
    );
    assert_eq!(
        view["connectable"], false,
        "a pin is never one-click connectable: {view}"
    );
    assert_eq!(
        view["setupHint"].as_str(),
        Some(PINNED_FROM_RECORD_HINT),
        "the vanished row carries the byte-exact TS hint: {view}"
    );
}

/// One pinned card off the daemon's `/mcp` roster: create a session, read
/// the service catalog, find the pinned row.
fn pinned_service_view(client: &mut Client, dir: &std::path::Path, id: &str) -> Value {
    client.send_command(
        &format!("c-{id}"),
        &json!({
            "type": "create",
            "config": {
                "cwd": dir.to_string_lossy(),
                "sessionDir": dir.join("sessions").to_string_lossy(),
            },
        }),
    );
    let created = client.read_response(&format!("c-{id}"));
    assert_eq!(created["success"], true, "create failed: {created}");
    let session_id = created["data"]["activeSessionId"]
        .as_str()
        .or_else(|| created["data"]["id"].as_str())
        .expect("active session id")
        .to_string();
    client.send_command(
        &format!("m-{id}"),
        &json!({ "type": "get_mcp_connections", "activeSessionId": session_id }),
    );
    let roster = client.read_response(&format!("m-{id}"));
    assert_eq!(roster["success"], true, "get_mcp_connections: {roster}");
    roster["data"]["services"]
        .as_array()
        .unwrap_or_else(|| panic!("services array: {roster}"))
        .iter()
        .find(|service| service["serviceId"] == "vanished-service")
        .expect("pinned discovery row")
        .clone()
}
