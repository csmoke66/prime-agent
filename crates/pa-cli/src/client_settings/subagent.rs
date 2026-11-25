use std::borrow::Cow;
use std::path::Path;

use anyhow::{Context, Result};
use pa_core::auth::AuthStorage;
use pa_core::models::ModelRegistry;
use pa_core::session::manager::read_session_header;
use pa_core::session::subagent_policy::SessionSubagentPolicy;
use pa_daemon::model_allowlist::{self, DaemonAllowlist};
use pa_tui::client_settings::{
    ChildModelChoice, ChildModelChoices, ChildPolicy, GlobalModelChoices, InheritedChildModels,
};

pub(super) fn owner_log(file: &Path) -> Result<Cow<'_, Path>> {
    let header = read_session_header(file).context("Cannot read the attached session header")?;
    let depth = u32::try_from(header.rlm_depth.unwrap_or(0))?;
    SessionSubagentPolicy::owner_log(file, depth)
}

pub(super) struct RootChildPolicy {
    policy: SessionSubagentPolicy,
    global: DaemonAllowlist,
}

impl RootChildPolicy {
    pub(super) fn load(file: &Path, cwd: &Path, agent_dir: &Path) -> Result<Self> {
        Ok(Self {
            policy: SessionSubagentPolicy::load(&owner_log(file)?)?,
            global: model_allowlist::load(cwd, agent_dir),
        })
    }

    pub(super) fn summary(&self) -> ChildPolicy {
        ChildPolicy {
            model_rules: self.policy.allowed_models.as_ref().map(Vec::len),
            inherited: match &self.global {
                DaemonAllowlist::Unrestricted => InheritedChildModels::All,
                DaemonAllowlist::Allowed(rules) | DaemonAllowlist::Selected(rules) => {
                    InheritedChildModels::Patterns(rules.len())
                }
                DaemonAllowlist::Unreadable(_) => InheritedChildModels::Unreadable,
            },
            effort: self.policy.effort.map(|level| level.model_level()),
        }
    }

    pub(super) fn model_choices(self, agent_dir: &Path) -> ChildModelChoices {
        let policy = self.summary();
        let override_policy = self.policy.allowed_models.map(|models| {
            DaemonAllowlist::session_override(models, self.policy.exact_model_selectors)
        });
        let registry = model_registry(agent_dir);
        // Do not filter by the current policy: an override may replace it rather than narrow it.
        let models = model_choices(
            registry.get_rlm_searchable_models(),
            &self.global,
            override_policy.as_ref(),
        );
        ChildModelChoices { policy, models }
    }
}

pub(super) fn global_model_choices(cwd: &Path, agent_dir: &Path) -> Result<GlobalModelChoices> {
    let policy = model_allowlist::load(cwd, agent_dir);
    if let DaemonAllowlist::Unreadable(error) = &policy {
        anyhow::bail!("{error}");
    }
    let registry = model_registry(agent_dir);
    if let Some(error) = registry.get_error() {
        anyhow::bail!("{error}");
    }
    let available = registry.get_available();
    let catalog = registry.get_all().iter().filter(|model| {
        !pa_core::models::is_private_prime_inference_model(model) || available.contains(model)
    });
    let mut models = model_choices(catalog, &policy, None);
    if let DaemonAllowlist::Allowed(rules) = &policy {
        // Exact selectors can be temporarily absent from the catalog (e.g. a custom provider).
        for rule in rules {
            if !rule.contains(['*', '?', '['])
                && rule.contains('/')
                && !models
                    .iter()
                    .any(|model| model.selector.eq_ignore_ascii_case(rule))
            {
                models.push(ChildModelChoice {
                    selector: rule.clone(),
                    name: "Not in catalog".to_owned(),
                    allowed: true,
                    inherited: true,
                });
            }
        }
        models.sort_unstable_by(|a, b| a.selector.cmp(&b.selector));
    }
    Ok(GlobalModelChoices {
        unrestricted: policy == DaemonAllowlist::Unrestricted,
        models,
    })
}

fn model_registry(agent_dir: &Path) -> ModelRegistry {
    ModelRegistry::create(
        AuthStorage::create(agent_dir),
        agent_dir.join("models.json"),
    )
}

fn model_choices<'a>(
    catalog: impl IntoIterator<Item = &'a pa_types::ai::Model>,
    global: &DaemonAllowlist,
    override_policy: Option<&DaemonAllowlist>,
) -> Vec<ChildModelChoice> {
    let mut models: Vec<_> = catalog
        .into_iter()
        .map(|model| {
            let selector = format!("{}/{}", model.provider, model.id);
            let inherited = global.allows(&selector);
            let allowed = override_policy.map_or(inherited, |policy| policy.allows(&selector));
            ChildModelChoice {
                selector,
                name: model.name.clone(),
                allowed,
                inherited,
            }
        })
        .collect();
    models.sort_unstable_by(|a, b| a.selector.cmp(&b.selector));
    models.dedup_by(|a, b| a.selector == b.selector);
    models
}

