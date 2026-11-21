//! RLM child model discovery and resolution against the authenticated catalog and
//! daemon policy, plus the spawn-time thinking and working-directory checks and
//! roster text caps.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use pa_ai::models::{get_supported_thinking_levels, thinking_level_from_str};
use pa_core::kernel::rlm_runtime::{find_rlm_model_matches, RlmModelInfo};
use pa_types::ai::Model;

use crate::model_allowlist::{self, DaemonAllowlist};

/// Close matches listed in model-resolution errors.
const MODEL_ERROR_SUGGESTION_LIMIT: usize = 3;
/// Cap on the answer preview handed to the parent model.
pub const ANSWER_PREVIEW_MAX_CHARS: usize = 160;
/// Cap on the one-line task label shown in kernel rosters.
pub const LABEL_MAX_CHARS: usize = 200;
/// Cap on the FULL final-answer text the collect envelope carries (the
/// settle-binding data plane): the roster preview stays
/// [`ANSWER_PREVIEW_MAX_CHARS`], while a consumer that binds outputs from
/// the answer (the factory's output ports) needs the whole fenced JSON,
/// so the lane is bounded instead of unbounded, never ballooning the
/// collect reply for a runaway answer.
pub const ANSWER_TEXT_MAX_CHARS: usize = 65_536;
const ELLIPSIS: &str = "...";

/// The authenticated RLM catalog, narrowed to the global daemon policy before ranking.
pub(super) fn discoverable_models(cwd: &Path, agent_dir: &Path) -> Result<Vec<RlmModelInfo>> {
    let allowlist = model_allowlist::load(cwd, agent_dir);
    require_readable_policy(&allowlist)?;
    let registry = crate::state_getters::worker_model_registry(agent_dir);
    Ok(eligible_models(
        registry.get_rlm_searchable_models(),
        &allowlist,
    ))
}

fn require_readable_policy(allowlist: &DaemonAllowlist) -> Result<()> {
    if let DaemonAllowlist::Unreadable(error) = allowlist {
        bail!(
            "The daemon model allowlist could not be read ({error}); \
             refusing model discovery. Fix settings.json and retry."
        );
    }
    Ok(())
}

fn model_selector(model: &Model) -> String {
    format!("{}/{}", model.provider, model.id)
}

fn eligible_models(models: Vec<&Model>, allowlist: &DaemonAllowlist) -> Vec<RlmModelInfo> {
    models
        .into_iter()
        .filter(|model| allowlist.allows_model(model))
        .map(|model| RlmModelInfo {
            provider: model.provider.clone(),
            id: model.id.clone(),
            name: model.name.clone(),
        })
        .collect()
}

/// Resolve the child model reference, then enforce the daemon
/// `allowedModels` allowlist on the resolved selector (inherited parent
/// model included): outside the allowlist fails loudly, never a fallback.
///
/// # Errors
/// Returns an error when the reference cannot be resolved or the selector is outside the allowlist.
pub fn resolve_child_model(
    agent_dir: &Path,
    reference: Option<&str>,
    parent_model: Option<&str>,
    target: &str,
    allowlist: &DaemonAllowlist,
) -> Result<String> {
    let model = resolve_child_model_reference(agent_dir, reference, parent_model, target, allowlist)?;
    model_allowlist::assert_allowed(allowlist, &model)?;
    Ok(model)
}

/// Resolve a child model using the authenticated catalog and without applying daemon policy.
/// Public for the daemonless print host: print sessions do not enforce
/// `allowedModels`, so its children do not either.
///
/// # Errors
/// Returns an error when the reference cannot be resolved against the catalog.
pub fn resolve_child_model_unchecked(
    agent_dir: &Path,
    reference: Option<&str>,
    parent_model: Option<&str>,
    target: &str,
) -> Result<String> {
    resolve_child_model(
        agent_dir,
        reference,
        parent_model,
        target,
        &DaemonAllowlist::Unrestricted,
    )
}

