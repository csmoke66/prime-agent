//! The model-resolution tests (catalog, allowlist, restore/switch/configure, the thinking clamp).
use super::*;

/// A models.json custom provider (name has no env-key mapping), with an
/// apiKey the registry must resolve for request auth (the env-key map
/// alone cannot find it).
fn write_custom_provider_models_json(agent_dir: &std::path::Path, base_url: &str) {
    std::fs::create_dir_all(agent_dir).unwrap();
    std::fs::write(
        agent_dir.join("models.json"),
        serde_json::json!({
            "providers": {
                "battery": {
                    "api": "openai-completions",
                    "baseUrl": base_url,
                    "apiKey": "sk-battery",
                    "models": [
                        {
                            "id": "mock-1",
                            "name": "Mock 1",
                            "api": "openai-completions",
                            "contextWindow": 128_000,
                            "maxTokens": 4096,
                        }
                    ]
                }
            }
        })
        .to_string(),
    )
    .unwrap();
}

/// A models.json custom-provider pair for the thinking clamp: a
/// reasoning model (up to `high`) and a non-reasoning one (`off`).
fn write_thinking_pair_models_json(agent_dir: &std::path::Path, base_url: &str) {
    std::fs::create_dir_all(agent_dir).unwrap();
    std::fs::write(
        agent_dir.join("models.json"),
        serde_json::json!({
            "providers": {
                "battery": {
                    "api": "openai-completions",
                    "baseUrl": base_url,
                    "apiKey": "sk-battery",
                    "models": [
                        {
                            "id": "mock-reason",
                            "name": "Mock Reasoning",
                            "api": "openai-completions",
                            "contextWindow": 128_000,
                            "maxTokens": 4096,
                            "reasoning": true,
                        },
                        {
                            "id": "mock-plain",
                            "name": "Mock Plain",
                            "api": "openai-completions",
                            "contextWindow": 128_000,
                            "maxTokens": 4096,
                        }
                    ]
                }
            }
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn create_config_flags_reach_the_engine_model_resolution() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_custom_provider_models_json(&agent_dir, "http://127.0.0.1:9");

    let engine = AgentSessionEngine::new(AgentEngineConfig {
        cwd: dir.path().to_path_buf(),
        agent_dir,
        // No process-level fallback: the wire flags must be the source.
        provider: None,
        model: None,
        api_key: None,
        thinking: None,
        session_dir: None,
        session_file: None,
        faux_script: None,
        supervisor_link: None,
        telemetry_disabled: None,
        cron_store: None,
        queued_steering_probe: None,
    })
    .unwrap();
    engine.configure_model(EngineModelSelection {
        provider: Some("battery".to_string()),
        model: Some("mock-1".to_string()),
        api_key: None,
        thinking: None,
    });
    let model = engine.resolve_registry_model().expect("resolved model");
    assert_eq!(model.provider, "battery");
    assert_eq!(model.id, "mock-1");
    // The registry resolves the models.json apiKey (the provider name has
    // no env-key mapping), so the engine can authenticate without env.
    assert_eq!(
        engine.resolve_request_auth(&model).api_key.as_deref(),
        Some("sk-battery")
    );
}

#[test]
fn settings_default_drives_unflagged_resolution() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_custom_provider_models_json(&agent_dir, "http://127.0.0.1:9");
    let mut settings = bh_core::settings::SettingsManager::create(dir.path(), &agent_dir);
    settings
        .set_default_model_and_provider("battery", "mock-1")
        .unwrap();
    let engine = AgentSessionEngine::new(AgentEngineConfig {
        cwd: dir.path().to_path_buf(),
        agent_dir,
        provider: None,
        model: None,
        api_key: None,
        thinking: None,
        session_dir: None,
        session_file: None,
        faux_script: None,
        supervisor_link: None,
        telemetry_disabled: None,
        cron_store: None,
        queued_steering_probe: None,
    })
    .unwrap();
    let model = engine.resolve_registry_model().expect("resolved model");
    assert_eq!(model.provider, "battery");
    assert_eq!(model.id, "mock-1");
}

/// One auth.json with a stored Anthropic key: the file auth the engine's registry reads.
fn write_anthropic_auth(agent_dir: &std::path::Path) {
    std::fs::create_dir_all(agent_dir).unwrap();
    std::fs::write(
        agent_dir.join("auth.json"),
        serde_json::json!({ "anthropic": { "type": "api_key", "key": "test-key" } }).to_string(),
    )
    .unwrap();
}

/// The provider default the startup chain lands on when only Anthropic is configured.
const STARTUP_DEFAULT: &str = "claude-opus-4-7";

/// The saved model the restore tests pin (not the startup default).
const PINNED_MODEL: &str = "claude-sonnet-5-5";

/// A session file whose last `model_change` row pins [`PINNED_MODEL`] —
/// what a revived worker reads at create.
fn session_file_pinning_saved_model(dir: &std::path::Path) -> std::path::PathBuf {
    session_file_pinning_model(dir, "anthropic", PINNED_MODEL)
}

/// A session file whose last `model_change` row pins the given model —
/// what a revived worker reads at create.
fn session_file_pinning_model(
    dir: &std::path::Path,
    provider: &str,
    model: &str,
) -> std::path::PathBuf {
    let mut session =
        crate::session_store::SessionFile::create(dir.to_str().unwrap_or("/tmp"), None, 0);
    let path = dir.join(crate::session_store::session_file_name(
        session.session_id(),
    ));
    session.set_path(path.clone());
    session.append_model_change(provider, model);
    session.rewrite().unwrap();
    path
}

fn restore_test_engine(
    dir: &std::path::Path,
    provider: Option<&str>,
    model: Option<&str>,
) -> AgentSessionEngine {
    AgentSessionEngine::new(AgentEngineConfig {
        cwd: dir.to_path_buf(),
        agent_dir: dir.join("agent"),
        provider: provider.map(str::to_string),
        model: model.map(str::to_string),
        api_key: None,
        thinking: None,
        session_dir: None,
        session_file: None,
        faux_script: None,
        supervisor_link: None,
        telemetry_disabled: Some(true),
        cron_store: None,
        queued_steering_probe: None,
    })
    .expect("engine")
}

#[test]
fn root_startup_chain_ignores_child_allowlist() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_custom_provider_models_json(&agent_dir, "http://127.0.0.1:9");
    std::fs::write(
        agent_dir.join("settings.json"),
        serde_json::json!({
            "defaultProvider": "battery", "defaultModel": "mock-1", "allowedModels": ["anthropic/*"]
        })
        .to_string(),
    )
    .unwrap();
    let engine = AgentSessionEngine::new(AgentEngineConfig {
        cwd: dir.path().to_path_buf(),
        agent_dir,
        provider: None,
        model: None,
        api_key: None,
        thinking: None,
        session_dir: None,
        session_file: None,
        faux_script: None,
        supervisor_link: None,
        telemetry_disabled: Some(true),
        cron_store: None,
        queued_steering_probe: None,
    })
    .unwrap();
    let model = engine.resolve_registry_model().expect("resolved model");
    assert_eq!(model.provider, "battery");
    assert_eq!(model.id, "mock-1");
}

/// A revived create without model flags resolves the startup default until the
/// create-time restore pins the session's saved model.
#[tokio::test]
async fn revived_session_restores_its_pinned_model_not_the_startup_default() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_anthropic_auth(&agent_dir);
    let path = session_file_pinning_saved_model(dir.path());
    let engine = restore_test_engine(dir.path(), None, None);
    engine.set_session_file(path.clone());

    let cold = engine.resolve_registry_model().expect("cold resolution");
    assert_eq!(cold.provider, "anthropic");
    assert_eq!(cold.id, STARTUP_DEFAULT);

    engine.restore_session_model(&path, None).await;
    let restored = engine
        .resolve_registry_model()
        .expect("restored resolution");
    assert_eq!(restored.provider, "anthropic");
    assert_eq!(restored.id, PINNED_MODEL);
    assert!(
        engine.model_fallback_message().is_none(),
        "a successful restore leaves no fallback message"
    );
}

