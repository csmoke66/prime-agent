// large_futures: stack futures on hot paths by design. too_many_lines: style gate
// only. Casts: 64-bit targets; narrowing sits at bounded OS/protocol boundaries.
#![allow(
    clippy::large_futures,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

//! End-to-end verifier for the clipboard/auth/update command group: a scripted
//! daemon session driven headlessly — `/copy`, `/import`, `/settings`,
//! `/login`, `/logout`, and `/update` through their
//! composition-root hooks.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use bh_types::daemon::DaemonCommand;

struct Supervisor {
    child: Child,
    socket: PathBuf,
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        graceful_shutdown(&self.socket);
        let worker_pids = child_pids_of(self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
        for pid in worker_pids {
            kill_worker(pid);
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn kill_worker(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while process_alive(pid) {
        assert!(
            Instant::now() < deadline,
            "worker {pid} survived the teardown kill"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "linux")]
fn process_alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        let rest = stat
            .rsplit_once(')')
            .map(|(_, rest)| rest)
            .unwrap_or_default();
        !rest.starts_with('Z')
    })
}

#[cfg(target_os = "linux")]
fn child_pids_of(ppid: u32) -> Vec<u32> {
    let mut pids = Vec::new();
    let entries = std::fs::read_dir("/proc").expect("read /proc");
    for entry in entries.flatten() {
        let Ok(entry_pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{entry_pid}/stat")) else {
            continue;
        };
        let Some((_, rest)) = stat.rsplit_once(')') else {
            continue;
        };
        let mut fields = rest.split_whitespace();
        fields.next(); // process state
        let Ok(parent) = fields.next().unwrap_or_default().parse::<u32>() else {
            continue;
        };
        if parent == ppid {
            pids.push(entry_pid);
        }
    }
    pids
}

/// The children of `ppid` without `/proc`: one `ps` listing of every pid's parent.
#[cfg(not(target_os = "linux"))]
fn child_pids_of(parent_pid: u32) -> Vec<u32> {
    let output = Command::new("ps")
        .args(["-A", "-o", "pid=,ppid="])
        .output()
        .expect("run ps");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let parent: u32 = fields.next()?.parse().ok()?;
            (parent == parent_pid).then_some(pid)
        })
        .collect()
}

/// Liveness without `/proc`: the platform probe, which also reads zombies as dead.
#[cfg(not(target_os = "linux"))]
fn process_alive(pid: u32) -> bool {
    bh_types::platform::process::is_process_alive(pid).unwrap_or(false)
}

fn graceful_shutdown(socket: &Path) {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    let Ok(stream) = UnixStream::connect(socket) else {
        return;
    };
    let Ok(write_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut writer = write_half;
    let mut hello = String::new();
    let _ = reader.read_line(&mut hello); // daemon_hello
    let command = serde_json::json!({
        "type": "command",
        "id": "test-shutdown",
        "protocol": { "name": "beehive.daemon", "version": 7 },
        "command": { "type": "shutdown" },
    });
    let mut line = serde_json::to_string(&command).expect("serialize");
    line.push('\n');
    if writer.write_all(line.as_bytes()).is_err() {
        return;
    }
    let _ = writer.flush();
    let _ = reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(5)));
    let mut response = String::new();
    let _ = reader.read_line(&mut response);
}

#[allow(clippy::zombie_processes)]
fn spawn_supervisor(dir: &Path) -> Supervisor {
    let socket = dir.join("daemon.sock");
    let agent_dir = dir.join("agent");
    std::fs::create_dir_all(&agent_dir).expect("agent dir");
    let stderr_path = dir.join("supervisor.stderr");
    let mut command = Command::new(env!("CARGO_BIN_EXE_beehive"));
    command
        .args(["--mode", "daemon", "--daemon-socket"])
        .arg(&socket)
        .env("BEEHIVE_CODING_AGENT_DIR", &agent_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&stderr_path).expect("supervisor stderr"));
    for var in [
        bh_daemon::worker::WORKER_ROLE_ENV,
        bh_daemon::worker::WORKER_TOKEN_ENV,
        bh_daemon::worker::WORKER_ACTIVE_SESSION_ID_ENV,
        bh_daemon::worker::WORKER_RECOVERY_JOURNAL_ENV,
        bh_daemon::worker::WORKER_SUPERVISOR_SOCKET_ENV,
        bh_daemon::worker::WORKER_SOCKET_ENV,
        bh_daemon::worker::WORKER_INSTANCE_ID_ENV,
        bh_daemon::worker::WORKER_SCRIPT_ENV,
    ] {
        command.env_remove(var);
    }
    for provider in bh_ai::models_generated::get_providers() {
        if let Some(vars) = bh_ai::env_api_keys::get_api_key_env_vars(provider) {
            for var in vars {
                command.env_remove(var);
            }
        }
    }
    command.env(
        bh_daemon::worker::WORKER_SUPERVISOR_LOST_EXIT_MS_ENV,
        "15000",
    );
    let mut child = command.spawn().expect("spawn beehive --mode daemon");
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("check supervisor exit") {
            let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
            panic!("supervisor exited before accepting connections ({status}); stderr: {stderr}");
        }
        if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
            return Supervisor { child, socket };
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
    panic!("supervisor never accepted a connection within 15s; stderr: {stderr}");
}