#[cfg(test)]
mod tests {
    use super::super::CliClientSettings;
    use pa_tui::client_settings::ClientSettings;
    use pa_types::ai::ModelThinkingLevel;
    use serde_json::json;

    #[test]
    fn global_model_editor_lists_blocked_models_and_persists_only_the_global_list() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"allowedModels":["fixture/alpha", "missing/old"], "unrelated":42}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("models.json"),
            json!({"providers": {"fixture": {
                "baseUrl": "http://localhost:9", "api": "openai-completions", "apiKey": "test-key",
                "models": [
                    {"id": "alpha", "name": "Alpha", "contextWindow": 1000, "maxTokens": 100},
                    {"id": "beta", "name": "Beta", "contextWindow": 1000, "maxTokens": 100}
                ]
            }}})
            .to_string(),
        )
        .unwrap();
        let settings = CliClientSettings::new(dir.to_owned(), dir.to_owned());
        let choices = settings.global_model_choices().unwrap();
        let checkmarks: Vec<_> = choices
            .models
            .iter()
            .filter(|entry| {
                entry.selector.starts_with("fixture/") || entry.selector == "missing/old"
            })
            .map(|entry| (entry.selector.as_str(), entry.allowed))
            .collect();
        assert_eq!(
            checkmarks,
            [
                ("fixture/alpha", true),
                ("fixture/beta", false),
                ("missing/old", true)
            ]
        );

        settings
            .set_global_models(vec!["fixture/beta".into(), "missing/old".into()])
            .unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(
            saved,
            json!({"allowedModels": ["fixture/beta", "missing/old"], "unrelated": 42})
        );
        let policy = pa_daemon::model_allowlist::load(dir, dir);
        assert!(!policy.allows("fixture/alpha"));
        assert!(policy.allows("fixture/beta"));
    }

    #[test]
    fn child_settings_resolve_catalog_patterns_and_persist_only_on_the_family_root() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let logs = dir.join("sessions");
        std::fs::create_dir(&logs).unwrap();
        let a = logs.join("a.jsonl");
        let b = logs.join("b.jsonl");
        let child = logs.join("child.jsonl");
        for (file, depth) in [(&a, 0), (&b, 0), (&child, 1)] {
            std::fs::write(
                file,
                json!({
                    "type": "session", "id": file.file_stem().unwrap().to_str().unwrap(),
                    "cwd": "/", "timestamp": "now", "rlmDepth": depth, "parentSession": a,
                })
                .to_string(),
            )
            .unwrap();
        }
        let global = r#"{"allowedModels":["selected/*"]}"#;
        std::fs::write(dir.join("settings.json"), global).unwrap();
        std::fs::write(
            dir.join("models.json"),
            json!({
                "providers": {
                    "selected": {
                        "baseUrl": "http://localhost:9",
                        "apiKey": "test-key",
                        "api": "openai-completions",
                        "models": [
                            { "id": "one", "name": "One", "contextWindow": 1000, "maxTokens": 100 }
                        ]
                    },
                    "other": {
                        "baseUrl": "http://localhost:9",
                        "apiKey": "test-key",
                        "api": "openai-completions",
                        "models": [
                            { "id": "selected/one", "name": "Other One", "contextWindow": 1000, "maxTokens": 100 }
                        ]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let settings = CliClientSettings::new(dir.to_owned(), dir.to_owned());
        let a = a.to_str().unwrap();
        let b = b.to_str().unwrap();
        let child = child.to_str().unwrap();
        let checks = |file| {
            settings
                .child_model_choices(file)
                .unwrap()
                .models
                .into_iter()
                .filter(|model| matches!(model.selector.as_str(), "other/selected/one" | "selected/one"))
                .map(|model| (model.selector, model.allowed))
                .collect::<Vec<_>>()
        };
        let inherited = vec![("other/selected/one".into(), true), ("selected/one".into(), true)];
        assert_eq!(checks(child), inherited);
        settings
            .set_child_models(child, Some(vec!["selected/one".into()]))
            .unwrap();
        settings
            .set_child_effort(a, Some(ModelThinkingLevel::High))
            .unwrap();
        assert_eq!(
            checks(a),
            [("other/selected/one".into(), false), ("selected/one".into(), true)]
        );
        assert_eq!(settings.child_policy(child).unwrap().effort, Some(ModelThinkingLevel::High));
        assert_eq!(checks(b), inherited);
        assert_eq!(settings.child_policy(b).unwrap().effort, None);
        settings.set_child_models(a, Some(Vec::new())).unwrap();
        assert_eq!(settings.child_policy(child).unwrap().model_rules, Some(0));
        settings.set_child_models(a, None).unwrap();
        settings.set_child_effort(child, None).unwrap();
        assert_eq!(settings.child_policy(child).unwrap(), settings.child_policy(b).unwrap());
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.json")).unwrap(),
            global
        );
    }
}