#[tokio::test]
async fn revived_session_fallback_is_on_the_record() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_anthropic_auth(&agent_dir);
    let path = session_file_pinning_model(dir.path(), "anthropic", "claude-retired");
    let engine = restore_test_engine(dir.path(), None, None);
    engine.set_session_file(path.clone());

    engine.restore_session_model(&path, None).await;
    assert_eq!(
        engine.model_fallback_message().as_deref(),
        Some("Could not restore model anthropic/claude-retired. Using anthropic/claude-opus-4-7"),
        "the fallback is published, never silent"
    );
    let resolved = engine.resolve_registry_model().expect("startup chain");
    assert_eq!(resolved.provider, "anthropic");
    assert_eq!(resolved.id, STARTUP_DEFAULT);
}

#[tokio::test]
async fn create_flags_beat_the_saved_session_model() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_custom_provider_models_json(&agent_dir, "http://127.0.0.1:9");
    let path = session_file_pinning_saved_model(dir.path());
    let engine = restore_test_engine(dir.path(), Some("battery"), Some("mock-1"));
    engine.set_session_file(path.clone());

    engine.restore_session_model(&path, None).await;
    let resolved = engine.resolve_registry_model().expect("flagged resolution");
    assert_eq!(resolved.provider, "battery");
    assert_eq!(resolved.id, "mock-1");
    assert!(engine.model_fallback_message().is_none());
}