// Scripted hooks (composition-root seams; the verifier drives the traits directly).

use std::sync::{Arc, Mutex};

use bh_tui::provider_auth::{
    AuthFlow, AuthStatusIndicator, AuthStatusStyle, AuthType, ProviderAuthCommands,
    ProviderAuthFuture, ProviderAuthOutcome, ProviderRow, ProviderRowsFuture,
    ProviderWarningFuture,
};

/// The login/logout flows the scripted hook serves (one API-key provider).
struct ScriptedProviderAuth {
    stored_keys: Mutex<std::collections::HashMap<String, String>>,
    calls: Mutex<Vec<String>>,
}

impl ScriptedProviderAuth {
    fn new() -> Self {
        ScriptedProviderAuth {
            stored_keys: Mutex::new(std::collections::HashMap::default()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn handle(self) -> bh_tui::provider_auth::ProviderAuthCommandsHandle {
        bh_tui::provider_auth::ProviderAuthCommandsHandle(Arc::new(self))
    }

    fn openai_row(&self, name: &str, auth_type: AuthType) -> ProviderRow {
        let configured = self.stored_keys.lock().unwrap().contains_key("openai");
        ProviderRow {
            id: "openai".to_string(),
            name: name.to_string(),
            auth_type,
            status: (!configured).then(|| AuthStatusIndicator {
                style: AuthStatusStyle::Muted,
                label: "unconfigured".to_string(),
            }),
            flow: AuthFlow::ApiKeyPrompt,
            configured,
            available: true,
        }
    }
}

impl ProviderAuthCommands for ScriptedProviderAuth {
    fn login_options(&self) -> ProviderRowsFuture {
        let row = self.openai_row("OpenAI", AuthType::ApiKey);
        Box::pin(async move { vec![row] })
    }

    fn logout_options(&self) -> ProviderRowsFuture {
        let stored: Vec<String> = self.stored_keys.lock().unwrap().keys().cloned().collect();
        Box::pin(async move {
            stored
                .into_iter()
                .map(|id| ProviderRow {
                    name: "OpenAI".to_string(),
                    auth_type: AuthType::ApiKey,
                    status: Some(AuthStatusIndicator {
                        style: AuthStatusStyle::Success,
                        label: "configured".to_string(),
                    }),
                    id,
                    flow: AuthFlow::ApiKeyPrompt,
                    configured: true,
                    available: true,
                })
                .collect()
        })
    }

    fn login(&self, provider: &ProviderRow, api_key: Option<&str>) -> ProviderAuthFuture {
        let key = api_key.unwrap_or_default().to_string();
        let (id, name) = (provider.id.clone(), provider.name.clone());
        if key.is_empty() {
            return Box::pin(async move {
                ProviderAuthOutcome::Error(format!(
                    "Failed to save API key for {name}: API key cannot be empty."
                ))
            });
        }
        self.stored_keys.lock().unwrap().insert(id.clone(), key);
        self.calls.lock().unwrap().push(format!("login:{id}"));
        Box::pin(async move {
            ProviderAuthOutcome::Status(format!(
                "Saved API key for {name}. Credentials saved to /agent/auth.json"
            ))
        })
    }

    fn login_on_panel(
        &self,
        _provider: &ProviderRow,
        _panel: bh_tui::auth_panel::AuthPanelHandle,
    ) -> ProviderAuthFuture {
        Box::pin(async move { ProviderAuthOutcome::Cancelled })
    }

    fn logout(&self, provider: &ProviderRow) -> ProviderAuthFuture {
        let (id, name) = (provider.id.clone(), provider.name.clone());
        self.stored_keys.lock().unwrap().remove(&id);
        self.calls.lock().unwrap().push(format!("logout:{id}"));
        Box::pin(async move {
            ProviderAuthOutcome::Status(format!(
                "Removed stored API key for {name}. Environment variables and models.json config are unchanged."
            ))
        })
    }

    fn anthropic_subscription_warning(&self) -> ProviderWarningFuture {
        // The clipboard e2e drives no Anthropic subscription auth.
        Box::pin(async move { None })
    }
}

/// The interactive options over one attached scripted session, hooks injected.
#[allow(clippy::too_many_arguments)]
fn command_options(
    socket: &Path,
    dir: &Path,
    session_dir: &Path,
    script_path: &Path,
    session_id: &str,
    provider_auth: Option<bh_tui::provider_auth::ProviderAuthCommandsHandle>,
) -> bh_tui::interactive::InteractiveOptions {
    bh_tui::interactive::InteractiveOptions {
        models: None,
        socket_path: socket.to_path_buf(),
        cwd: dir.to_path_buf(),
        session_dir: Some(session_dir.to_path_buf()),
        script_path: Some(script_path.to_path_buf()),
        model_selection: bh_tui::interactive::ModelSelection::default(),
        model_catalog: Vec::new(),
        model_configured_providers: std::collections::HashSet::default(),
        model_recent_models: Vec::new(),
        default_thinking_level: None,
        no_session: false,
        session: bh_tui::interactive::SessionSelection::Attach(session_id.to_string()),
        show_images: false,
        client_settings: None,
        fullscreen_mouse: true,
        initial_message: None,
        theme: "beehive".to_string(),
        code_block_indent: "  ".to_string(),
        tree_filter_mode: String::new(),
        branch_summary_skip_prompt: false,
        version: "0.0.0".to_string(),
        onboarding: None,
        telemetry_disabled: None,
        client_auth: None,
        provider_auth,
        telemetry: None,
        keybindings: bh_tui::keybindings::KeybindingsManager::new(),
        session_rlm_depth: None,
        prompt_stash: std::sync::Arc::default(),
        session_has_children: false,
        restore_dock_focus: false,
    }
}

/// One scripted session created against the supervisor (the interactive create flow).
async fn create_session_via_daemon(
    socket: &Path,
    script_path: &Path,
    script: &serde_json::Value,
    cwd: &Path,
    session_dir: &Path,
) -> String {
    std::fs::write(script_path, script.to_string()).expect("write script");
    let (client, _events) = bh_tui::daemon_client::DaemonClient::connect(socket)
        .await
        .expect("connect supervisor");
    let data = client
        .request_ok(DaemonCommand::Create {
            id: None,
            session_path: None,
            continue_recent: None,
            no_session: None,
            name: None,
            config: Some(serde_json::json!({
                "cwd": cwd.display().to_string(),
                "sessionDir": session_dir.display().to_string(),
                "script": script_path.display().to_string(),
            })),
            telemetry_disabled: None,
            runtime_metadata: None,
            lifecycle: None,
            env: None,
            launch_env: None,
            rest: serde_json::Map::default(),
        })
        .await
        .expect("create session");
    client.close();
    data.get("activeSessionId")
        .or_else(|| data.get("id"))
        .and_then(serde_json::Value::as_str)
        .expect("session id")
        .to_string()
}

/// The joined rendered frames of one headless run.
fn rendered_frames(outcome: &bh_tui::interactive::InteractiveOutcome) -> String {
    outcome.frames.join("\n")
}

fn enter() -> bh_tui::interactive::HeadlessStep {
    bh_tui::interactive::HeadlessStep::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ))
}

// /copy

#[tokio::test]
async fn tui_copy_emits_the_ts_osc52_sequence() {
    use base64::Engine;
    // No clipboard tools in the verifier: the copy chain falls to OSC 52 (the TS fallback).
    for var in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "SSH_CONNECTION",
        "SSH_CLIENT",
        "MOSH_CONNECTION",
    ] {
        std::env::remove_var(var);
    }
    let dir = tempfile::TempDir::new().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    let session_dir = agent_dir.join("sessions");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    let supervisor = spawn_supervisor(dir.path());
    let script = serde_json::json!({ "engine": "faux", "responses": [
        { "text": "hello from scripted" },
    ] });
    let script_path = dir.path().join("script.json");
    std::fs::write(&script_path, script.to_string()).expect("write script");
    let session_id = create_session_via_daemon(
        &supervisor.socket,
        &script_path,
        &script,
        dir.path(),
        &session_dir,
    )
    .await;
    let options = command_options(
        &supervisor.socket,
        dir.path(),
        &session_dir,
        &script_path,
        &session_id,
        None,
    );
    let plan = bh_tui::interactive::HeadlessPlan {
        steps: vec![
            bh_tui::interactive::HeadlessStep::Submit("/copy".to_string()),
            bh_tui::interactive::HeadlessStep::Submit("hi".to_string()),
            // Let the turn start before the idle barrier samples the state (it passes when idle).
            bh_tui::interactive::HeadlessStep::WaitMs(400),
            bh_tui::interactive::HeadlessStep::WaitIdle { timeout_ms: 30_000 },
            bh_tui::interactive::HeadlessStep::Submit("/copy".to_string()),
            bh_tui::interactive::HeadlessStep::Submit("/copy extra".to_string()),
        ],
        width: 120,
        height: 36,
    };
    let outcome =
        bh_tui::interactive::run_interactive(options, bh_tui::interactive::UiMode::Headless(plan))
            .await
            .expect("interactive run");
    let rendered = rendered_frames(&outcome);
    assert!(
        rendered.contains("No agent messages to copy yet."),
        "the empty-transcript error renders:\n{rendered}"
    );
    assert!(
        rendered.contains("Clipboard request sent; paste in the local terminal to verify delivery"),
        "the copy status renders:\n{rendered}"
    );
    assert!(
        rendered.contains("Usage: /copy"),
        "the usage error renders:\n{rendered}"
    );
    // The exact TS OSC 52 sequence for the scripted assistant text.
    let encoded = base64::engine::general_purpose::STANDARD.encode("hello from scripted");
    assert_eq!(
        outcome.clipboard_emissions,
        vec![format!("\x1b]52;c;{encoded}\x07")],
        "the OSC 52 emission holds the TS byte shape"
    );
}

