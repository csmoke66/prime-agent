//! The daemon's enforcement of the settings `allowedModels` allowlist:
//! the worker-side model-resolution seams refuse a model outside the
//! allowlist loudly (never a fallback) and emit the adoption event
//! (`model refused`).
use anyhow::Result;
use std::path::Path;

use pa_core::models::ModelAllowlistRefusal;
use pa_telemetry::TelemetryClient;
use pa_types::ai::Model;

/// The daemon allowlist's loaded state (settings `allowedModels`, global
/// scope). A security guardrail fails CLOSED: a settings document that
/// could not be loaded is an unknown policy, never an unrestricted one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonAllowlist {
    /// No `allowedModels` configured: unrestricted (TS parity).
    Unrestricted,
    /// The configured patterns.
    Allowed(Vec<String>),
    /// Explicit qualified model selections for one session family.
    Selected(Vec<String>),
    /// The global settings document could not be loaded (lock contention,
    /// read, or parse failure): the configured policy is unknown, so every
    /// resolution fails loudly instead of bypassing it.
    Unreadable(String),
}

impl DaemonAllowlist {
    /// Interpret the stored model override for one session family.
    #[must_use]
    pub fn session_override(models: Vec<String>, exact_selectors: bool) -> Self {
        if models.is_empty() {
            Self::Unrestricted
        } else if exact_selectors {
            Self::Selected(models)
        } else {
            Self::Allowed(models)
        }
    }

    /// Whether a selector may be listed. An unreadable policy allows nothing.
    #[must_use]
    pub fn allows(&self, selector: &str) -> bool {
        match self {
            Self::Unrestricted => true,
            Self::Allowed(patterns) => pa_core::models::model_allowed(selector, patterns),
            Self::Selected(models) => models.iter().any(|model| model == selector),
            Self::Unreadable(_) => false,
        }
    }

    /// Whether a catalog model may be listed; only configured patterns need its selector.
    #[must_use]
    pub fn allows_model(&self, model: &Model) -> bool {
        match self {
            Self::Unrestricted => true,
            Self::Allowed(_) => self.allows(&format!("{}/{}", model.provider, model.id)),
            Self::Selected(models) => models
                .iter()
                .any(|selector| {
                    selector.split_once('/') == Some((model.provider.as_str(), model.id.as_str()))
                }),
            Self::Unreadable(_) => false,
        }
    }
}

/// Load the daemon allowlist state (settings `allowedModels`, global
/// scope).
#[must_use]
pub fn load(cwd: &Path, agent_dir: &Path) -> DaemonAllowlist {
    let settings = pa_core::settings::SettingsManager::create(cwd, agent_dir);
    if let Some(error) = settings
        .errors()
        .iter()
        .find(|error| error.scope == pa_core::settings::SettingsScope::Global)
    {
        return DaemonAllowlist::Unreadable(error.message.clone());
    }
    if let Some(patterns) = settings.get_allowed_models() {
        DaemonAllowlist::Allowed(patterns)
    } else {
        // A PRESENT-but-malformed `allowedModels` (the lenient load drops
        // wrong-typed known fields to unset) fails closed: the restriction
        // was requested, so an unreadable shape is never an unrestricted
        // gate; explicit `null` stays unset (TS parity).
        let unreadable = settings.global_raw().and_then(|raw| {
            if !raw.is_object() {
                return Some("the global settings document is not a JSON object".to_string());
            }
            let malformed = match raw.get("allowedModels") {
                None | Some(serde_json::Value::Null) => false,
                Some(value) => value
                    .as_array()
                    .is_none_or(|items| items.iter().any(|item| item.as_str().is_none())),
            };
            malformed.then(|| "allowedModels is present but is not an array of strings".to_string())
        });
        match unreadable {
            Some(message) => DaemonAllowlist::Unreadable(message),
            None => DaemonAllowlist::Unrestricted,
        }
    }
}