#[tokio::test]
async fn fresh_session_without_a_saved_model_keeps_the_startup_chain() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_anthropic_auth(&agent_dir);
    let mut session =
        crate::session_store::SessionFile::create(dir.path().to_str().unwrap_or("/tmp"), None, 0);
    let path = dir.path().join(crate::session_store::session_file_name(
        session.session_id(),
    ));
    session.set_path(path.clone());
    session.rewrite().unwrap();
    let engine = restore_test_engine(dir.path(), None, None);
    engine.set_session_file(path.clone());

    engine.restore_session_model(&path, None).await;
    assert!(engine.model_fallback_message().is_none());
    let resolved = engine.resolve_registry_model().expect("startup chain");
    assert_eq!(resolved.id, STARTUP_DEFAULT);
}

#[tokio::test]
async fn a_restore_decision_is_scoped_to_its_session_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_anthropic_auth(&agent_dir);
    let pinned = session_file_pinning_saved_model(dir.path());
    let engine = restore_test_engine(dir.path(), None, None);
    engine.set_session_file(pinned.clone());
    engine.restore_session_model(&pinned, None).await;
    let restored = engine
        .resolve_registry_model()
        .expect("restored resolution");
    assert_eq!(restored.id, PINNED_MODEL);

    let mut other =
        crate::session_store::SessionFile::create(dir.path().to_str().unwrap_or("/tmp"), None, 0);
    let other_path = dir
        .path()
        .join(crate::session_store::session_file_name(other.session_id()));
    other.set_path(other_path.clone());
    other.rewrite().unwrap();
    engine.set_session_file(other_path);
    let moved = engine.resolve_registry_model().expect("moved resolution");
    assert_eq!(moved.id, STARTUP_DEFAULT);
    assert!(
        engine.model_fallback_message().is_none(),
        "the old file's decision does not leak into the moved-to session"
    );
}

#[tokio::test]
async fn a_model_switch_never_leaks_into_the_replacement_session() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_anthropic_auth(&agent_dir);
    write_custom_provider_models_json(&agent_dir, "http://127.0.0.1:9");

    let file_a = session_file_pinning_saved_model(dir.path());
    let engine = std::sync::Arc::new(restore_test_engine(dir.path(), None, None));
    engine.set_session_file(file_a.clone());
    engine.restore_session_model(&file_a, None).await;
    let restored = engine.resolve_registry_model().expect("restored");
    assert_eq!(restored.id, PINNED_MODEL);

    // A mid-session /model switch on session A (the worker runs the
    // engine's synchronous switch on the blocking pool, like the turn
    // path — a tokio context must not block on its locks).
    let switched_engine = std::sync::Arc::clone(&engine);
    let switched = tokio::task::spawn_blocking(move || {
        switched_engine.switch_model(EngineModelSelection {
            provider: Some("battery".to_string()),
            model: Some("mock-1".to_string()),
            api_key: None,
            thinking: None,
        })
    })
    .await
    .expect("blocking switch");
    assert!(switched);
    let switched = engine.resolve_registry_model().expect("switched");
    assert_eq!(switched.id, "mock-1");

    let file_b = session_file_pinning_saved_model(dir.path());
    engine.set_session_file(file_b.clone());
    engine.restore_session_model(&file_b, None).await;
    let moved = engine.resolve_registry_model().expect("moved resolution");
    assert_eq!(
        moved.id, PINNED_MODEL,
        "the moved-to session's own file pin wins over the previous session's switch"
    );
    assert!(engine.model_fallback_message().is_none());
}

#[tokio::test]
async fn an_unpersisted_session_keeps_its_live_selection() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_thinking_pair_models_json(&agent_dir, "http://127.0.0.1:9");
    let engine = std::sync::Arc::new(restore_test_engine(dir.path(), None, None));
    engine.configure_create_model(EngineModelSelection {
        provider: Some("battery".to_string()),
        model: Some("mock-plain".to_string()),
        api_key: None,
        thinking: None,
    });
    let switched_engine = std::sync::Arc::clone(&engine);
    let switched = tokio::task::spawn_blocking(move || {
        switched_engine.switch_model(EngineModelSelection {
            provider: Some("battery".to_string()),
            model: Some("mock-reason".to_string()),
            api_key: None,
            thinking: None,
        })
    })
    .await
    .expect("blocking switch");
    assert!(switched);

    engine
        .restore_session_model(std::path::Path::new(""), None)
        .await;
    let resolved = engine.resolve_registry_model().expect("live selection");
    assert_eq!(
        (resolved.provider.as_str(), resolved.id.as_str()),
        ("battery", "mock-reason"),
        "the live selection survives an unpersisted replacement"
    );
}