#[tokio::test]
async fn tui_copy_toast_coalesces_consecutive_copies_and_auto_dismisses() {
    use base64::Engine;
    for var in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "SSH_CONNECTION",
        "SSH_CLIENT",
        "MOSH_CONNECTION",
    ] {
        std::env::remove_var(var);
    }
    let dir = tempfile::TempDir::new().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    let session_dir = agent_dir.join("sessions");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    let supervisor = spawn_supervisor(dir.path());
    let script = serde_json::json!({ "engine": "faux", "responses": [
        { "text": "hello from scripted" },
    ] });
    let script_path = dir.path().join("script.json");
    std::fs::write(&script_path, script.to_string()).expect("write script");
    let session_id = create_session_via_daemon(
        &supervisor.socket,
        &script_path,
        &script,
        dir.path(),
        &session_dir,
    )
    .await;
    let options = command_options(
        &supervisor.socket,
        dir.path(),
        &session_dir,
        &script_path,
        &session_id,
        None,
    );
    let plan = bh_tui::interactive::HeadlessPlan {
        steps: vec![
            bh_tui::interactive::HeadlessStep::Submit("hi".to_string()),
            bh_tui::interactive::HeadlessStep::WaitMs(400),
            bh_tui::interactive::HeadlessStep::WaitIdle { timeout_ms: 30_000 },
            bh_tui::interactive::HeadlessStep::Submit("/copy".to_string()),
            bh_tui::interactive::HeadlessStep::Submit("/copy".to_string()),
            bh_tui::interactive::HeadlessStep::Submit("/copy".to_string()),
            // The coalesced count-bump toast renders (observed, not slept-for).
            bh_tui::interactive::HeadlessStep::WaitRender {
                needle:
                    "Clipboard request sent; paste in the local terminal to verify delivery (x3)"
                        .to_string(),
                timeout_ms: 10_000,
            },
            // Past the toast's TTL: the overlay dismisses.
            bh_tui::interactive::HeadlessStep::WaitGone {
                needle: "Clipboard request sent; paste in the local terminal to verify delivery"
                    .to_string(),
                timeout_ms: 10_000,
            },
        ],
        width: 120,
        height: 36,
    };
    let outcome =
        bh_tui::interactive::run_interactive(options, bh_tui::interactive::UiMode::Headless(plan))
            .await
            .expect("interactive run");

    if let Ok(dir) = std::env::var("PA_TUI_DUMP_FRAMES") {
        for (index, frame) in outcome.frames.iter().enumerate() {
            let _ = std::fs::write(
                std::path::Path::new(&dir).join(format!("frame-{index:03}.txt")),
                frame,
            );
        }
    }
    let label = "Clipboard request sent; paste in the local terminal to verify delivery";
    // Every copy registered: the headless OSC 52 sink is one buffer for
    // the whole run, so the exact TS sequence appears three times
    // concatenated - one emission per copy.
    let encoded = base64::engine::general_purpose::STANDARD.encode("hello from scripted");
    let emission = format!("\x1b]52;c;{encoded}\x07");
    let joined = outcome.clipboard_emissions.join("");
    assert_eq!(
        joined,
        emission.repeat(3),
        "every copy ran the OSC 52 chain"
    );
    // Three consecutive copies read as one "(x3)" toast, never stacked rows.
    let coalesced_label = format!("{label} (x3)");
    assert!(
        outcome
            .frames
            .iter()
            .any(|frame| frame.contains(&coalesced_label)),
        "the coalesced count-bump toast renders"
    );
    for (index, frame) in outcome.frames.iter().enumerate() {
        let rows = frame.lines().filter(|row| row.contains(label)).count();
        assert!(
            rows <= 1,
            "frame {index} shows the copy toast at most once, got {rows}"
        );
    }
    // The toast is ephemeral: past its TTL the settled frame no longer shows it.
    let last = outcome.frames.last().expect("the settled frame");
    assert!(
        !last.contains(label),
        "the expired toast auto-dismisses:\n{last}"
    );
}

