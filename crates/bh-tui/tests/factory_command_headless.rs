//! Headless coverage for the Factory setting, its live-run disable guard, and the factory page.
//! The settings seam records the gate in memory; the bh-cli seam covers its on-disk shape.
#![cfg(unix)]
// Pedantic-gate exceptions (every other pedantic warning in this crate is
// fixed in place; each exception carries its one-line justification):
// - the casts: terminal-layout arithmetic narrows structurally bounded
//   values (screen coordinates, byte counts, timestamps); guarded
//   conversions would add panic paths the bounds guarantee away.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
// - the render routes are flat tables (one arm per route); splitting them
//   would add indirection without changing the flow.
#![allow(clippy::too_many_lines)]
// - widget state structs carry independent flag bits; a nested struct
//   would add indirection without changing the shape.
#![allow(clippy::struct_excessive_bools, clippy::fn_params_excessive_bools)]
// - the futures are bounded by the surface's lifetime; boxing them would
//   add an allocation to the steady-state loop.
#![allow(clippy::large_futures)]
// - the wrappers preserve a uniform Result-returning API surface; unwrap
//   removals would ripple through the callers without changing behavior.
#![allow(clippy::unnecessary_wraps)]

mod support;

use std::os::unix::net::UnixListener;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use bh_tui::interactive::{HeadlessPlan, HeadlessStep, InteractiveOptions};
use serde_json::{json, Value};

use support::{
    attach_response, create_response, detach_response, run_headless, serve_connection,
    session_stats_response, write_json, Request,
};

struct MockSupervisor {
    listener: UnixListener,
    /// The hello's advertised capabilities (the default hello advertises
    /// none, so the factory lane stays unadvertised).
    server_capabilities: Vec<String>,
    /// The `factory_activity` graph reply the mock answers while the
    /// lane is advertised: `None` answers every action with empty data.
    factory_graph: Option<Value>,
    /// The daemon refusal the mock answers the `graph` action with
    /// (`None` keeps the success path): the worker arm's exact wire
    /// shape for a session whose kernel is not built.
    factory_graph_error: Option<String>,
}

impl MockSupervisor {
    fn bind_with(
        socket: &std::path::Path,
        server_capabilities: Vec<String>,
        factory_graph: Option<Value>,
        factory_graph_error: Option<String>,
    ) -> Self {
        MockSupervisor {
            listener: UnixListener::bind(socket).expect("bind mock socket"),
            server_capabilities,
            factory_graph,
            factory_graph_error,
        }
    }

    /// Serve one connection: attach an empty session, then answer the
    /// loop's requests.
    fn serve(self) {
        let MockSupervisor {
            listener,
            server_capabilities,
            factory_graph,
            factory_graph_error,
        } = self;
        let (stream, _) = listener.accept().expect("accept");
        let capabilities: Vec<&str> = server_capabilities.iter().map(String::as_str).collect();
        // A page stop also stops the mock's session: the daemon's `session_closed` follows the
        // stop's reply, and the closed session's graph lane answers a refusal from then on.
        let mut session_closed = false;
        serve_connection(
            stream,
            &capabilities,
            |writer, Request { id, kind, command }| {
                match kind {
                    "create" => {
                        write_json(writer, &create_response(id));
                    }
                    "attach" => {
                        write_json(writer, &attach_response(id, "factory session"));
                    }
                    "get_session_stats" => {
                        write_json(writer, &session_stats_response(id));
                    }
                    "detach" => {
                        write_json(writer, &detach_response(id));
                    }
                    "factory_activity" => {
                        // The lane's mock: a graph action answers the
                        // configured refusal (the worker arm's exact
                        // kernel-not-running wire shape) or the configured
                        // runs reply; every other action answers empty data
                        // (the guard reads the graph list only).
                        if command.get("action").and_then(Value::as_str) == Some("stop") {
                            write_json(
                                writer,
                                &json!({
                                    "type": "response",
                                    "id": id,
                                    "command": "factory_activity",
                                    "success": true,
                                    "data": { "state": "stopping" },
                                }),
                            );
                            write_json(
                                writer,
                                &json!({
                                    "type": "session_closed",
                                    "activeSessionId": "s1",
                                    "reason": "killed",
                                }),
                            );
                            session_closed = true;
                            return true;
                        }
                        if command.get("action").and_then(Value::as_str) == Some("graph") {
                            let error = if session_closed {
                                Some("Unknown active session: s1".to_string())
                            } else {
                                factory_graph_error.clone()
                            };
                            if let Some(error) = error.as_ref() {
                                write_json(
                                    writer,
                                    &json!({
                                        "type": "response",
                                        "id": id,
                                        "command": "factory_activity",
                                        "success": false,
                                        "error": error,
                                    }),
                                );
                                return true;
                            }
                        }
                        let data = match (
                            factory_graph.as_ref(),
                            command.get("action").and_then(Value::as_str),
                        ) {
                            (Some(runs), Some("graph")) => runs.clone(),
                            _ => json!({}),
                        };
                        write_json(
                            writer,
                            &json!({
                                "type": "response",
                                "id": id,
                                "command": "factory_activity",
                                "success": true,
                                "data": data,
                            }),
                        );
                    }
                    _ => return false,
                }
                true
            },
        );
    }
}