#[tokio::test]
async fn a_replacement_restores_the_moved_to_sessions_saved_thinking_level() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_thinking_pair_models_json(&agent_dir, "http://127.0.0.1:9");
    let engine = restore_test_engine(dir.path(), None, None);

    let mut file_a =
        crate::session_store::SessionFile::create(dir.path().to_str().unwrap_or("/tmp"), None, 0);
    let path_a = dir
        .path()
        .join(crate::session_store::session_file_name(file_a.session_id()));
    file_a.set_path(path_a.clone());
    file_a.append_model_change("battery", "mock-reason");
    file_a.append_thinking_level_change("low");
    file_a.rewrite().unwrap();
    engine.set_session_file(path_a.clone());
    engine.restore_session_model(&path_a, None).await;
    assert_eq!(
        engine.effective_thinking_level().as_deref(),
        Some("low"),
        "the moved-to session's saved thinking level restores, not the medium default"
    );

    let mut file_b =
        crate::session_store::SessionFile::create(dir.path().to_str().unwrap_or("/tmp"), None, 0);
    let path_b = dir
        .path()
        .join(crate::session_store::session_file_name(file_b.session_id()));
    file_b.set_path(path_b.clone());
    file_b.append_model_change("battery", "mock-plain");
    file_b.append_thinking_level_change("high");
    file_b.rewrite().unwrap();
    engine.set_session_file(path_b.clone());
    engine.restore_session_model(&path_b, None).await;
    assert_eq!(
        engine.effective_thinking_level().as_deref(),
        Some("off"),
        "the saved level re-clamps against the restored non-reasoning model"
    );
}

#[tokio::test]
async fn a_compacted_session_restores_its_post_compaction_model() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_thinking_pair_models_json(&agent_dir, "http://127.0.0.1:9");
    let mut session =
        crate::session_store::SessionFile::create(dir.path().to_str().unwrap_or("/tmp"), None, 0);
    let path = dir.path().join(crate::session_store::session_file_name(
        session.session_id(),
    ));
    session.set_path(path.clone());
    session.append_model_change("battery", "mock-reason");
    let kept = session.append_message(&serde_json::json!({
        "role": "assistant",
        "provider": "battery",
        "model": "mock-plain",
        "api": "openai-responses",
        "content": [],
        "stopReason": "stop",
        "timestamp": 0u64,
        "usage": {
            "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0,
            "cost": { "input": 0.0, "output": 0.0, "cacheRead": 0.0, "cacheWrite": 0.0, "total": 0.0 }
        }
    }));
    session.append_entry(
        "compaction",
        serde_json::json!({
            "summary": "summary",
            "firstKeptEntryId": kept,
            "tokensBefore": 100
        }),
    );
    session.rewrite().unwrap();

    let engine = restore_test_engine(dir.path(), None, None);
    engine.set_session_file(path.clone());
    engine.restore_session_model(&path, None).await;
    let restored = engine
        .resolve_registry_model()
        .expect("restored resolution");
    assert_eq!(
        (restored.provider.as_str(), restored.id.as_str()),
        ("battery", "mock-plain"),
        "the post-compaction assistant message pins the restored model, not the superseded model_change"
    );
    assert!(engine.model_fallback_message().is_none());
}

#[tokio::test]
async fn create_flags_survive_a_session_replacement() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_thinking_pair_models_json(&agent_dir, "http://127.0.0.1:9");
    let engine = std::sync::Arc::new(restore_test_engine(dir.path(), None, None));
    engine.configure_create_model(EngineModelSelection {
        provider: Some("battery".to_string()),
        model: Some("mock-plain".to_string()),
        api_key: None,
        thinking: None,
    });

    let switched_engine = std::sync::Arc::clone(&engine);
    let switched = tokio::task::spawn_blocking(move || {
        switched_engine.switch_model(EngineModelSelection {
            provider: Some("battery".to_string()),
            model: Some("mock-reason".to_string()),
            api_key: None,
            thinking: None,
        })
    })
    .await
    .expect("blocking switch");
    assert!(switched);
    let switched = engine.resolve_registry_model().expect("switched");
    assert_eq!(switched.id, "mock-reason");

    let moved = session_file_pinning_model(dir.path(), "battery", "mock-reason");
    engine.set_session_file(moved.clone());
    engine.restore_session_model(&moved, None).await;
    let resolved = engine.resolve_registry_model().expect("flagged resolution");
    assert_eq!(
        (resolved.provider.as_str(), resolved.id.as_str()),
        ("battery", "mock-plain"),
        "the create command's flags survive the replacement"
    );
    assert!(
        engine.model_fallback_message().is_none(),
        "a flagged restore never records a fallback"
    );
}