// /import

#[tokio::test]
async fn tui_import_replaces_the_session_from_a_fixture() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    let session_dir = agent_dir.join("sessions");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    let supervisor = spawn_supervisor(dir.path());
    let script = serde_json::json!({ "engine": "faux", "responses": [
        { "text": "fixture turn text" },
    ] });
    let script_path = dir.path().join("script.json");
    std::fs::write(&script_path, script.to_string()).expect("write script");
    let session_id = create_session_via_daemon(
        &supervisor.socket,
        &script_path,
        &script,
        dir.path(),
        &session_dir,
    )
    .await;
    let options = command_options(
        &supervisor.socket,
        dir.path(),
        &session_dir,
        &script_path,
        &session_id,
        None,
    );
    // The fixture: the exported branch, plus a copy whose header cwd points at a gone directory.
    let fixture = dir.path().join("fixture.jsonl");
    let gone_fixture = dir.path().join("gone-cwd-fixture.jsonl");
    let plan = bh_tui::interactive::HeadlessPlan {
        steps: vec![
            // The missing-file error (confirm first, then the TS error).
            bh_tui::interactive::HeadlessStep::Submit(
                "/import /definitely/not/there.jsonl".to_string(),
            ),
            enter(),
            bh_tui::interactive::HeadlessStep::Submit("hi".to_string()),
            bh_tui::interactive::HeadlessStep::WaitMs(400),
            bh_tui::interactive::HeadlessStep::WaitIdle { timeout_ms: 30_000 },
            bh_tui::interactive::HeadlessStep::Submit(format!("/export {}", fixture.display())),
        ],
        width: 120,
        height: 36,
    };
    let outcome =
        bh_tui::interactive::run_interactive(options, bh_tui::interactive::UiMode::Headless(plan))
            .await
            .expect("interactive run");
    let rendered = rendered_frames(&outcome);
    assert!(
        rendered.contains("Import session"),
        "the confirm panel renders:\n{rendered}"
    );
    assert!(
        rendered.contains("Replace current session with /definitely/not/there.jsonl?"),
        "the confirm message renders:\n{rendered}"
    );
    assert!(
        rendered.contains("Failed to import session: File not found: /definitely/not/there.jsonl"),
        "the TS file error renders:\n{rendered}"
    );
    assert!(
        rendered.contains("Session exported to:"),
        "the export status renders:\n{rendered}"
    );
    assert!(fixture.is_file(), "the export wrote the fixture");
    // The gone-cwd fixture: a header cwd that no longer exists (TS `MissingSessionCwdError`).
    let gone_cwd = dir.path().join("gone-dir");
    let lines: Vec<String> = std::fs::read_to_string(&fixture)
        .expect("read fixture")
        .lines()
        .map(str::to_string)
        .collect();
    let mut header: serde_json::Value =
        serde_json::from_str(&lines[0]).expect("the fixture header parses");
    header["cwd"] = serde_json::json!(gone_cwd.display().to_string());
    let mut gone_lines = vec![header.to_string()];
    gone_lines.extend(lines.into_iter().skip(1));
    std::fs::write(&gone_fixture, gone_lines.join("\n")).expect("write gone-cwd fixture");

    let fresh_id = create_session_via_daemon(
        &supervisor.socket,
        &script_path,
        &script,
        dir.path(),
        &session_dir,
    )
    .await;
    let options = command_options(
        &supervisor.socket,
        dir.path(),
        &session_dir,
        &script_path,
        &fresh_id,
        None,
    );
    let plan = bh_tui::interactive::HeadlessPlan {
        steps: vec![
            bh_tui::interactive::HeadlessStep::Submit(format!("/import {}", fixture.display())),
            enter(),
            bh_tui::interactive::HeadlessStep::WaitMs(400),
            // The gone-cwd confirm; Yes retries with the fallback cwd.
            bh_tui::interactive::HeadlessStep::Submit(format!(
                "/import {}",
                gone_fixture.display()
            )),
            enter(),
            enter(),
        ],
        width: 120,
        height: 36,
    };
    let outcome =
        bh_tui::interactive::run_interactive(options, bh_tui::interactive::UiMode::Headless(plan))
            .await
            .expect("interactive run");
    let rendered = rendered_frames(&outcome);
    assert!(
        rendered.contains("Session imported from:"),
        "the import status renders:\n{rendered}"
    );
    assert!(
        rendered.contains("fixture turn text"),
        "the imported branch's messages render:\n{rendered}"
    );
    assert!(
        rendered.contains("Session cwd not found"),
        "the missing-cwd confirm renders:\n{rendered}"
    );
    assert!(
        rendered.contains("cwd from session file does not exist"),
        "the missing-cwd message renders:\n{rendered}"
    );
    // The imported branch answers on the fresh session's daemon state.
    let (client, _events) = bh_tui::daemon_client::DaemonClient::connect(&supervisor.socket)
        .await
        .expect("connect supervisor");
    let last = client
        .request_ok(DaemonCommand::GetLastAssistantText {
            id: None,
            active_session_id: fresh_id.clone(),
            rest: serde_json::Map::default(),
        })
        .await
        .expect("get last assistant text");
    assert_eq!(
        last.get("text").and_then(serde_json::Value::as_str),
        Some("fixture turn text"),
        "the imported branch's assistant text answers on the fresh session"
    );
    client.close();
    let options = command_options(
        &supervisor.socket,
        dir.path(),
        &session_dir,
        &script_path,
        &fresh_id,
        None,
    );
    let plan = bh_tui::interactive::HeadlessPlan {
        steps: vec![bh_tui::interactive::HeadlessStep::Submit(
            "/import".to_string(),
        )],
        width: 120,
        height: 36,
    };
    let outcome =
        bh_tui::interactive::run_interactive(options, bh_tui::interactive::UiMode::Headless(plan))
            .await
            .expect("interactive run");
    assert!(
        rendered_frames(&outcome).contains("Usage: /import <path.jsonl>"),
        "the usage error renders"
    );
}