fn resolve_child_model_reference(
    agent_dir: &Path,
    reference: Option<&str>,
    parent_model: Option<&str>,
    target: &str,
    allowlist: &DaemonAllowlist,
) -> Result<String> {
    let Some(reference) = reference else {
        return parent_model
            .map(str::to_string)
            .ok_or_else(|| anyhow!("No model selected. Use /model to pick one."));
    };
    let reference = reference.trim();
    let normalized = reference.to_lowercase();
    if let Some(parent) = parent_model {
        if parent.to_lowercase() == normalized {
            return Ok(parent.to_string());
        }
    }
    let registry = crate::state_getters::worker_model_registry(agent_dir);
    let candidates = registry.get_rlm_searchable_models();
    // Resolve explicit selectors against the authenticated catalog so a blocked
    // selector still reaches the typed refusal rather than appearing unavailable.
    if normalized.contains('/') {
        if let Some(exact) = candidates
            .iter()
            .map(|model| model_selector(model))
            .find(|selector| selector.to_lowercase() == normalized)
        {
            return Ok(exact);
        }
    }
    let suffix = format!("/{normalized}");
    let mut short_matches = candidates.iter().filter_map(|model| {
        let selector = model_selector(model);
        selector
            .to_lowercase()
            .ends_with(&suffix)
            .then_some(selector)
    });
    let ambiguous = match (short_matches.next(), short_matches.next()) {
        // Preserve refusals for a uniquely-resolved short reference, too.
        (Some(single), None) => return Ok(single),
        (Some(first), Some(second)) => {
            let mut allowed = [first, second]
                .into_iter()
                .chain(short_matches)
                .filter(|selector| allowlist.allows(selector));
            match (allowed.next(), allowed.next()) {
                (Some(single), None) => return Ok(single),
                (Some(_), Some(_)) => true,
                _ => false,
            }
        }
        _ => false,
    };
    if !ambiguous {
        if let Some(parent) = parent_model {
            if parent.to_lowercase().ends_with(&suffix) {
                return Ok(parent.to_string());
            }
        }
    }
    require_readable_policy(allowlist)?;
    let eligible = eligible_models(candidates, allowlist);
    Err(model_unavailable_error(reference, target, &eligible))
}

/// A requested thinking level must be supported by the resolved model;
/// an out-of-catalog model (a scripted verification model) passes unchecked.
///
/// # Errors
/// Returns an error when the supported levels exclude the requested one (the message lists them);
/// the unchecked cases pass.
pub fn assert_thinking_supported(
    agent_dir: &Path,
    level: Option<&str>,
    selector: &str,
) -> Result<()> {
    let Some(level) = level else {
        return Ok(());
    };
    let Some((provider, id)) = selector.split_once('/') else {
        return Ok(());
    };
    let registry = crate::state_getters::worker_model_registry(agent_dir);
    let Some(model) = registry
        .get_rlm_searchable_models()
        .into_iter()
        .find(|model| model.provider == provider && model.id == id)
    else {
        return Ok(());
    };
    let supported = get_supported_thinking_levels(model);
    let Some(requested) = thinking_level_from_str(level) else {
        return Ok(());
    };
    if supported.contains(&requested) {
        return Ok(());
    }
    let levels = supported
        .iter()
        .map(|level| level.wire_name())
        .collect::<Vec<_>>()
        .join(", ");
    bail!(
        "Requested thinking level \"{level}\" is not supported by model \"{selector}\"; supported levels: {levels}"
    );
}

/// The directory a child starts in: a requested `cwd` resolves against the
/// session cwd and is canonicalized (project-context discovery walks its
/// ancestors), otherwise the child shares the session cwd.
///
/// # Errors
/// Returns an error naming `operation` when the requested directory does not exist.
pub fn resolve_child_cwd(
    session_cwd: &Path,
    requested: Option<&str>,
    operation: &str,
) -> Result<PathBuf> {
    let Some(requested) = requested else {
        return Ok(session_cwd.to_path_buf());
    };
    let joined = session_cwd.join(requested);
    match joined.canonicalize() {
        Ok(cwd) if cwd.is_dir() => Ok(cwd),
        _ => bail!("{operation} cwd {} is not a directory", joined.display()),
    }
}

