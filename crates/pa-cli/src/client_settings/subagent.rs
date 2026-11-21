use std::borrow::Cow;
use std::path::Path;

use anyhow::{Context, Result};
use pa_core::auth::AuthStorage;
use pa_core::models::ModelRegistry;
use pa_core::session::manager::read_session_header;
use pa_core::session::subagent_policy::SessionSubagentPolicy;
use pa_daemon::model_allowlist::{self, DaemonAllowlist};
use pa_tui::client_settings::{
    ChildModelChoice, ChildModelChoices, ChildPolicy, InheritedChildModels,
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
        // The same authenticated snapshot as `rlm.find_models`, with no current policy filter:
        // an override may replace the inherited list rather than only narrowing it.
        let registry =
            ModelRegistry::create(AuthStorage::create(agent_dir), agent_dir.join("models.json"));
        let mut models: Vec<_> = registry
            .get_rlm_searchable_models()
            .into_iter()
            .map(|model| {
                let selector = format!("{}/{}", model.provider, model.id);
                let inherited = self.global.allows(&selector);
                let allowed = override_policy
                    .as_ref()
                    .map_or(inherited, |policy| policy.allows(&selector));
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
        ChildModelChoices { policy, models }
    }
}

#[cfg(test)]
mod tests {
    use super::super::CliClientSettings;
    use pa_tui::client_settings::ClientSettings;
    use pa_types::ai::ModelThinkingLevel;
    use serde_json::json;

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
