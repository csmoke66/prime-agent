//! Headless regression for the client-only `/cost` display checkpoint.
#![cfg(unix)]
#![warn(clippy::allow_attributes_without_reason)]

mod support;

use std::process::Command;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use pa_tui::interactive::{HeadlessPlan, HeadlessStep};
use serde_json::{json, Value};

use support::{attach_response, create_response, options, run_headless, serve_connection, write_json, MockSupervisor, Request};

const CHILD_ENV: &str = "PA_COST_COMMAND_HEADLESS_CHILD";

fn roster() -> Value {
    json!({
        "roster": [
            {
                "agentId": "s1",
                "status": "idle",
                "summary": {
                    "sessionId": "sess-1",
                    "activeSessionId": "s1",
                    "sessionFile": "/tmp/sess-1.jsonl",
                    "lifecycle": "live",
                    "runtimeKind": "top-level",
                    "usage": {
                        "cost": 0.12,
                        "costBreakdown": {
                            "input": 0.03,
                            "output": 0.04,
                            "cacheRead": 0.02,
                            "cacheWrite": 0.01,
                            "total": 0.10
                        }
                    }
                }
            },
            {
                "agentId": "child-1",
                "status": "idle",
                "summary": {
                    "sessionId": "child-session",
                    "activeSessionId": "child-live",
                    "sessionFile": "/tmp/child-1.jsonl",
                    "lifecycle": "live",
                    "runtimeKind": "subagent",
                    "parentActiveSessionId": "s1",
                    "usage": {
                        "cost": 0.30,
                        "costBreakdown": {
                            "input": 0.07,
                            "output": 0.08,
                            "cacheRead": 0.09,
                            "cacheWrite": 0.06,
                            "total": 0.30
                        }
                    }
                }
            }
        ]
    })
}

impl MockSupervisor {
    fn serve_cost(self) {
        let (stream, _) = self.listener.accept().expect("accept");
        serve_connection(stream, &[], |writer, Request { id, kind, .. }| {
            match kind {
                "create" => write_json(writer, &create_response(id)),
                "attach" => write_json(writer, &attach_response(id, "cost session")),
                "roster_subscribe" => write_json(
                    writer,
                    &json!({
                        "type": "response",
                        "id": id,
                        "command": "roster_subscribe",
                        "success": true,
                        "data": roster(),
                    }),
                ),
                _ => return false,
            }
            true
        });
    }
}

fn wait_render(needle: &str) -> HeadlessStep {
    HeadlessStep::WaitRender {
        needle: needle.to_string(),
        timeout_ms: 30_000,
    }
}

/// The real smoke body runs only in the isolated child process created below, where the cost
/// baseline cannot read or overwrite the developer's data directory or another parallel test's.
#[test]
fn cost_command_child_mode() {
    if std::env::var_os(CHILD_ENV).is_none() {
        // The title consumes the same resettable rollup as the command panel.
        wait_render("$0.42"),
        return;
    }
    let steps = vec![
        HeadlessStep::Submit("/cost".to_string()),
        wait_render("Total: $0.4200"),
        wait_render("Uncached input: $0.1000"),
        wait_render("Unclassified: $0.0200"),
        HeadlessStep::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        HeadlessStep::WaitGone {
            needle: "Session Cost".to_string(),
            timeout_ms: 30_000,
        },
        HeadlessStep::Submit("/cost reset".to_string()),
        wait_render("Total: $0.0000"),
        wait_render("Cache write: $0.0000"),
    ];
    let outcome = run_headless(
        HeadlessStep::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        HeadlessStep::WaitGone {
            needle: "Session Cost".to_string(),
            timeout_ms: 30_000,
        },
        wait_render("$0.00"),
        HeadlessPlan {
            steps,
            width: 100,
            height: 30,
        },
        options,
        |socket| {
            let supervisor = MockSupervisor::bind(socket);
            move || supervisor.serve_cost()
        },
    );
    let rendered = outcome.frames.join("\n");
    assert!(rendered.contains("Output: $0.1200"), "{rendered}");
    assert!(rendered.contains("Cache read: $0.1100"), "{rendered}");
}

/// Re-execute this one-test integration binary with a unique agent directory. Environment changes
/// therefore remain process-local even when the workspace test runner executes binaries in parallel.
#[test]
fn cost_command_displays_and_resets_the_whole_session_tree() {
    let agent_dir = tempfile::TempDir::new().expect("agent dir");
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .arg("--exact")
        .arg("cost_command_child_mode")
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .env("PRIME_AGENT_CODING_AGENT_DIR", agent_dir.path())
        .output()
        .expect("run isolated cost smoke");
    assert!(
        output.status.success(),
        "isolated cost smoke failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