// /login + /logout

#[tokio::test]
async fn tui_login_and_logout_run_the_provider_flows() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    let session_dir = agent_dir.join("sessions");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    let supervisor = spawn_supervisor(dir.path());
    let script = serde_json::json!({ "engine": "faux", "responses": [
        { "text": "ok" },
    ] });
    let script_path = dir.path().join("script.json");
    std::fs::write(&script_path, script.to_string()).expect("write script");
    let session_id = create_session_via_daemon(
        &supervisor.socket,
        &script_path,
        &script,
        dir.path(),
        &session_dir,
    )
    .await;
    let auth = Arc::new(ScriptedProviderAuth::new());
    let auth_view = Arc::clone(&auth);
    let options = command_options(
        &supervisor.socket,
        dir.path(),
        &session_dir,
        &script_path,
        &session_id,
        Some(bh_tui::provider_auth::ProviderAuthCommandsHandle(
            Arc::clone(&auth) as Arc<dyn ProviderAuthCommands>,
        )),
    );
    let plan = bh_tui::interactive::HeadlessPlan {
        steps: vec![
            bh_tui::interactive::HeadlessStep::Submit("/login".to_string()),
            enter(),
            bh_tui::interactive::HeadlessStep::Type("sk-test-key".to_string()),
            enter(),
            bh_tui::interactive::HeadlessStep::Submit("/logout".to_string()),
            enter(),
        ],
        width: 120,
        height: 36,
    };
    let outcome =
        bh_tui::interactive::run_interactive(options, bh_tui::interactive::UiMode::Headless(plan))
            .await
            .expect("interactive run");
    let rendered = rendered_frames(&outcome);
    assert!(
        rendered.contains("Search providers"),
        "the login menu's search bar renders (the picker grammar, no title):\n{rendered}"
    );
    assert!(
        rendered.contains("OpenAI · api key"),
        "the provider row renders:\n{rendered}"
    );
    assert!(
        rendered.contains("Enter API key:"),
        "the panel prompt renders:\n{rendered}"
    );
    assert!(
        rendered.contains("Saved API key for OpenAI. Credentials saved to /agent/auth.json"),
        "the store status renders:\n{rendered}"
    );
    assert!(
        rendered.contains("Saved Credentials"),
        "the logout panel renders:\n{rendered}"
    );
    assert!(
        rendered.contains("Removed stored API key for OpenAI. Environment variables and models.json config are unchanged."),
        "the removal status renders:\n{rendered}"
    );
    assert_eq!(
        auth_view.calls.lock().unwrap().clone(),
        vec!["login:openai".to_string(), "logout:openai".to_string()],
        "the flows ran in order"
    );
    assert!(
        auth_view.stored_keys.lock().unwrap().is_empty(),
        "the logout removed the stored key"
    );

    let auth = Arc::new(ScriptedProviderAuth::new());
    let options = command_options(
        &supervisor.socket,
        dir.path(),
        &session_dir,
        &script_path,
        &session_id,
        Some(bh_tui::provider_auth::ProviderAuthCommandsHandle(
            Arc::clone(&auth) as Arc<dyn ProviderAuthCommands>,
        )),
    );
    let plan = bh_tui::interactive::HeadlessPlan {
        steps: vec![
            bh_tui::interactive::HeadlessStep::Submit("/login arg".to_string()),
            bh_tui::interactive::HeadlessStep::Submit("/logout arg".to_string()),
        ],
        width: 120,
        height: 36,
    };
    let outcome =
        bh_tui::interactive::run_interactive(options, bh_tui::interactive::UiMode::Headless(plan))
            .await
            .expect("interactive run");
    let rendered = rendered_frames(&outcome);
    assert!(
        rendered.contains("Usage: /login"),
        "the login usage error renders"
    );
    assert!(
        rendered.contains("Usage: /logout"),
        "the logout usage error renders"
    );
}