/// Resolve both child controls from one session-policy snapshot. A session
/// override *replaces* the global list; it is not an intersection.
pub(crate) fn child_policy(
    cwd: &Path,
    agent_dir: &Path,
    session_file: Option<&Path>,
    depth: u32,
) -> Result<(
    DaemonAllowlist,
    Option<pa_core::settings::ThinkingLevelSetting>,
)> {
    policy_for(cwd, agent_dir, session_file, depth, false)
}

pub(crate) fn policy_for(
    cwd: &Path, agent_dir: &Path, session_file: Option<&Path>, depth: u32, own_model: bool,
) -> Result<(DaemonAllowlist, Option<pa_core::settings::ThinkingLevelSetting>)> {
    policy_for_with_grant(cwd, agent_dir, session_file, depth, own_model, pa_core::workflows::load_model_override)
}

fn policy_for_with_grant(
    cwd: &Path,
    agent_dir: &Path,
    session_file: Option<&Path>,
    depth: u32,
    own_model: bool,
    load_grant: impl Fn(&str) -> Result<pa_core::workflows::Workflow>,
) -> Result<(DaemonAllowlist, Option<pa_core::settings::ThinkingLevelSetting>)> {
    use pa_core::session::subagent_policy::SessionSubagentPolicy;
    let mut selected: Option<Vec<String>> = None;
    let mut workflow_effort = None;
    let mut use_global_models = false;
    let policy = match session_file {
        Some(file) => {
            let root = SessionSubagentPolicy::visit_ancestors(file, depth, |log, at| {
                let Some(policy) = pa_core::session::subagent_policy::workflow_policy(log)? else { return Ok(()) };
                if let Some(name) = &policy.operator_workflow {
                    anyhow::ensure!(at == 1, "operator workflow override is not directly under a root session");
                    let grant = load_grant(name)?;
                    anyhow::ensure!(
                        grant.model == policy.runner_model
                            && grant.subagents.and_then(|subagents| subagents.allowed_models) == policy.allowed_models,
                        "operator workflow {name:?} model grant no longer matches this run"
                    );
                    use_global_models = true;
                }
                let models = if own_model && at == depth {
                    Some(vec![policy.runner_model])
                } else {
                    if let Some(effort) = policy.effort {
                        anyhow::ensure!(workflow_effort.is_none_or(|previous| previous == effort), "conflicting nested workflow effort constraints");
                        workflow_effort = Some(effort);
                    }
                    policy.allowed_models
                };
                if let Some(models) = models {
                    match &mut selected {
                        Some(existing) => existing.retain(|m| models.contains(m)),
                        None => selected = Some(models),
                    }
                }
                Ok(())
            })?;
            SessionSubagentPolicy::load(&root)?
        }
        None if depth == 0 => SessionSubagentPolicy::default(),
        None => anyhow::bail!("sub-agent policy cannot be read without the child's session log"),
    };
    let allowlist = if use_global_models {
        load(cwd, agent_dir)
    } else {
        policy.allowed_models.map_or_else(
            || load(cwd, agent_dir),
            |models| DaemonAllowlist::session_override(models, policy.exact_model_selectors),
        )
    };
    let allowlist = match selected {
        Some(mut models) if !matches!(allowlist, DaemonAllowlist::Unreadable(_)) => {
            models.retain(|m| allowlist.allows(m));
            DaemonAllowlist::Selected(models)
        }
        _ => allowlist,
    };
    anyhow::ensure!(workflow_effort.is_none() || policy.effort.is_none() || workflow_effort == policy.effort,
        "workflow effort conflicts with the session's child effort policy");
    Ok((allowlist, workflow_effort.or(policy.effort)))
}