/// Rejection message for an unresolved model reference: the
/// unavailability, the selector form, and close matches for retry.
fn model_unavailable_error(
    reference: &str,
    target: &str,
    candidates: &[RlmModelInfo],
) -> anyhow::Error {
    let base = format!(
        "Requested {target} model \"{reference}\" is unavailable, unauthenticated, or expired"
    );
    let hint =
        "selectors use the form \"provider/model-id\" (e.g. \"prime-inference/z-ai/glm-5.3\")";
    let close_matches = find_rlm_model_matches(reference, candidates, MODEL_ERROR_SUGGESTION_LIMIT);
    if close_matches.is_empty() {
        anyhow!("{base}; {hint}")
    } else {
        let selectors = close_matches
            .iter()
            .map(|match_| format!("\"{}\"", match_.selector))
            .collect::<Vec<_>>()
            .join(", ");
        anyhow!("{base}; {hint}; close matches: {selectors}")
    }
}

/// Collapse whitespace and cap at the roster limit.
#[must_use]
pub fn compact_rlm_text(text: &str) -> String {
    let compact: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    cap_text(&compact, ANSWER_PREVIEW_MAX_CHARS)
}

/// One-line task label: collapsed prompt, capped for roster rows.
#[must_use]
pub fn rlm_child_label(prompt: &str) -> String {
    let collapsed: String = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    let collapsed = if collapsed.is_empty() {
        "child agent".to_string()
    } else {
        collapsed
    };
    cap_text(&collapsed, LABEL_MAX_CHARS)
}