#[tokio::test]
async fn a_replacement_re_clamps_the_thinking_level_against_the_restored_model() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_thinking_pair_models_json(&agent_dir, "http://127.0.0.1:9");
    let engine = restore_test_engine(dir.path(), None, None);
    engine.configure_create_model(EngineModelSelection {
        provider: None,
        model: None,
        api_key: None,
        thinking: Some(bh_types::ai::ModelThinkingLevel::High),
    });

    let plain = session_file_pinning_model(dir.path(), "battery", "mock-plain");
    engine.set_session_file(plain.clone());
    engine.restore_session_model(&plain, None).await;
    assert_eq!(
        engine.effective_thinking_level().as_deref(),
        Some("off"),
        "the clamp follows the restored non-reasoning model, not the reset selection"
    );

    let reason = session_file_pinning_model(dir.path(), "battery", "mock-reason");
    engine.set_session_file(reason.clone());
    engine.restore_session_model(&reason, None).await;
    assert_eq!(
        engine.effective_thinking_level().as_deref(),
        Some("high"),
        "the replacement re-clamps the requested level against its restored model"
    );
}

#[test]
fn root_switch_accepts_a_model_outside_the_child_allowlist() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_thinking_pair_models_json(&agent_dir, "http://127.0.0.1:9");
    std::fs::write(
        agent_dir.join("settings.json"),
        serde_json::json!({ "allowedModels": ["battery/mock-plain"] }).to_string(),
    )
    .unwrap();
    let engine = AgentSessionEngine::new(AgentEngineConfig {
        cwd: dir.path().to_path_buf(),
        agent_dir,
        provider: Some("battery".into()),
        model: Some("mock-plain".into()),
        api_key: None,
        thinking: None,
        session_dir: None,
        session_file: None,
        faux_script: None,
        supervisor_link: None,
        telemetry_disabled: Some(true),
        cron_store: None,
        queued_steering_probe: None,
    })
    .unwrap();
    let model = engine.resolve_registry_model().expect("resolved model");
    assert_eq!(model.id, "mock-plain");
    let switched = engine.switch_model(EngineModelSelection {
        provider: Some("battery".to_string()),
        model: Some("mock-reason".to_string()),
        api_key: None,
        thinking: None,
    });
    assert!(switched);
    let model = engine.resolve_registry_model().expect("resolved model");
    assert_eq!(model.id, "mock-reason");
}

#[test]
fn switch_model_propagates_the_new_model_to_the_child_identity() {
    let dir = tempfile::tempdir().unwrap();
    let agent_dir = dir.path().join("agent");
    write_custom_provider_models_json(&agent_dir, "http://127.0.0.1:9");
    let engine = AgentSessionEngine::new(AgentEngineConfig {
        cwd: dir.path().to_path_buf(),
        agent_dir,
        provider: None,
        model: None,
        api_key: None,
        thinking: None,
        session_dir: None,
        session_file: None,
        faux_script: None,
        supervisor_link: Some(SupervisorLinkConfig {
            socket_path: dir.path().join("absent-supervisor.sock"),
            active_session_id: "parent-live".to_string(),
            worker_token: "test-token".to_string(),
        }),
        telemetry_disabled: Some(true),
        cron_store: None,
        queued_steering_probe: None,
    })
    .unwrap();
    let children = engine
        .children
        .as_ref()
        .expect("the supervisor link wires the children registry")
        .clone();
    children.set_model("battery/mock-2".to_string());
    let switched = engine.switch_model(EngineModelSelection {
        provider: Some("battery".to_string()),
        model: Some("mock-1".to_string()),
        api_key: None,
        thinking: None,
    });
    assert!(switched, "the switch proceeds without an allowlist");
    assert_eq!(
        children.parent_model().as_deref(),
        Some("battery/mock-1"),
        "an inherited spawn must resolve the switched-to model, not the stale build-time selector"
    );
}