/// Enforce the allowlist on a resolved selector: `Ok(())` when allowed
/// (and when no allowlist is configured), the typed refusal for an
/// off-allowlist model, and a fail-closed error when the policy is
/// unreadable.
pub(crate) fn assert_allowed(allowlist: &DaemonAllowlist, selector: &str) -> Result<()> {
    match allowlist {
        DaemonAllowlist::Unreadable(error) => Err(anyhow::anyhow!(
            "The daemon model allowlist could not be read ({error}); \
             refusing to resolve model \"{selector}\" — the daemon fails closed instead of \
             bypassing the configured policy. Check the session override and settings.json."
        )),
        _ if allowlist.allows(selector) => Ok(()),
        _ => Err(ModelAllowlistRefusal {
            selector: selector.to_string(),
        }
        .into()),
    }
}

/// The worker's refusal telemetry: one lazily-built client shared by the
/// worker's enforcement seams, kept for the worker lifetime (the sinks
/// batch asynchronously, so a dropped client loses the event). `None`
/// telemetry when the create command opted the session out.
pub struct ModelRefusalTelemetry {
    agent_dir: std::path::PathBuf,
    enabled: bool,
    client: std::sync::Mutex<Option<(std::path::PathBuf, TelemetryClient)>>,
    /// Already-noted `(surface, selector)` pairs: one event per distinct
    /// refusal per worker, so a polling getter that re-resolves a refused
    /// model (the connection-state surface) never spams the event.
    noted: std::sync::Mutex<std::collections::HashSet<(String, String)>>,
}