#[tokio::test]
async fn tui_logout_with_no_stored_credentials_reports_the_ts_status() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    let session_dir = agent_dir.join("sessions");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    let supervisor = spawn_supervisor(dir.path());
    let script = serde_json::json!({ "engine": "faux", "responses": [
        { "text": "ok" },
    ] });
    let script_path = dir.path().join("script.json");
    std::fs::write(&script_path, script.to_string()).expect("write script");
    let session_id = create_session_via_daemon(
        &supervisor.socket,
        &script_path,
        &script,
        dir.path(),
        &session_dir,
    )
    .await;
    let options = command_options(
        &supervisor.socket,
        dir.path(),
        &session_dir,
        &script_path,
        &session_id,
        Some(ScriptedProviderAuth::new().handle()),
    );
    let plan = bh_tui::interactive::HeadlessPlan {
        steps: vec![bh_tui::interactive::HeadlessStep::Submit(
            "/logout".to_string(),
        )],
        width: 120,
        height: 36,
    };
    let outcome =
        bh_tui::interactive::run_interactive(options, bh_tui::interactive::UiMode::Headless(plan))
            .await
            .expect("interactive run");
    let rendered = rendered_frames(&outcome);
    // The long status wraps at the frame width; assert its two line segments.
    assert!(
        rendered.contains("No stored credentials to remove. /logout only removes credentials saved by /login; environment variables and"),
        "the empty-store status head renders:\n{rendered}"
    );
    assert!(
        rendered.contains("models.json config are unchanged."),
        "the empty-store status tail renders:\n{rendered}"
    );
    assert!(
        !rendered.contains("Saved Credentials"),
        "no selector mounts for an empty store:\n{rendered}"
    );
}