#[test]
fn configure_model_merges_only_present_fields() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_custom_provider_models_json(&agent_dir, "http://127.0.0.1:9");
    let engine = AgentSessionEngine::new(AgentEngineConfig {
        cwd: dir.path().to_path_buf(),
        agent_dir,
        provider: Some("battery".to_string()),
        model: Some("mock-1".to_string()),
        api_key: Some("flag-key".to_string()),
        thinking: None,
        session_dir: None,
        session_file: None,
        faux_script: None,
        supervisor_link: None,
        telemetry_disabled: None,
        cron_store: None,
        queued_steering_probe: None,
    })
    .unwrap();
    engine.configure_model(EngineModelSelection {
        provider: None,
        model: Some("mock-1".to_string()),
        api_key: None,
        thinking: None,
    });
    let model = engine.resolve_registry_model().expect("resolved model");
    assert_eq!(model.provider, "battery");
    assert_eq!(
        engine.resolve_request_auth(&model).api_key.as_deref(),
        Some("flag-key")
    );
}

#[test]
fn agent_engine_reports_model_resolution_failures() {
    let dir = tempfile::TempDir::new().unwrap();
    // One auth-configured model keeps the available list non-empty in
    // every environment (a clean env with no credentials resolves to
    // "No models available" before the flagged-provider error, while a
    // machine with ambient env credentials reaches this test's branch).
    std::fs::create_dir_all(dir.path().join("agent")).unwrap();
    std::fs::write(
        dir.path().join("agent").join("models.json"),
        serde_json::json!({
            "providers": {
                "battery": {
                    "api": "openai-completions",
                    "baseUrl": "http://127.0.0.1:9",
                    "apiKey": "sk-battery",
                    "models": [
                        { "id": "mock-1", "contextWindow": 128_000, "maxTokens": 4096 }
                    ]
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let engine = AgentSessionEngine::new(AgentEngineConfig {
        cwd: dir.path().to_path_buf(),
        agent_dir: dir.path().join("agent"),
        provider: Some("no-such-provider".to_string()),
        model: Some("some-model".to_string()),
        api_key: None,
        thinking: None,
        session_dir: None,
        session_file: None,
        faux_script: None,
        supervisor_link: None,
        telemetry_disabled: None,
        cron_store: None,
        queued_steering_probe: None,
    })
    .unwrap();
    let mut events: Vec<EngineEvent> = Vec::new();
    engine.run_prompt(
        0,
        PromptRequest {
            batch: Vec::new(),
            images: Vec::new(),
            message: "hi".to_string(),
            source: "user".to_string(),
            agent_message_id: None,
            custom_message: None,
            streaming_behavior: None,
        },
        &|| false,
        &mut |event| {
            events.push(event);
            true
        },
    );
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], EngineEvent::UserMessage(_)));
    let EngineEvent::Done(Err(error)) = &events[1] else {
        panic!("expected error done");
    };
    assert!(error.contains("Unknown provider"));
}

#[test]
fn configure_model_thinking_clamps_to_the_models_supported_levels() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    std::fs::write(
        agent_dir.join("models.json"),
        serde_json::json!({
            "providers": {
                "battery": {
                    "api": "openai-completions",
                    "baseUrl": "http://127.0.0.1:9",
                    "apiKey": "sk-battery",
                    "models": [
                        {
                            "id": "mock-1",
                            "reasoning": true,
                            "contextWindow": 128_000,
                            "maxTokens": 4096
                        }
                    ]
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let engine = AgentSessionEngine::new(AgentEngineConfig {
        cwd: dir.path().to_path_buf(),
        agent_dir,
        provider: Some("battery".to_string()),
        model: Some("mock-1".to_string()),
        api_key: None,
        thinking: None,
        session_dir: None,
        session_file: None,
        faux_script: None,
        supervisor_link: None,
        telemetry_disabled: None,
        cron_store: None,
        queued_steering_probe: None,
    })
    .unwrap();
    // Without an explicit flag the TS default applies (medium, clamped).
    assert_eq!(engine.effective_thinking_level().as_deref(), Some("medium"));
    // The create-config flag is authoritative, clamped to model support.
    engine.configure_model(EngineModelSelection {
        provider: None,
        model: None,
        api_key: None,
        thinking: Some(bh_types::ai::ModelThinkingLevel::Max),
    });
    assert_eq!(engine.effective_thinking_level().as_deref(), Some("high"));
    engine.configure_model(EngineModelSelection {
        provider: None,
        model: None,
        api_key: None,
        thinking: Some(bh_types::ai::ModelThinkingLevel::Low),
    });
    assert_eq!(engine.effective_thinking_level().as_deref(), Some("low"));
}

#[tokio::test]
async fn a_pre_read_saved_context_restores_like_the_file_read() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_thinking_pair_models_json(&agent_dir, "http://127.0.0.1:9");

    let mut session =
        crate::session_store::SessionFile::create(dir.path().to_str().unwrap_or("/tmp"), None, 0);
    let path = dir.path().join(crate::session_store::session_file_name(
        session.session_id(),
    ));
    session.set_path(path.clone());
    session.append_model_change("battery", "mock-reason");
    session.append_thinking_level_change("low");
    session.rewrite().unwrap();

    // Engine A: the file-read path (no pre-read context).
    let engine_a = restore_test_engine(dir.path(), None, None);
    engine_a.set_session_file(path.clone());
    engine_a.restore_session_model(&path, None).await;

    // Engine B: the pre-read context off the create's own windowed open
    // (the exact shape the resume create passes).
    let store = crate::session_store::SessionFile::open_windowed(&path).unwrap();
    let saved = super::super::model::saved_session_context_from_parts(
        &store.restored_settings(),
        store.has_thinking_level(),
    );
    let engine_b = restore_test_engine(dir.path(), None, None);
    engine_b.set_session_file(path.clone());
    engine_b.restore_session_model(&path, Some(saved)).await;

    let resolved_a = engine_a
        .resolve_registry_model()
        .expect("file-read resolution");
    let resolved_b = engine_b
        .resolve_registry_model()
        .expect("pre-read resolution");
    assert_eq!(
        (resolved_a.provider, resolved_a.id),
        (resolved_b.provider, resolved_b.id),
        "the pre-read restore pins the same model as the file read"
    );
    assert_eq!(
        engine_a.effective_thinking_level(),
        engine_b.effective_thinking_level(),
        "the pre-read restore adopts the same thinking level"
    );
    assert_eq!(
        engine_a.model_fallback_message(),
        engine_b.model_fallback_message(),
        "the pre-read restore records the same fallback decision"
    );
    assert_eq!(
        engine_b.effective_thinking_level().as_deref(),
        Some("low"),
        "the pre-read path restores the saved level, not the default"
    );
}

#[test]
fn a_model_switch_keeps_the_agent_slot_in_sync_with_the_reported_level() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_thinking_pair_models_json(&agent_dir, "http://127.0.0.1:9");
    let engine = AgentSessionEngine::new(AgentEngineConfig {
        cwd: dir.path().to_path_buf(),
        agent_dir,
        provider: Some("battery".to_string()),
        model: Some("mock-reason".to_string()),
        api_key: None,
        thinking: Some(bh_types::ai::ModelThinkingLevel::High),
        session_dir: None,
        session_file: None,
        faux_script: None,
        supervisor_link: None,
        telemetry_disabled: Some(true),
        cron_store: None,
        queued_steering_probe: None,
    })
    .unwrap();
    let model = engine.resolve_model().expect("resolves mock-reason");
    engine
        .ensure_core_session(&model)
        .expect("the session builds");
    let agent_wire_level = |engine: &AgentSessionEngine| {
        let session = engine.session.blocking_lock();
        let core = session.as_deref().expect("the built session");
        let state = engine
            .runtime
            .block_on(async { core.session.agent().state().await });
        bh_core::session_engine::provider_adapter::model_thinking_level(state.thinking_level)
            .wire_name()
            .to_string()
    };
    assert_eq!(engine.effective_thinking_level().as_deref(), Some("high"));
    assert_eq!(agent_wire_level(&engine), "high");

    // The switch onto the plain model clamps the effective level to off:
    // the agent slot must follow (the request stops carrying a level the
    // switched-to model cannot serve).
    assert!(engine.switch_model(EngineModelSelection {
        provider: Some("battery".to_string()),
        model: Some("mock-plain".to_string()),
        api_key: None,
        thinking: None,
    }));
    assert_eq!(
        engine.effective_thinking_level().as_deref(),
        Some("off"),
        "the plain model clamps the requested high to off"
    );
    assert_eq!(
        agent_wire_level(&engine),
        "off",
        "the agent slot re-syncs to the re-clamped reported level"
    );

    assert!(engine.switch_model(EngineModelSelection {
        provider: Some("battery".to_string()),
        model: Some("mock-reason".to_string()),
        api_key: None,
        thinking: None,
    }));
    assert_eq!(
        engine.effective_thinking_level().as_deref(),
        Some("high"),
        "the requested level survives the round trip"
    );
    assert_eq!(
        agent_wire_level(&engine),
        "high",
        "the agent slot re-syncs to the restored level"
    );
}

fn missing_default_create_worker(dir: &std::path::Path) -> std::sync::Arc<crate::worker::Worker> {
    let agent_dir = dir.join("agent");
    write_anthropic_auth(&agent_dir);
    std::fs::write(
        agent_dir.join("settings.json"),
        json!({
            "defaultProvider": "anthropic",
            "defaultModel": "claude-retired",
            "enabledModels": ["anthropic/claude-sonnet-5-5", "anthropic/claude-retired"]
        })
        .to_string(),
    )
    .unwrap();
    std::sync::Arc::new(crate::worker::Worker::new(
        crate::worker::WorkerConfig {
            socket_path: dir.join("worker.sock"),
            supervisor_socket_path: std::path::PathBuf::new(),
            token: "model-race".to_string(),
            worker_instance_id: String::new(),
            active_session_id: "race-create".to_string(),
            agent_dir,
            recovery_journal_path: dir.join("recovery.jsonl"),
            script: None,
            telemetry_disabled: Some(true),
        },
        None,
    ))
}

#[tokio::test]
async fn a_fresh_scoped_create_falls_back_with_a_recorded_message() {
    for no_session in [false, true] {
        let dir = tempfile::TempDir::new().unwrap();
        let worker = missing_default_create_worker(dir.path());
        let mut payload = json!({ "cwd": dir.path() });
        if no_session {
            payload["noSession"] = json!(true);
        } else {
            payload["sessionPath"] = json!(dir.path().join("race-session.jsonl"));
        }
        let created = worker.dispatch("create", &payload).await;
        assert!(created.success, "create failed: {created:?}");
        let summary = created.data.expect("create summary");
        let state = worker
            .dispatch(
                "get_connection_state",
                &json!({ "activeSessionId": "race-create" }),
            )
            .await;
        let data = state.data.expect("state data");
        assert_eq!(
            data["model"]["id"], PINNED_MODEL,
            "the fallback is the first scoped model: {data}"
        );
        assert_eq!(
            summary["modelFallbackMessage"],
            "Default model anthropic/claude-retired is not available. Using anthropic/claude-sonnet-5-5",
            "the fallback is on the record (noSession={no_session}): {summary}"
        );
        let attached = worker
            .dispatch("attach", &json!({ "activeSessionId": "race-create" }))
            .await;
        assert!(attached.success, "attach failed: {attached:?}");
        let snapshot = attached.data.expect("attach data");
        assert_eq!(
            snapshot["snapshot"]["summary"]["modelFallbackMessage"],
            summary["modelFallbackMessage"],
            "the attach summary delivers the warning to the TUI"
        );
        if no_session {
            let path = dir.path().join("authorized-session.jsonl");
            let mut target =
                crate::session_store::SessionFile::create(dir.path().to_str().unwrap(), None, 0);
            target.set_path(path.clone());
            target.append_model_change("anthropic", PINNED_MODEL);
            target.rewrite().unwrap();
            let switched = worker
                .dispatch(
                    "switch_session",
                    &json!({ "activeSessionId": "race-create", "sessionPath": path }),
                )
                .await;
            assert!(switched.success, "switch failed: {switched:?}");
            let attached = worker
                .dispatch("attach", &json!({ "activeSessionId": "race-create" }))
                .await;
            assert!(attached.success, "attach failed: {attached:?}");
            let snapshot = attached.data.expect("replacement attach data");
            assert!(
                snapshot["snapshot"]["summary"]["modelFallbackMessage"].is_null(),
                "navigation must not replay the old in-memory warning: {snapshot}"
            );
        }
    }
}

#[test]
fn an_explicit_scope_with_no_matches_does_not_warn_about_an_excluded_default() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_custom_provider_models_json(&agent_dir, "http://127.0.0.1:9");
    std::fs::write(
        agent_dir.join("settings.json"),
        json!({"defaultProvider": "anthropic", "defaultModel": "excluded-default"}).to_string(),
    )
    .unwrap();
    let engine = restore_test_engine(dir.path(), None, None);
    *engine.startup_scope.lock().unwrap() = Some(StartupScope {
        scoped_models: Vec::new(),
        patterns: vec!["battery/no-such-model".to_string()],
        is_continuing: false,
    });
    let resolved = engine.resolve_registry_model().expect("available fallback");
    assert_eq!(
        (resolved.provider.as_str(), resolved.id.as_str()),
        ("battery", "mock-1")
    );
    assert!(
        engine.model_fallback_message().is_none(),
        "explicitly excluded defaults do not imply an unavailable-model fallback"
    );
}
