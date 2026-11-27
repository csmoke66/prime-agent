//! The `/model` picker serves the current registry snapshot. Its optional Codex badges come
//! from the signed-in account's catalog.

use serde_json::{json, Value};

use crate::model_allowlist::DaemonAllowlist;
use crate::protocol::{response_success, DaemonResponse};
use crate::worker::Worker;

impl Worker {
    pub(crate) fn model_selection_policy(&self) -> DaemonAllowlist {
        self.agent_engine.as_ref().map_or_else(
            || DaemonAllowlist::Unrestricted,
            |engine| engine.model_selection_policy(),
        )
    }

    /// `get_model_catalog`: the picker loads this response off the UI thread.
    pub(crate) async fn handle_get_model_catalog(&self) -> DaemonResponse {
        if let Err(response) = self.require_created("get_model_catalog") {
            return response;
        }
        let agent_dir = &self.config.agent_dir;
        let registry = pa_core::models::ModelRegistry::create(
            pa_core::auth::AuthStorage::create(agent_dir),
            agent_dir.join("models.json"),
        );
        let allowlist = self.model_selection_policy();
        let available = registry.get_available();
        let mut served = catalog_payload(&registry, available.iter().copied(), &allowlist);
        if let Some(engine) = &self.agent_engine {
            let mut logins = std::collections::HashMap::new();
            let mut badges = Vec::new();
            for model in available {
                if !pa_ai::is_codex_daybreak_target(model) || !allowlist.allows_model(model) {
                    continue;
                }
                let ids = match logins.entry(model.provider.as_str()) {
                    std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        let ids = match engine.daybreak_login(model.clone()).await {
                            Some((_, token)) => pa_ai::codex_daybreak_model_ids(&token).await,
                            None => Vec::new(),
                        };
                        entry.insert(ids)
                    }
                };
                if ids.contains(&model.id) {
                    badges.push(format!("{}/{}", model.provider, model.id));
                }
            }
            served["daybreakBlueModels"] = json!(badges);
        }
        response_success(None, "get_model_catalog", Some(served))
    }
}

/// The wire payload of one catalog snapshot: policy-visible models and their configured
/// providers.
fn catalog_payload<'a>(
    registry: &pa_core::models::ModelRegistry,
    available: impl IntoIterator<Item = &'a pa_types::ai::Model>,
    allowlist: &DaemonAllowlist,
) -> Value {
    let mut providers = Vec::new();
    for model in available {
        if allowlist.allows_model(model) {
            providers.push(model.provider.as_str());
        }
    }
    providers.sort();
    providers.dedup();
    // Canonical (provider, id) order.
    let mut entries: Vec<&pa_types::ai::Model> = registry
        .get_all()
        .iter()
        .filter(|model| allowlist.allows_model(model))
        .collect();
    entries.sort_by(|a, b| (&a.provider, &a.id).cmp(&(&b.provider, &b.id)));
    let models: Vec<Value> = entries
        .into_iter()
        .map(|model| serde_json::to_value(model).unwrap_or(Value::Null))
        .collect();
    json!({
        "models": models,
        "configuredProviders": providers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::WorkerConfig;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn has_model(payload: &Value, provider: &str, id: &str) -> bool {
        payload["models"]
            .as_array()
            .expect("models array")
            .iter()
            .any(|model| model["provider"] == provider && model["id"] == id)
    }

    /// The root picker serves every model, including ones outside the global
    /// `allowedModels` policy, while `get_available_models` keeps the policy.
    #[tokio::test]
    async fn root_picker_includes_models_outside_global_policy() {
        let dir = tempfile::tempdir().expect("temp dir");
        let agent_dir = dir.path().join("agent");
        std::fs::create_dir_all(&agent_dir).expect("agent dir");
        std::fs::write(
            agent_dir.join("models.json"),
            json!({ "providers": { "local": {
                "api": "openai-completions", "baseUrl": "http://127.0.0.1:9/v1",
                "apiKey": "sk-local",
                "models": [{ "id": "mock-1", "name": "Mock 1", "contextWindow": 128_000,
                             "maxTokens": 4096 }]
            }}})
            .to_string(),
        )
        .expect("write models.json");
        std::fs::write(
            agent_dir.join("auth.json"),
            json!({ "anthropic": { "type": "api_key", "key": "sk-ant" } }).to_string(),
        )
        .expect("write auth.json");
        std::fs::write(
            agent_dir.join("settings.json"),
            json!({ "allowedModels": ["local/mock-1"] }).to_string(),
        )
        .expect("write global settings");
        let config = WorkerConfig {
            socket_path: dir.path().join("worker.sock"),
            supervisor_socket_path: PathBuf::new(),
            token: "token".to_string(),
            worker_instance_id: String::new(),
            active_session_id: "catalog-session".to_string(),
            agent_dir,
            recovery_journal_path: dir.path().join("recovery.jsonl"),
            telemetry_disabled: None,
            script: Some(json!({ "responses": ["ack"] })),
        };
        let worker = Arc::new(Worker::new(config, None));
        let created = worker
            .dispatch(
                "create",
                &json!({ "noSession": true, "cwd": dir.path().to_string_lossy() }),
            )
            .await;
        assert!(created.success, "create failed: {created:?}");

        let served = worker
            .dispatch(
                "get_model_catalog",
                &json!({ "activeSessionId": "catalog-session" }),
            )
            .await
            .data
            .expect("catalog payload");
        assert!(has_model(&served, "local", "mock-1"));
        assert!(has_model(&served, "anthropic", "claude-sonnet-5-5"));
        let keys: Vec<(String, String)> = served["models"]
            .as_array()
            .expect("models array")
            .iter()
            .map(|model| {
                (
                    model["provider"].as_str().unwrap_or_default().to_string(),
                    model["id"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "the payload's models are canonically ordered");

        let available = worker
            .dispatch(
                "get_available_models",
                &json!({ "activeSessionId": "catalog-session" }),
            )
            .await
            .data
            .expect("available models");
        assert!(has_model(&available, "local", "mock-1"));
        assert!(!has_model(&available, "anthropic", "claude-sonnet-5-5"));
    }
}
