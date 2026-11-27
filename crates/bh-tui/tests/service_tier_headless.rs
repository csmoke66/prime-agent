//! Headless coverage for the session tier and Fast mode rows in `/settings`.
#![cfg(unix)]
#![warn(clippy::allow_attributes_without_reason)]

mod support;

use std::os::unix::net::UnixListener;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use bh_tui::interactive::{HeadlessPlan, HeadlessStep, InteractiveOptions};
use serde_json::{json, Value};

use support::{
    attach_response, create_response, detach_response, options, run_headless, serve_connection,
    write_json, Request,
};

struct MockSupervisor {
    listener: UnixListener,
}

/// The `OpenRouter` catalog entry as the daemon's `get_model_catalog` answer carries it:
/// completions API, so flex and priority are both eligible. The same json seeds
/// `InteractiveOptions::model_catalog` (the composition-root snapshot the picker serves before the
/// fetch lands).
fn openrouter_model_value() -> Value {
    json!({
        "id": "openai/gpt-5.5",
        "name": "OpenAI: GPT-5.5",
        "api": "openai-completions",
        "provider": "openrouter",
        "baseUrl": "https://openrouter.ai/api/v1",
        "reasoning": true,
        "input": ["text"],
        "cost": { "input": 5, "output": 30, "cacheRead": 0.5, "cacheWrite": 0 },
        "contextWindow": 272_000,
        "maxTokens": 128_000
    })
}

impl MockSupervisor {
    fn bind(socket: &std::path::Path) -> Self {
        MockSupervisor {
            listener: UnixListener::bind(socket).expect("bind mock socket"),
        }
    }

    /// The worker applies a tier and reports the applied value through connection state.
    fn serve(self) {
        let (stream, _) = self.listener.accept().expect("accept");
        let mut tier = "default".to_string();
        serve_connection(stream, &[], |writer, Request { id, kind, command }| {
            match kind {
                "create" => {
                    write_json(writer, &create_response(id));
                }
                "attach" => {
                    write_json(writer, &attach_data(id));
                }
                "set_service_tier" => {
                    tier = command["serviceTier"].as_str().expect("tier").to_owned();
                    write_json(
                        writer,
                        &json!({
                            "type": "response",
                            "id": id,
                            "command": "set_service_tier",
                            "success": true,
                            "data": {},
                        }),
                    );
                }
                "get_model_catalog" => {
                    // The startup fetch (`spawn_model_catalog_refresh`) replaces the
                    // composition-root catalog; serve the same OpenRouter entry the eligibility
                    // reads run against.
                    write_json(
                        writer,
                        &json!({
                            "type": "response",
                            "id": id,
                            "command": "get_model_catalog",
                            "success": true,
                            "data": {
                                "models": [openrouter_model_value()],
                                "configuredProviders": ["openrouter"],
                            },
                        }),
                    );
                }
                "get_connection_state" => {
                    write_json(
                        writer,
                        &json!({
                            "type": "response",
                            "id": id,
                            "command": "get_connection_state",
                            "success": true,
                            "data": {
                                "activeSessionId": "s1",
                                "sessionId": "sess-1",
                                "serviceTier": tier,
                                "steeringMode": "one-at-a-time",
                                "model": { "id": "openai/gpt-5.5", "provider": "openrouter" },
                                "isStreaming": false,
                            },
                        }),
                    );
                }
                "detach" => {
                    write_json(writer, &detach_response(id));
                }
                _ => return false,
            }
            true
        });
    }
}

/// The slim attach result: one session on an `OpenRouter` completions model (the catalog entry the
/// eligibility predicate reads).
fn attach_data(id: &str) -> Value {
    let mut response = attach_response(id, "tier session");
    response["data"]["snapshot"]["state"]["model"] =
        json!({ "id": "openai/gpt-5.5", "provider": "openrouter" });
    response
}

/// The composition-root catalog seed (the options snapshot).
fn openrouter_catalog() -> Vec<bh_types::ai::Model> {
    serde_json::from_value(openrouter_model_value())
        .map(|model| vec![model])
        .expect("catalog model")
}

fn run_plan(steps: Vec<HeadlessStep>) -> Vec<String> {
    let outcome = run_headless(
        HeadlessPlan {
            steps,
            width: 100,
            height: 30,
        },
        |socket| InteractiveOptions {
            model_catalog: openrouter_catalog(),
            ..options(socket)
        },
        |socket| {
            let supervisor = MockSupervisor::bind(socket);
            move || supervisor.serve()
        },
    );
    outcome.frames
}

#[test]
fn tier_picker_only_offers_supported_tiers_and_applies_the_choice() {
    let frames = run_plan(vec![
        HeadlessStep::Submit("/settings".into()),
        wait_for("Search settings"),
        key(KeyCode::Char('2')),
        HeadlessStep::Paste("session".into()),
        key(KeyCode::Enter),
        wait_for("Session Service Tier"),
        key(KeyCode::Down),
        key(KeyCode::Enter),
        key(KeyCode::Esc),
        wait_for("openai/gpt-5.5 \u{00b7} flex"),
    ]);
    let picker = frames.iter().find(|frame| frame.contains("Session Service Tier")).unwrap();
    assert!(picker.contains("\u{203a} default"), "{picker}");
    assert!(picker.contains("flex") && picker.contains("priority"), "{picker}");
    assert!(!picker.contains("auto") && !picker.contains("scale"), "{picker}");
    let all = frames.join("\n");
    assert!(all.contains("Service tier: flex"), "{all}");
}

#[test]
fn fast_mode_toggles_priority_and_back_to_default() {
    let frames = run_plan(vec![
        HeadlessStep::Submit("/settings".into()),
        wait_for("Search settings"),
        key(KeyCode::Char('2')),
        HeadlessStep::Paste("fast".into()),
        key(KeyCode::Enter),
        HeadlessStep::WaitMs(200),
        key(KeyCode::Enter),
        key(KeyCode::Esc),
        wait_for("Fast mode: off"),
    ]);
    assert!(frames.iter().any(|frame| {
        frame.lines().any(|line| line.contains("Fast mode") && line.trim_end().ends_with("on"))
    }));
    let last = frames.last().unwrap();
    assert!(last.contains("Fast mode: off") && !last.contains("\u{00b7} priority"), "{last}");
}

fn key(code: KeyCode) -> HeadlessStep {
    HeadlessStep::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn wait_for(needle: &str) -> HeadlessStep {
    HeadlessStep::WaitRender { needle: needle.into(), timeout_ms: 5_000 }
}