/// Whitespace-collapsed text capped at `max` chars with an ellipsis.
#[must_use]
pub fn cap_text(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max - ELLIPSIS.len()).collect();
    format!("{}{}", kept.trim_end(), ELLIPSIS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A models.json custom provider, like the pa-core registry tests.
    fn write_catalog(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("models.json"),
            json!({
                "providers": {
                    "test-provider": {
                        "baseUrl": "http://localhost:9",
                        "apiKey": "test-key",
                        "api": "openai-completions",
                        "models": [
                            { "id": "glm-5.3", "name": "GLM 5.3", "contextWindow": 1000, "maxTokens": 100 },
                            { "id": "glm-5.3-turbo", "name": "GLM Turbo", "contextWindow": 1000, "maxTokens": 100 }
                        ]
                    },
                    "other-provider": {
                        "baseUrl": "http://localhost:9",
                        "apiKey": "test-key",
                        "api": "openai-completions",
                        "models": [
                            { "id": "glm-5.3", "name": "Other GLM", "contextWindow": 1000, "maxTokens": 100 }
                        ]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn daemon_find_models_filters_global_policy_before_ranking_and_fails_closed() {
        use std::sync::Arc;

        use pa_core::kernel::shared::HostRequestPayload;
        use pa_core::session::manager::SessionManager;
        use pa_core::session_engine::runtime_wiring::{wire_session_runtime, RlmWiring};

        use crate::rlm_children::{ParentIdentity, SupervisorChildSessions};

        let dir = tempfile::TempDir::new().unwrap();
        let agent_dir = dir.path().join("agent");
        write_catalog(&agent_dir);
        let settings = agent_dir.join("settings.json");
        std::fs::write(
            &settings,
            r#"{"allowedModels":["test-provider/glm-5.3-turbo"]}"#,
        )
        .unwrap();
        let project_settings = dir.path().join(".prime/agent");
        std::fs::create_dir_all(&project_settings).unwrap();
        std::fs::write(
            project_settings.join("settings.json"),
            r#"{"allowedModels":["test-provider/glm-5.3"]}"#,
        )
        .unwrap();
        let host = SupervisorChildSessions::new(
            Arc::new(crate::supervisor_link::SupervisorLink::new(
                dir.path().join("supervisor.sock"),
            )),
            agent_dir.clone(),
            "parent-live".into(),
            Arc::new(model_allowlist::ModelRefusalTelemetry::new(
                agent_dir.clone(),
                true,
            )),
            String::new(),
        );
        host.set_identity(ParentIdentity {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..ParentIdentity::with_default_depth()
        });
        let wiring = wire_session_runtime(
            SessionManager::in_memory(dir.path()),
            &agent_dir,
            RlmWiring {
                subagent_host: Some(Arc::new(host)),
                ..RlmWiring::default()
            },
            None,
            None,
        );
        let handler = wiring.handlers.get("rlm.find_models").unwrap();
        let search = || {
            handler(HostRequestPayload {
                data: json!({ "query": "test-provider/glm-5.3", "limit": 1 }),
            })
        };
        let response = search().await.unwrap();
        assert_eq!(
            response["models"],
            json!([{
                "provider": "test-provider",
                "id": "glm-5.3-turbo",
                "name": "GLM Turbo",
                "selector": "test-provider/glm-5.3-turbo",
            }])
        );
        std::fs::write(&settings, r#"{"allowedModels":"invalid"}"#).unwrap();
        let error = search().await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("daemon model allowlist could not be read"),
            "{error}"
        );
    }

    #[test]
    fn short_model_resolution_and_suggestions_use_the_allowed_catalog() {
        let dir = tempfile::TempDir::new().unwrap();
        write_catalog(dir.path());
        let allow = DaemonAllowlist::Allowed(vec!["test-provider/*".into()]);
        let resolved =
            resolve_child_model(dir.path(), Some("glm-5.3"), None, "subagent", &allow).unwrap();
        assert_eq!(resolved, "test-provider/glm-5.3");

        let refused = resolve_child_model(
            dir.path(),
            Some("other-provider/glm-5.3"),
            None,
            "subagent",
            &allow,
        )
        .unwrap_err();
        assert_eq!(
            refused
                .downcast_ref::<pa_core::models::ModelAllowlistRefusal>()
                .unwrap()
                .selector,
            "other-provider/glm-5.3"
        );

        let error = resolve_child_model(dir.path(), Some("glm-5"), None, "subagent", &allow)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("close matches: \"test-provider/glm-5.3\""),
            "{error}"
        );
        assert!(!error.contains("other-provider"), "{error}");
    }

    #[test]
    fn resolves_parent_exact_and_short_form_references() {
        let dir = tempfile::TempDir::new().unwrap();
        write_catalog(dir.path());
        let resolved = resolve_child_model(
            dir.path(),
            None,
            Some("test-provider/glm-5.3"),
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap();
        assert_eq!(resolved, "test-provider/glm-5.3");
        // Parent equality short-circuits even a catalog refresh miss.
        let resolved = resolve_child_model(
            dir.path(),
            Some("Test-Provider/GLM-5.3"),
            Some("test-provider/glm-5.3"),
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap();
        assert_eq!(resolved, "test-provider/glm-5.3");
        let resolved = resolve_child_model(
            dir.path(),
            Some("test-provider/glm-5.3-turbo"),
            Some("test-provider/glm-5.3"),
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap();
        assert_eq!(resolved, "test-provider/glm-5.3-turbo");
        let resolved = resolve_child_model(
            dir.path(),
            Some("glm-5.3-turbo"),
            Some("test-provider/glm-5.3"),
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap();
        assert_eq!(resolved, "test-provider/glm-5.3-turbo");
        let error = resolve_child_model(
            dir.path(),
            None,
            None,
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "No model selected. Use /model to pick one."
        );
    }

    #[test]
    fn the_allowlist_refuses_resolved_models_loudly() {
        let dir = tempfile::TempDir::new().unwrap();
        write_catalog(dir.path());
        let allow =
            crate::model_allowlist::DaemonAllowlist::Allowed(vec!["prime-inference/*".to_string()]);
        // An explicit reference outside the allowlist fails with the typed refusal.
        let error = resolve_child_model(
            dir.path(),
            Some("test-provider/glm-5.3"),
            None,
            "subagent",
            &allow,
        )
        .unwrap_err();
        let refusal = error
            .downcast_ref::<pa_core::models::ModelAllowlistRefusal>()
            .expect("typed refusal");
        assert_eq!(refusal.selector, "test-provider/glm-5.3");
        assert!(
            error
                .to_string()
                .contains("blocked by the daemon model allowlist"),
            "{error}"
        );
        // An inherited parent model outside the allowlist refuses too:
        // inheritance is a resolution, not an exemption.
        let error = resolve_child_model(
            dir.path(),
            None,
            Some("test-provider/glm-5.3"),
            "subagent",
            &allow,
        )
        .unwrap_err();
        assert!(
            error
                .downcast_ref::<pa_core::models::ModelAllowlistRefusal>()
                .is_some(),
            "{error}"
        );
        let resolved = resolve_child_model(
            dir.path(),
            Some("test-provider/glm-5.3"),
            None,
            "subagent",
            &crate::model_allowlist::DaemonAllowlist::Allowed(vec!["test-provider/*".to_string()]),
        )
        .unwrap();
        assert_eq!(resolved, "test-provider/glm-5.3");
        let resolved = resolve_child_model(
            dir.path(),
            Some("test-provider/glm-5.3"),
            None,
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap();
        assert_eq!(resolved, "test-provider/glm-5.3");
    }

    #[test]
    fn unmatched_references_carry_the_ts_error_with_close_matches() {
        let dir = tempfile::TempDir::new().unwrap();
        write_catalog(dir.path());
        // TS parity (4649 regression suite): a prefix reference lists
        // close matches, a matching-nothing reference does not (the
        // reference prefixes the test provider, never real defaults).
        let error = resolve_child_model(
            dir.path(),
            Some("test-provi"),
            Some("test-provider/glm-5.3"),
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(
            message.starts_with(
                "Requested subagent model \"test-provi\" is unavailable, unauthenticated, or expired; selectors use the form \"provider/model-id\""
            ),
            "{message}"
        );
        // Close matches list full selectors, nearest first.
        assert!(
            message.contains("close matches: \"test-provider/glm-5.3\""),
            "{message}"
        );
        let error = resolve_child_model(
            dir.path(),
            Some("zzz"),
            Some("test-provider/glm-5.3"),
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap_err();
        assert!(!error.to_string().contains("close matches:"), "{error}");
    }

    #[test]
    fn thinking_support_follows_the_resolved_model() {
        let dir = tempfile::TempDir::new().unwrap();
        write_catalog(dir.path());
        // The custom catalog model is non-reasoning: only "off" is supported.
        assert_thinking_supported(dir.path(), Some("off"), "test-provider/glm-5.3").unwrap();
        let error = assert_thinking_supported(dir.path(), Some("high"), "test-provider/glm-5.3")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Requested thinking level \"high\" is not supported by model \"test-provider/glm-5.3\"; supported levels: off"
        );
        // A model outside the catalog (scripted verification models) passes.
        assert_thinking_supported(dir.path(), Some("high"), "scripted/faux-1").unwrap();
    }

    /// The child-propagation regression: a spawned child resolves an
    /// entitled private `internal/*` model through the worker's same disk caches.
    #[test]
    fn a_spawned_child_resolves_an_entitled_private_model() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        // The worker-path auth (auth.json): one Prime Inference key+team.
        std::fs::write(
            dir.path().join("auth.json"),
            json!({
                "prime-inference": {
                    "type": "api_key",
                    "key": "child-key",
                    "primeTeam": { "teamId": "team-7", "name": "Team 7" }
                }
            })
            .to_string(),
        )
        .unwrap();
        // The entitled private model: the compiled fallback lacks it, the
        // account's scoped Prime Inference snapshot carries it.
        let mut auth = pa_core::auth::AuthStorage::create(dir.path());
        let api_key = auth.get_api_key("prime-inference").expect("api key");
        // The snapshot below is written for whatever pair resolves (an
        // ambient `PRIME_API_KEY` on a dogfood box changes the pair).
        let team_id = auth
            .get_provider_headers("prime-inference")
            .and_then(|headers| headers.get("X-Prime-Team-ID").cloned())
            .unwrap_or_default();
        // Every compiled route, so the snapshot passes the coverage gate.
        let mut data: Vec<serde_json::Value> =
            pa_models::transports::prime_inference_offline_entries()
                .iter()
                .map(|model| {
                    json!({
                        "id": model.id,
                        "pricing": { "input_usd_per_mtok": 1.0, "output_usd_per_mtok": 1.0 },
                    })
                })
                .collect();
        data.push(json!({
            "id": "internal/glm-5.3-fast",
            "display_name": "GLM 5.3 Fast",
            "pricing": { "input_usd_per_mtok": 0.42, "output_usd_per_mtok": 2.1 },
            "specs": {
                "context_window": 400_000, "max_output_tokens": 131_072,
                "supports_reasoning": true,
                "modalities": { "input": ["text"], "output": ["text"] },
            },
        }));
        std::fs::create_dir_all(dir.path().join("models")).unwrap();
        std::fs::write(
            dir.path().join("models/prime-inference-models-cache.json"),
            json!({
                "url": format!("{}/models", pa_core::models::PRIME_INFERENCE_BASE_URL),
                "scope": pa_models::prime_inference::scope_key(&api_key, &team_id),
                "fetchedAt": 1,
                "payload": { "data": data },
            })
            .to_string(),
        )
        .unwrap();

        assert!(discoverable_models(dir.path(), dir.path())
            .unwrap()
            .iter()
            .any(|model| model.id == "internal/glm-5.3-fast"));
        let resolved = resolve_child_model(
            dir.path(),
            Some("prime-inference/internal/glm-5.3-fast"),
            Some("prime-inference/z-ai/glm-5.3"),
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap();
        assert_eq!(resolved, "prime-inference/internal/glm-5.3-fast");
        let resolved = resolve_child_model(
            dir.path(),
            Some("internal/glm-5.3-fast"),
            Some("prime-inference/z-ai/glm-5.3"),
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap();
        assert_eq!(resolved, "prime-inference/internal/glm-5.3-fast");
        // The spawn-time thinking check follows the resolved entitlement.
        assert_thinking_supported(
            dir.path(),
            Some("high"),
            "prime-inference/internal/glm-5.3-fast",
        )
        .unwrap();
        assert_thinking_supported(
            dir.path(),
            Some("off"),
            "prime-inference/internal/glm-5.3-fast",
        )
        .unwrap();
    }

    /// The catalog-repo child-resolution regression: a fetched entry the compiled fallback lacks
    /// resolves through the on-disk provider catalog, once its provider is auth-configured.
    #[test]
    fn a_spawned_child_resolves_a_catalog_repo_entry_the_compiled_fallback_lacks() {
        const PROBE_ID: &str = "gpt-6-probe";
        assert!(
            !pa_ai::models_generated::get_models("openai-codex")
                .iter()
                .any(|model| model.id == PROBE_ID),
            "the probe entry is compiled in; pick another id"
        );
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("models")).unwrap();
        // The provider auth the resolution filters on: a models.json
        // provider entry (headers alone satisfy the config validation and
        // has_configured_auth).
        std::fs::write(
            dir.path().join("models.json"),
            json!({
                "providers": {
                    "openai-codex": {
                        "apiKey": "codex-key",
                        "headers": { "X-Probe": "catalog-chain" }
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        // The layer-A disk cache the startup refresh writes beside
        // models.json: one catalog-repo entry riding the compiled
        // openai-codex transport tuple (the pinning invariant).
        std::fs::write(
            dir.path()
                .join("models")
                .join("provider-model-catalog.v1.json"),
            json!({
                "url": pa_models::fetch::MODEL_CATALOG_URL,
                "scope": pa_models::cache::PUBLIC_SCOPE,
                "fetchedAt": 1,
                "payload": { "schemaVersion": 1, "models": [
                    {
                        "id": PROBE_ID, "name": "GPT-6 Probe",
                        "api": "openai-codex-responses", "provider": "openai-codex",
                        "baseUrl": "https://chatgpt.com/backend-api",
                        "reasoning": true,
                        "thinkingLevelMap": { "minimal": null, "xhigh": "xhigh", "max": "max" },
                        "input": ["text"],
                        "cost": { "input": 2, "output": 10, "cacheRead": 0.2, "cacheWrite": 2.5 },
                        "contextWindow": 272_000, "maxTokens": 128_000,
                    }
                ]}
            })
            .to_string(),
        )
        .unwrap();
        let selector = format!("openai-codex/{PROBE_ID}");
        let catalog = discoverable_models(dir.path(), dir.path()).unwrap();
        let probe = catalog
            .iter()
            .find(|model| model.selector() == selector)
            .expect("the fetched entry lists for find_models");
        assert_eq!(probe.name, "GPT-6 Probe");
        let resolved = resolve_child_model(
            dir.path(),
            Some(&selector),
            Some("prime-inference/z-ai/glm-5.3"),
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap();
        assert_eq!(resolved, selector);
        let resolved = resolve_child_model(
            dir.path(),
            Some(PROBE_ID),
            Some("prime-inference/z-ai/glm-5.3"),
            "subagent",
            &DaemonAllowlist::Unrestricted,
        )
        .unwrap();
        assert_eq!(resolved, selector);
        // The spawn-time thinking check follows the fetched entry's map:
        // xhigh is explicitly mapped, minimal is explicitly nulled out.
        assert_thinking_supported(dir.path(), Some("xhigh"), &selector).unwrap();
        let error = assert_thinking_supported(dir.path(), Some("minimal"), &selector).unwrap_err();
        assert!(
            error.to_string().contains("not supported by model"),
            "{error}"
        );
    }

    #[test]
    fn roster_text_is_collapsed_and_capped() {
        let label = rlm_child_label("  ship   the\nlane  ");
        assert_eq!(label, "ship the lane");
        let long = "word ".repeat(100);
        let label = rlm_child_label(&long);
        assert!(label.chars().count() <= LABEL_MAX_CHARS);
        assert!(label.ends_with("..."));
        let preview = compact_rlm_text("  a\nshort   answer ");
        assert_eq!(preview, "a short answer");
        let long_answer = "x".repeat(ANSWER_PREVIEW_MAX_CHARS + 50);
        let preview = compact_rlm_text(&long_answer);
        assert_eq!(preview.chars().count(), ANSWER_PREVIEW_MAX_CHARS);
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn the_binding_lane_keeps_far_more_than_the_preview_cap() {
        // The collect envelope's full-answer lane: a fenced JSON output
        // longer than the 160-character roster preview binds whole from
        // it, so the bound stays far above the preview cap and only a
        // genuinely oversized payload (over ANSWER_TEXT_MAX_CHARS) sees
        // the ellipsis.
        let long = "y".repeat(ANSWER_TEXT_MAX_CHARS + 50);
        let capped = cap_text(&long, ANSWER_TEXT_MAX_CHARS);
        assert_eq!(capped.chars().count(), ANSWER_TEXT_MAX_CHARS);
        assert!(capped.ends_with("..."));
        let wide_json = format!(
            "{{\"blob\": \"{}\"}}",
            "z".repeat(ANSWER_PREVIEW_MAX_CHARS * 4)
        );
        let lane = cap_text(&wide_json, ANSWER_TEXT_MAX_CHARS);
        assert_eq!(lane, wide_json, "a wide-but-bounded JSON keeps its tail");
    }
}