impl ModelRefusalTelemetry {
    #[must_use]
    pub fn new(agent_dir: std::path::PathBuf, telemetry_disabled: bool) -> Self {
        Self {
            agent_dir,
            enabled: !telemetry_disabled,
            client: std::sync::Mutex::new(None),
            noted: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Emit the refusal's `model refused` event (best-effort; no-op when
    /// opted out). The client binds to `cwd` — the settings posture is
    /// scoped (the enabled switch and local mirror read the project
    /// scope), so a session that moved directories rebinds instead of
    /// reporting through the old project.
    ///
    /// # Panics
    ///
    /// Panics when an internal mutex is poisoned; the client-bound expect
    /// is an internal invariant and cannot fire.
    pub fn note_refused(&self, surface: &str, selector: &str, cwd: &Path) {
        if !self.enabled {
            return;
        }
        let settings = pa_core::settings::SettingsManager::create(cwd, &self.agent_dir);
        // The same gating as the supervisor's `daemon event`: the env
        // override wins, else the merged settings' telemetry switch.
        if !pa_core::session_engine::telemetry::telemetry_switch(&settings).enabled() {
            return;
        }
        // Once per distinct (surface, selector).
        {
            let mut noted = self.noted.lock().expect("refusal noted lock");
            if !noted.insert((surface.to_string(), selector.to_string())) {
                return;
            }
        }
        let mut slot = self.client.lock().expect("refusal telemetry lock");
        if slot.as_ref().is_none_or(|(bound_cwd, _)| bound_cwd != cwd) {
            let client =
                pa_core::session_engine::telemetry::build_client(&settings, &self.agent_dir);
            *slot = Some((cwd.to_path_buf(), client));
        }
        let client = &slot.as_ref().expect("client bound").1;
        let (provider, model_id) = selector.split_once('/').unwrap_or((selector, ""));
        pa_core::session_engine::telemetry::track_model_refused(
            client, surface, provider, model_id,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_settings(dir: &Path, value: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("settings.json"), value).unwrap();
    }

    #[test]
    fn workflow_runner_and_descendants_obey_different_intersected_model_sets() {
        use pa_core::session::subagent_policy::{set_workflow_policy, SessionSubagentPolicy, WorkflowRunPolicy};
        use pa_core::settings::ThinkingLevelSetting;
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        let root = sessions.join("root.jsonl");
        let workflow = sessions.join("flow.jsonl");
        let nested = sessions.join("child.jsonl");
        for (file, parent, depth) in [(&workflow, &root, 1), (&nested, &workflow, 2)] {
            std::fs::write(file, serde_json::json!({"type":"session","id":"id","cwd":"/","timestamp":"2026-01-01","rlmDepth":depth,"parentSession":parent}).to_string()).unwrap();
        }
        SessionSubagentPolicy::set_model_selectors(&root, Some(vec!["p/main".into(),"p/child".into()])).unwrap();
        set_workflow_policy(&workflow, &WorkflowRunPolicy {
            runner_model: "p/main".into(),
            allowed_models: Some(vec!["p/child".into(), "p/blocked".into()]),
            effort: Some(ThinkingLevelSetting::Low),
            operator_workflow: None,
        }).unwrap();
        let read = |log: &Path, depth, own| policy_for(tmp.path(), tmp.path(), Some(log), depth, own).unwrap();
        assert_eq!(read(&workflow, 1, true).0, DaemonAllowlist::Selected(vec!["p/main".into()]));
        let (children, effort) = read(&nested, 2, true);
        assert_eq!(children, DaemonAllowlist::Selected(vec!["p/child".into()]));
        assert_eq!(effort, Some(ThinkingLevelSetting::Low));
        assert_eq!(read(&workflow, 1, false).0, children);
        SessionSubagentPolicy::set_model_selectors(&root, Some(vec!["p/main".into()])).unwrap();
        assert!(!read(&workflow, 1, false).0.allows("p/child"));
        assert!(read(&root, 0, false).0.allows("p/main"));
    }
    #[test]
    fn protected_workflow_can_use_globally_allowed_models_without_widening_root_spawns() {
        use pa_core::session::subagent_policy::{set_workflow_policy, SessionSubagentPolicy, WorkflowRunPolicy};
        let tmp = tempfile::tempdir().unwrap();
        write_settings(tmp.path(), r#"{"allowedModels":["p/normal","p/runner","p/helper"]}"#);
        let sessions = tmp.path().join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        let root = sessions.join("root.jsonl");
        let workflow = sessions.join("flow.jsonl");
        std::fs::write(&workflow, serde_json::json!({
            "type":"session","id":"flow","cwd":"/","timestamp":"2026-01-01",
            "rlmDepth":1,"parentSession":root
        }).to_string()).unwrap();
        SessionSubagentPolicy::set_model_selectors(&root, Some(vec!["p/normal".into()])).unwrap();
        set_workflow_policy(&workflow, &WorkflowRunPolicy {
            runner_model: "p/runner".into(),
            allowed_models: Some(vec!["p/helper".into()]),
            effort: None,
            operator_workflow: Some("review".into()),
        }).unwrap();
        let grant = |name: &str| {
            assert_eq!(name, "review");
            Ok(serde_json::from_value(serde_json::json!({
                "version":1,"model":"p/runner","prompt":"Review documentation.",
                "inputFields":{},"outputFields":{},"overrideSubagentModels":true,
                "subagents":{"allowedModels":["p/helper"]}
            })).unwrap())
        };
        let policy = |own| policy_for_with_grant(
            tmp.path(), tmp.path(), Some(&workflow), 1, own, grant,
        ).unwrap().0;
        assert_eq!(policy(true), DaemonAllowlist::Selected(vec!["p/runner".into()]));
        assert_eq!(policy(false), DaemonAllowlist::Selected(vec!["p/helper".into()]));
        assert!(!child_policy(tmp.path(), tmp.path(), Some(&root), 0).unwrap().0.allows("p/runner"));
        write_settings(tmp.path(), r#"{"allowedModels":["p/normal"]}"#);
        assert!(!policy(true).allows("p/runner"));
        assert!(!policy(false).allows("p/helper"));
    }


    #[test]
    fn load_reads_the_global_scope_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agent");
        assert!(matches!(
            load(dir.path(), &agent_dir),
            DaemonAllowlist::Unrestricted
        ));
        write_settings(&agent_dir, r#"{"allowedModels": ["prime-inference/*"]}"#);
        assert_eq!(
            load(dir.path(), &agent_dir),
            DaemonAllowlist::Allowed(vec!["prime-inference/*".to_string()])
        );
        // A project scope cannot weaken the global pin: the project dir is
        // the temp dir itself.
        write_settings(
            &dir.path().join(".prime").join("agent"),
            r#"{"allowedModels": []}"#,
        );
        assert_eq!(
            load(dir.path(), &agent_dir),
            DaemonAllowlist::Allowed(vec!["prime-inference/*".to_string()])
        );
    }

    #[test]
    fn load_trims_and_drops_empty_lists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agent");
        write_settings(
            &agent_dir,
            r#"{"allowedModels": ["  prime-inference/internal/*  ", ""]}"#,
        );
        assert_eq!(
            load(dir.path(), &agent_dir),
            DaemonAllowlist::Allowed(vec!["prime-inference/internal/*".to_string()])
        );
        write_settings(&agent_dir, r#"{"allowedModels": [" ", ""]}"#);
        assert!(matches!(
            load(dir.path(), &agent_dir),
            DaemonAllowlist::Unrestricted
        ));
    }

    #[test]
    fn assert_allowed_passes_without_an_allowlist_and_types_the_refusal() {
        assert_allowed(&DaemonAllowlist::Unrestricted, "anything/model").unwrap();
        let allow = DaemonAllowlist::Allowed(vec!["prime-inference/*".to_string()]);
        assert_allowed(&allow, "prime-inference/internal/glm-5.3-fast").unwrap();
        let error = assert_allowed(&allow, "zai/glm-5.3").expect_err("refused");
        let refusal = error
            .downcast_ref::<ModelAllowlistRefusal>()
            .expect("typed refusal");
        assert_eq!(refusal.selector, "zai/glm-5.3");
    }

    #[test]
    fn an_unreadable_settings_document_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("settings.json"), "{ not json").unwrap();
        let state = load(dir.path(), &agent_dir);
        assert!(matches!(state, DaemonAllowlist::Unreadable(_)));
        assert!(!state.allows("prime-inference/mock-1"));
        let error = assert_allowed(&state, "prime-inference/mock-1").expect_err("fail closed");
        let message = error.to_string();
        assert!(message.contains("could not be read"), "{message}");
        assert!(message.contains("fails closed"), "{message}");
        assert!(
            error.downcast_ref::<ModelAllowlistRefusal>().is_none(),
            "fail-closed is not a pattern refusal"
        );
        std::fs::remove_file(agent_dir.join("settings.json")).unwrap();
        let state = load(dir.path(), &agent_dir);
        assert!(matches!(state, DaemonAllowlist::Unrestricted));
    }

    #[test]
    fn a_malformed_allowed_models_value_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("settings.json"),
            r#"{"allowedModels": "prime-inference/*"}"#,
        )
        .unwrap();
        assert!(matches!(
            load(dir.path(), &agent_dir),
            DaemonAllowlist::Unreadable(_)
        ));
        std::fs::write(
            agent_dir.join("settings.json"),
            r#"{"allowedModels": ["prime-inference/*", 42]}"#,
        )
        .unwrap();
        assert!(matches!(
            load(dir.path(), &agent_dir),
            DaemonAllowlist::Unreadable(_)
        ));
        std::fs::write(
            agent_dir.join("settings.json"),
            r#"{"allowedModels": null}"#,
        )
        .unwrap();
        assert!(matches!(
            load(dir.path(), &agent_dir),
            DaemonAllowlist::Unrestricted
        ));
        std::fs::write(agent_dir.join("settings.json"), r#"{"allowedModels": []}"#).unwrap();
        assert!(matches!(
            load(dir.path(), &agent_dir),
            DaemonAllowlist::Unrestricted
        ));
    }

    /// A NON-OBJECT global document (`[]`, `"bad"`) is a corrupted
    /// document, not an absent key: the guard fails closed.
    #[test]
    fn a_non_object_global_document_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        for corrupted in ["[]", r#""bad""#, "42"] {
            std::fs::write(agent_dir.join("settings.json"), corrupted).unwrap();
            let state = load(dir.path(), &agent_dir);
            assert!(
                matches!(state, DaemonAllowlist::Unreadable(_)),
                "{corrupted} must fail closed, got {state:?}"
            );
            let error = assert_allowed(&state, "prime-inference/mock-1").expect_err("closed");
            assert!(
                error.downcast_ref::<ModelAllowlistRefusal>().is_none(),
                "fail-closed is not a pattern refusal"
            );
        }
    }

    #[tokio::test]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "release builds send to the real endpoint"
    )]
    async fn refusal_telemetry_honors_both_opt_outs() {
        let _telemetry = crate::agent_engine::tests::telemetry_opt_in();
        let dir = tempfile::tempdir().expect("tempdir");
        let disabled = ModelRefusalTelemetry::new(dir.path().to_path_buf(), true);
        disabled.note_refused("set_model", "zai/glm-5.3", dir.path());
        assert!(
            disabled.client.lock().unwrap().is_none(),
            "no client when the create command opted out"
        );
        let agent_dir = dir.path().join("agent");
        let settings_gated = ModelRefusalTelemetry::new(agent_dir.clone(), false);
        write_settings(&agent_dir, r#"{"telemetry": {"enabled": false}}"#);
        settings_gated.note_refused("set_model", "zai/glm-5.3", dir.path());
        assert!(
            settings_gated.client.lock().unwrap().is_none(),
            "no client when settings disable telemetry"
        );
        let enabled = ModelRefusalTelemetry::new(dir.path().to_path_buf(), false);
        enabled.note_refused("set_model", "zai/glm-5.3", dir.path());
        assert!(
            enabled.client.lock().unwrap().as_ref().is_some(),
            "client built on first note"
        );
    }

    #[tokio::test]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "release builds send to the real endpoint"
    )]
    async fn refusal_telemetry_dedupes_repeated_resolves() {
        let _telemetry = crate::agent_engine::tests::telemetry_opt_in();
        let dir = tempfile::tempdir().expect("tempdir");
        let telemetry = ModelRefusalTelemetry::new(dir.path().to_path_buf(), false);
        telemetry.note_refused("session_start", "zai/glm-5.3", dir.path());
        assert_eq!(
            telemetry.noted.lock().unwrap().len(),
            1,
            "first refusal noted"
        );
        telemetry.note_refused("session_start", "zai/glm-5.3", dir.path());
        assert_eq!(telemetry.noted.lock().unwrap().len(), 1);
        telemetry.note_refused("session_start", "zai/glm-5.3-flash", dir.path());
        telemetry.note_refused("spawn", "zai/glm-5.3", dir.path());
        assert_eq!(telemetry.noted.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "release builds send to the real endpoint"
    )]
    async fn refusal_telemetry_rebinds_when_the_cwd_moves() {
        let _telemetry = crate::agent_engine::tests::telemetry_opt_in();
        let dir = tempfile::tempdir().expect("tempdir");
        let telemetry = ModelRefusalTelemetry::new(dir.path().join("agent"), false);
        let first = dir.path().join("project-a");
        std::fs::create_dir_all(&first).unwrap();
        telemetry.note_refused("set_model", "zai/glm-5.3", &first);
        assert_eq!(
            telemetry.client.lock().unwrap().as_ref().unwrap().0,
            first,
            "client bound to the live cwd"
        );
        let second = dir.path().join("project-b");
        std::fs::create_dir_all(&second).unwrap();
        telemetry.note_refused("spawn", "zai/glm-5.3", &second);
        assert_eq!(
            telemetry.client.lock().unwrap().as_ref().unwrap().0,
            second,
            "client rebound after the cwd move"
        );
    }
}