/// A minimal settings seam for the harness: the factory gate reads and
/// writes through a recording cell (the opt-in default is disabled), and
/// every other getter returns its TS default with no-op writes.
#[derive(Default)]
struct RecordingSettings {
    factory_enabled: std::sync::Mutex<bool>,
}

impl bh_tui::client_settings::ClientSettings for RecordingSettings {
    fn compaction_threshold_percent(&self) -> f64 {
        95.0
    }
    fn set_compaction_threshold_percent(&self, _percent: f64) -> Result<()> {
        Ok(())
    }

    fn show_images(&self) -> bool {
        true
    }
    fn set_show_images(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn clear_on_shrink(&self) -> bool {
        false
    }
    fn set_clear_on_shrink(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn show_terminal_progress(&self) -> bool {
        false
    }
    fn set_show_terminal_progress(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn image_auto_resize(&self) -> bool {
        true
    }
    fn set_image_auto_resize(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn block_images(&self) -> bool {
        false
    }
    fn set_block_images(&self, _blocked: bool) -> Result<()> {
        Ok(())
    }
    fn image_model(&self) -> Option<String> {
        None
    }
    fn enable_skill_commands(&self) -> bool {
        true
    }
    fn set_enable_skill_commands(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn enable_builtin_skills(&self) -> bool {
        true
    }
    fn set_enable_builtin_skills(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn show_hardware_cursor(&self) -> bool {
        false
    }
    fn set_show_hardware_cursor(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn editor_padding_x(&self) -> u64 {
        0
    }
    fn set_editor_padding_x(&self, _padding: u64) -> Result<()> {
        Ok(())
    }
    fn autocomplete_max_visible(&self) -> u64 {
        5
    }
    fn set_autocomplete_max_visible(&self, _max: u64) -> Result<()> {
        Ok(())
    }
    fn quiet_startup(&self) -> bool {
        false
    }
    fn set_quiet_startup(&self, _quiet: bool) -> Result<()> {
        Ok(())
    }
    fn idle_eviction_minutes(&self) -> String {
        "90".to_string()
    }
    fn set_idle_eviction_minutes(&self, _value: &str) -> Result<()> {
        Ok(())
    }
    fn mermaid_rendering_mode(&self) -> String {
        "streaming".to_string()
    }
    fn set_mermaid_rendering_mode(&self, _mode: &str) -> Result<()> {
        Ok(())
    }
    fn tree_filter_mode(&self) -> String {
        "user-only".to_string()
    }
    fn set_tree_filter_mode(&self, _mode: &str) -> Result<()> {
        Ok(())
    }
    fn default_service_tier(&self) -> String {
        "default".to_string()
    }
    fn set_default_service_tier(&self, _tier: &str) -> Result<()> {
        Ok(())
    }
    fn chat_detail(&self) -> String {
        "details".to_string()
    }
    fn set_chat_detail(&self, _detail: &str) -> Result<()> {
        Ok(())
    }
    fn factory_enabled(&self) -> bool {
        *self.factory_enabled.lock().expect("factory gate lock")
    }
    fn set_factory_enabled(&self, enabled: bool) -> Result<()> {
        *self.factory_enabled.lock().expect("factory gate lock") = enabled;
        Ok(())
    }
    fn telemetry_status(&self) -> String {
        "telemetry enabled".to_string()
    }
    fn set_telemetry_enabled(&self, _enabled: bool) -> Result<String> {
        Ok("telemetry enabled".to_string())
    }
    fn warnings_anthropic_extra_usage(&self) -> bool {
        true
    }
    fn set_warnings_anthropic_extra_usage(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
}

fn factory_setting() -> [HeadlessStep; 3] {
    [
        HeadlessStep::Submit("/settings".into()),
        HeadlessStep::Key(KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE)),
        HeadlessStep::Type("Factory".into()),
    ]
}

fn factory_round_trip() -> Vec<HeadlessStep> {
    factory_setting()
        .into_iter()
        .chain([
            HeadlessStep::Key(enter()),
            HeadlessStep::Key(enter()),
            HeadlessStep::Key(escape()),
        ])
        .collect()
}

/// The mock daemon's advertised capabilities and `factory_activity` graph reply.
fn run_plan_config(
    server_capabilities: Vec<String>,
    factory_graph: Option<Value>,
    steps: Vec<HeadlessStep>,
) -> (Vec<String>, bool) {
    run_plan_config_with_graph_error(server_capabilities, factory_graph, None, steps)
}

/// The full mock configuration: the graph action can also answer a daemon
/// refusal (the kernel-not-running wire shape) instead of a runs reply.
fn run_plan_config_with_graph_error(
    server_capabilities: Vec<String>,
    factory_graph: Option<Value>,
    factory_graph_error: Option<String>,
    steps: Vec<HeadlessStep>,
) -> (Vec<String>, bool) {
    let settings = std::sync::Arc::new(RecordingSettings::default());
    let settings_for_ui = settings.clone();
    let outcome = run_headless(
        HeadlessPlan {
            steps,
            width: 100,
            height: 30,
        },
        move |socket| InteractiveOptions {
            client_settings: Some(settings_for_ui),
            ..support::options(socket)
        },
        |socket| {
            let supervisor = MockSupervisor::bind_with(
                socket,
                server_capabilities,
                factory_graph,
                factory_graph_error,
            );
            move || supervisor.serve()
        },
    );
    let enabled = *settings.factory_enabled.lock().expect("factory gate lock");
    (outcome.frames, enabled)
}

/// The frames wrap notes to the frame width, so an assertion reads the
/// rendered text with the wraps collapsed.
fn flat_text(frames: &[String]) -> String {
    frames
        .join("\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn factory_defaults_to_disabled_without_a_factory_group() {
    let (frames, enabled) = run_plan_config(Vec::new(), None, factory_setting().into());
    let all = flat_text(&frames);
    assert!(all.contains("Enable factory orchestration for agents"), "{all}");
    assert!(!enabled, "the factory is opt-in");
    assert!(
        !frames.join("\n").contains('⚙'),
        "the lane is unadvertised:\n{all}"
    );
}

/// The settings toggle writes the gate in both directions.
#[test]
fn factory_on_and_off_round_trip_the_gate_through_the_seam() {
    let (frames, enabled) = run_plan_config(Vec::new(), None, factory_round_trip());
    let all = flat_text(&frames);
    assert!(all.contains("The factory is enabled."), "{all}");
    assert!(all.contains("The factory is disabled."), "{all}");
    assert!(!enabled, "the disable write persisted");
}

/// One factory run row in the wire shape the kernel's graph lane sends
/// (camelCase): the state a `running` row counts as live, a `done` row
/// with `running` children in flight counts (the resident lifecycle —
/// the dock's own liveness rule), and a terminal `done` row with none
/// does not.
fn factory_run_row(id: &str, state: &str, running_children: u64) -> Value {
    json!({
        "runId": id,
        "specId": "sw",
        "state": state,
        "elapsedMs": 100,
        "machine": { "states": [], "transitions": [] },
        "nodes": [],
        "usage": { "running": running_children },
    })
}

/// Disabling the Factory setting refuses while the session's kernel reports live runs
/// (the lifecycle guard, end to end through the daemon lane): the
/// refusal names the live count, and the gate stays enabled — the runs
/// keep their stop and visibility path (the page, `rlm.factory.stop`)
/// until the user stops them. The live count reads the dock's own
/// liveness rule: a `running` run and a `done` run with children still
/// in flight both count; a fully terminal run never does.
#[test]
fn factory_off_refuses_while_runs_are_live() {
    let graph = json!({
        "runs": [
            factory_run_row("terminal", "done", 0),
            factory_run_row("resident", "done", 2),
            factory_run_row("live", "running", 1),
        ]
    });
    let (frames, enabled) = run_plan_config(
        vec!["factory_activity".to_string()],
        Some(graph),
        factory_round_trip(),
    );
    let all = flat_text(&frames);
    assert!(
        all.contains("Cannot disable the factory while 2 runs are still live"),
        "the refusal names the live count:\n{all}"
    );
    assert!(enabled, "the gate stayed enabled after the refused off");
    assert!(!all.contains("The factory is disabled."), "{all}");
}

/// The complement: once no run is live (a fully terminal history), the
/// off write proceeds — the guard refuses only while the kernel reports
/// runs that still need their stop path.
#[test]
fn factory_off_proceeds_once_no_runs_are_live() {
    let graph = json!({
        "runs": [factory_run_row("terminal", "done", 0)]
    });
    let (frames, enabled) = run_plan_config(
        vec!["factory_activity".to_string()],
        Some(graph),
        factory_round_trip(),
    );
    let all = flat_text(&frames);
    assert!(all.contains("The factory is disabled."), "{all}");
    assert!(!enabled, "terminal history does not block the disable write");
}

/// The same refusal on a client whose hello predates the gate: a client
/// started while the factory was off keeps its unadvertised hello (the
/// running connection never re-reads the advertisement), but enabling
/// Factory opens the kernel gate immediately. The off guard must read
/// the lane even without the advertisement and refuse while runs live.
#[test]
fn factory_off_refuses_on_a_client_whose_hello_predates_the_gate() {
    let graph = json!({
        "runs": [factory_run_row("live", "running", 1)]
    });
    // The hello advertises nothing; the daemon still answers the lane.
    let (frames, enabled) = run_plan_config(Vec::new(), Some(graph), factory_round_trip());
    let all = flat_text(&frames);
    assert!(
        all.contains("Cannot disable the factory while 1 run is still live"),
        "the unadvertised-lane guard still refused:\n{all}"
    );
    assert!(enabled, "the gate stayed enabled after the refused off");
}

/// The None path never blocks the client whose hello predates the lane
/// (the fail-open half of the unreadable-count rule): a daemon that
/// cannot report runs — an older daemon answers the unknown command with
/// a failure, a session without a kernel refuses — carries no live runs,
/// so the write proceeds; only the lane-advertised client fails closed
/// (the pin below).
#[test]
fn factory_off_proceeds_when_the_lane_cannot_report() {
    // The mock answers every factory action with empty data: no runs list.
    let (frames, enabled) = run_plan_config(Vec::new(), None, factory_round_trip());
    let all = flat_text(&frames);
    assert!(all.contains("The factory is disabled."), "{all}");
    assert!(!enabled, "the unreadable unadvertised lane never blocked off");
}

/// The advertised lane fails closed on an unreadable count: a timed-out
/// or malformed graph reply cannot prove zero live runs, so the off write
/// refuses (an unknown liveness never opens the gate) and names the
/// unreadable count; the gate stays enabled for the retry.
#[test]
fn factory_off_refuses_when_the_advertised_lane_cannot_count() {
    // The lane is advertised; the mock answers every factory action with
    // empty data (a malformed reply the guard reads as an unreadable count).
    let (frames, enabled) = run_plan_config(
        vec!["factory_activity".to_string()],
        None,
        factory_round_trip(),
    );
    let all = flat_text(&frames);
    assert!(
        all.contains("Cannot verify live factory runs; factory remains enabled."),
        "the unreadable count refused the off:\n{all}"
    );
    assert!(enabled, "the gate stayed enabled after the refused off");
}

/// The kernel-not-running class never fails closed: the lane never
/// builds a kernel, and the kernel owns its run registry in memory, so a
/// session without a kernel cannot host live runs — the count reads as a
/// definitive zero even on the lane-advertised client (the refusal's
/// "try again once it answers" would never resolve otherwise: the lane
/// answers the same refusal until some other action boots the kernel,
/// so the advertised client could never disable an idle factory).
#[test]
fn factory_off_proceeds_when_the_kernel_is_not_running() {
    // The lane is advertised; the mock answers the graph read with the
    // worker arm's exact kernel-not-running refusal.
    let (frames, enabled) = run_plan_config_with_graph_error(
        vec!["factory_activity".to_string()],
        None,
        Some("Kernel is not running".to_string()),
        factory_round_trip(),
    );
    let all = flat_text(&frames);
    assert!(all.contains("The factory is disabled."), "{all}");
    assert!(!enabled, "no kernel means no live runs to block off");
}

/// One live named run row for the page battery (the wire shape the
/// kernel's graph lane sends): the name the panel header paints.
fn factory_page_run(id: &str, name: &str) -> Value {
    json!({
        "runId": id,
        "specId": "sw",
        "name": name,
        "state": "running",
        "elapsedMs": 100,
        "machine": { "states": [], "transitions": [] },
        "nodes": [],
        "usage": { "running": 0 },
    })
}

/// One alt+a key event (the dock's focus hand-off).
fn alt_a() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT)
}

/// One down-arrow key event (`tui.select.down`: the dock's next row, the page's next row).
fn down() -> KeyEvent {
    KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
}

/// One plain Enter key event (the dock's focused-section open).
fn enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
}

/// One ctrl+x key event (`app.agents.delete`: the page's two-press stop).
fn ctrl_x() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)
}

/// One Esc key event (`tui.select.cancel`: the open page closes).
fn escape() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
}

/// The page's picker keys repaint on the key itself, never waiting for
/// the 2-second refresh poll (the input loop's render contract: every
/// dispatched input paints — the same TS `handleInput` parity that
/// carries the dock's keys and every other picker key). The pin closes
/// the page on the input right after the arrow, so no poll window can
/// sit between the move and the paint: the moved selection marker must
/// land in a captured frame on the arrow's own repaint — and the poll
/// alone could never produce it, because a fold preserves the selected
/// run id, so a picker key that skipped its repaint would leave the
/// marker on the old run forever (the fold repaints the same unmoved
/// selection).
#[test]
fn factory_page_picker_keys_paint_the_moved_selection_on_the_key() {
    // Oldest first on the wire; the page reads newest-first, so
    // second-run is the opening selection and first-run is one Down away.
    let graph = json!({
        "runs": [
            factory_page_run("run-old", "first-run"),
            factory_page_run("run-new", "second-run"),
        ]
    });
    let steps = vec![
        // Focus the dock and step down to the factory row (subagents ->
        // heartbeats -> shells -> factory rows); Enter opens the page.
        HeadlessStep::Key(alt_a()),
        HeadlessStep::Key(down()),
        HeadlessStep::Key(down()),
        HeadlessStep::Key(down()),
        HeadlessStep::Key(enter()),
        // The page opens on the feed's live head: the newest run.
        HeadlessStep::WaitRender {
            needle: "factory: second-run".to_string(),
            timeout_ms: 5_000,
        },
        // Down moves the selection to the older run, and Esc closes the
        // page on the very next input — no poll cycle can land between
        // the two, so the moved marker below can only be the arrow's own
        // repaint.
        HeadlessStep::Key(down()),
        HeadlessStep::Key(escape()),
        HeadlessStep::WaitMs(200),
    ];
    let (frames, _) = run_plan_config(vec!["factory_activity".to_string()], Some(graph), steps);
    let all = flat_text(&frames);
    assert!(
        all.contains("factory: second-run"),
        "the page mounted on the newest run:\n{all}"
    );
    assert!(
        all.contains("\u{25b8} factory: first-run"),
        "the arrow's own repaint painted the moved selection before the page closed:\n{all}"
    );
}

/// A paste while the factory page owns the frame never lands in the editor hidden behind it: the
/// page has no text input, so it consumes the paste like the other input-less frame owners.
#[test]
fn a_paste_on_the_factory_page_stays_out_of_the_hidden_editor() {
    let graph = json!({ "runs": [factory_page_run("run-1", "only-run")] });
    let steps = vec![
        HeadlessStep::Key(alt_a()),
        HeadlessStep::Key(down()),
        HeadlessStep::Key(down()),
        HeadlessStep::Key(down()),
        HeadlessStep::Key(enter()),
        HeadlessStep::WaitRender {
            needle: "factory: only-run".to_string(),
            timeout_ms: 5_000,
        },
        HeadlessStep::Paste("zq-stray-paste".to_string()),
        HeadlessStep::Key(escape()),
        HeadlessStep::WaitGone {
            needle: "factory: only-run".to_string(),
            timeout_ms: 5_000,
        },
    ];
    let (frames, _) = run_plan_config(vec!["factory_activity".to_string()], Some(graph), steps);
    let all = flat_text(&frames);
    assert!(
        all.contains("factory: only-run"),
        "the page mounted before the paste:\n{all}"
    );
    assert!(
        !all.contains("zq-stray-paste"),
        "the paste must not reach the editor behind the page:\n{all}"
    );
}

/// A session the daemon stops while its factory page is open leaves no stale run cards: the
/// close clears the dock's run cache and the mounted page with it.
#[test]
fn a_closed_session_clears_the_open_factory_page() {
    let graph = json!({ "runs": [factory_page_run("run-1", "only-run")] });
    let steps = vec![
        HeadlessStep::Key(alt_a()),
        HeadlessStep::Key(down()),
        HeadlessStep::Key(down()),
        HeadlessStep::Key(down()),
        HeadlessStep::Key(enter()),
        HeadlessStep::WaitRender {
            needle: "factory: only-run".to_string(),
            timeout_ms: 5_000,
        },
        // The first stop press arms the selected run, the second stops it; the mock's daemon
        // then closes the session.
        HeadlessStep::Key(ctrl_x()),
        HeadlessStep::Key(ctrl_x()),
        HeadlessStep::WaitRender {
            needle: "No live factory runs.".to_string(),
            timeout_ms: 5_000,
        },
        HeadlessStep::WaitMs(200),
    ];
    let (frames, _) = run_plan_config(vec!["factory_activity".to_string()], Some(graph), steps);
    let last = flat_text(&frames[frames.len() - 1..]);
    assert!(
        last.contains("No live factory runs.") && !last.contains("only-run"),
        "the closed session's page drops its run cards:\n{last}"
    );
}
