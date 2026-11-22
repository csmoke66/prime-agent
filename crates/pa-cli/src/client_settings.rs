//! The interactive client-settings seam over the run's local settings and harness stores.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pa_core::refinement::{self, HarnessScope};
use pa_core::session_engine::harness_digest::local_harness_dir_for_log;
use pa_core::session::subagent_policy::SessionSubagentPolicy;
use pa_core::settings::ThinkingLevelSetting;
use pa_tui::client_settings::{ChildModelChoices, ChildPolicy, ClientSettings};
use pa_types::ai::ModelThinkingLevel;
use serde_json::Value;

mod subagent;

/// The seam handle the interactive run carries.
#[derive(Clone)]
pub struct CliClientSettings {
    cwd: PathBuf,
    agent_dir: PathBuf,
}

impl CliClientSettings {
    pub fn new(cwd: PathBuf, agent_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self { cwd, agent_dir })
    }

    fn manager(&self) -> pa_core::settings::SettingsManager {
        pa_core::settings::SettingsManager::create(&self.cwd, &self.agent_dir)
    }

    fn track_child_settings_change(&self, setting: &'static str) {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let settings = self.clone();
            runtime.spawn_blocking(move || {
                let client = pa_core::session_engine::telemetry::build_client(
                    &settings.manager(),
                    &settings.agent_dir,
                );
                let mut properties = pa_telemetry::Properties::new();
                properties.set("setting", serde_json::json!(setting));
                client.track("child settings changed", properties);
            });
        }
    }
}

fn local_harness_dir(session_file: &str) -> Result<PathBuf> {
    local_harness_dir_for_log(Path::new(session_file))
        .with_context(|| format!("Cannot locate the harness store for session {session_file}"))
}

macro_rules! setting {
    ($get:ident, $set:ident, $getter:ident, $setter:ident, $ty:ty) => {
        fn $get(&self) -> $ty {
            self.manager().$getter()
        }

        fn $set(&self, value: $ty) -> Result<()> {
            self.manager().$setter(value)
        }
    };
}

macro_rules! str_setting {
    ($get:ident, $set:ident, $getter:ident, $setter:ident) => {
        fn $get(&self) -> String {
            self.manager().$getter().to_string()
        }

        fn $set(&self, value: &str) -> Result<()> {
            self.manager().$setter(value)
        }
    };
}

impl ClientSettings for CliClientSettings {
    fn child_policy(&self, session_file: &str) -> Result<ChildPolicy> {
        Ok(subagent::RootChildPolicy::load(Path::new(session_file), &self.cwd, &self.agent_dir)?
            .summary())
    }

    fn child_model_choices(&self, session_file: &str) -> Result<ChildModelChoices> {
        Ok(subagent::RootChildPolicy::load(Path::new(session_file), &self.cwd, &self.agent_dir)?
            .model_choices(&self.agent_dir))
    }

    fn set_child_models(&self, session_file: &str, models: Option<Vec<String>>) -> Result<()> {
        let owner = subagent::owner_log(Path::new(session_file))?;
        SessionSubagentPolicy::set_model_selectors(&owner, models)?;
        self.track_child_settings_change("models");
        Ok(())
    }

    fn set_child_effort(&self, session_file: &str, effort: Option<ModelThinkingLevel>) -> Result<()> {
        let owner = subagent::owner_log(Path::new(session_file))?;
        SessionSubagentPolicy::set_effort(&owner, effort.map(ThinkingLevelSetting::from_model_level))?;
        self.track_child_settings_change("effort");
        Ok(())
    }

    fn harness_refinements(&self) -> bool {
        self.manager().get_harness_refinements()
    }

    fn set_harness_refinements(&self, enabled: bool) -> Result<()> {
        let mut manager = self.manager();
        manager.set_harness_refinements(enabled)?;
        // Adoption bookkeeping stays off the UI input/paint path.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let agent_dir = self.agent_dir.clone();
            runtime.spawn_blocking(move || {
                let client = pa_core::session_engine::telemetry::build_client(&manager, &agent_dir);
                let mut properties = pa_telemetry::Properties::new();
                properties.set("enabled", serde_json::json!(enabled));
                client.track("harness refinements changed", properties);
            });
        }
        Ok(())
    }

    fn provider_block_delivery(&self) -> String {
        self.manager().get_provider_block_delivery().id().to_owned()
    }

    fn set_provider_block_delivery(&self, mode: &str) -> Result<()> {
        let mode: pa_core::settings::ProviderBlockDelivery =
            serde_json::from_value(Value::String(mode.to_owned()))
                .map_err(|error| anyhow::anyhow!("Invalid provider-block delivery mode: {error}"))?;
        self.manager().set_provider_block_delivery(mode)
    }

    fn provider_block_target_session(&self) -> Option<String> {
        self.manager()
            .get_provider_block_target_session()
            .map(str::to_owned)
    }

    fn set_provider_block_target_session(&self, target: &str) -> Result<()> {
        self.manager()
            .set_provider_block_target_session(target.to_owned())
    }

    fn harness_entries(&self, session_file: Option<&str>) -> Result<Vec<Value>> {
        let mut rows = refinement::read_harness_entries(
            &refinement::get_global_harness_state_dir(&self.agent_dir),
            HarnessScope::Global,
        )?;
        if let Some(session_file) = session_file {
            rows.extend(refinement::read_harness_entries(
                &local_harness_dir(session_file)?,
                HarnessScope::Local,
            )?);
        }
        Ok(rows)
    }

    fn delete_harness_entry(&self, session_file: Option<&str>, entry: &Value) -> Result<()> {
        let dir = match entry.get("scope").and_then(Value::as_str) {
            Some("global") => refinement::get_global_harness_state_dir(&self.agent_dir),
            Some("local") => local_harness_dir(
                session_file.context("A persisted session is required to delete a local entry")?,
            )?,
            _ => anyhow::bail!("Invalid scope for the selected harness entry"),
        };
        refinement::delete_harness_entry_from_store(&dir, entry)
    }

    fn compaction_threshold_percent(&self) -> f64 {
        self.manager().get_compaction_threshold_percent()
    }

    fn set_compaction_threshold_percent(&self, percent: f64) -> Result<()> {
        self.manager().set_compaction_threshold_percent(percent)?;
        let settings = self.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let manager = settings.manager();
                if crate::mode::telemetry_disabled(&manager) {
                    return;
                }
                let client =
                    pa_core::session_engine::telemetry::build_client(&manager, &settings.agent_dir);
                let mut properties = pa_telemetry::base_properties("interactive");
                properties.set("threshold_percent", serde_json::json!(percent));
                client.track("autocompact threshold changed", properties);
                let _ = client.shutdown().await;
            });
        }
        Ok(())
    }

    fn web_search_provider(&self) -> String {
        self.manager().get_web_search_provider().id().to_string()
    }

    fn set_web_search_provider(&self, provider: &str) -> Result<()> {
        let parsed: pa_core::settings::WebSearchProvider = serde_json::from_value(
            serde_json::Value::String(provider.to_string()),
        )
        .map_err(|error| anyhow::anyhow!("Invalid web search provider \"{provider}\": {error}"))?;
        self.manager().set_web_search_provider(parsed)?;
        let settings = self.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let manager = settings.manager();
                if crate::mode::telemetry_disabled(&manager) {
                    return;
                }
                let client =
                    pa_core::session_engine::telemetry::build_client(&manager, &settings.agent_dir);
                let mut properties = pa_telemetry::base_properties("interactive");
                properties.set("provider", serde_json::json!(parsed.id()));
                client.track("web search provider changed", properties);
                let _ = client.shutdown().await;
            });
        }
        Ok(())
    }

    fn theme(&self) -> Option<String> {
        self.manager().get_theme().map(str::to_string)
    }

    fn set_theme(&self, theme: &str) -> Result<()> {
        self.manager().set_theme(theme.to_string())
    }

    fn default_service_tier(&self) -> String {
        // The wire name of the persisted default tier; "default" when
        // unset or unreadable.
        serde_json::to_value(self.manager().get_default_service_tier())
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "default".to_string())
    }

    fn set_default_service_tier(&self, tier: &str) -> Result<()> {
        let parsed: pa_types::ai::ServiceTier =
            serde_json::from_value(serde_json::Value::String(tier.to_string()))
                .map_err(|error| anyhow::anyhow!("Invalid service tier \"{tier}\": {error}"))?;
        self.manager().set_default_service_tier(parsed)
    }

    setting!(
        show_images,
        set_show_images,
        get_show_images,
        set_show_images,
        bool
    );
 
    setting!(
        clear_on_shrink,
        set_clear_on_shrink,
        get_clear_on_shrink,
        set_clear_on_shrink,
        bool
    );
    setting!(
        show_terminal_progress,
        set_show_terminal_progress,
        get_show_terminal_progress,
        set_show_terminal_progress,
        bool
    );
    setting!(
        image_auto_resize,
        set_image_auto_resize,
        get_image_auto_resize,
        set_image_auto_resize,
        bool
    );
    setting!(
        block_images,
        set_block_images,
        get_block_images,
        set_block_images,
        bool
    );

    fn image_model(&self) -> Option<String> {
        self.manager().get_image_model()
    }
    setting!(
        enable_skill_commands,
        set_enable_skill_commands,
        get_enable_skill_commands,
        set_enable_skill_commands,
        bool
    );
    setting!(
        enable_builtin_skills,
        set_enable_builtin_skills,
        get_enable_builtin_skills,
        set_enable_builtin_skills,
        bool
    );
    setting!(
        show_hardware_cursor,
        set_show_hardware_cursor,
        get_show_hardware_cursor,
        set_show_hardware_cursor,
        bool
    );
    setting!(
        emoji_shortcodes,
        set_emoji_shortcodes,
        get_emoji_shortcodes,
        set_emoji_shortcodes,
        bool
    );
    setting!(
        editor_padding_x,
        set_editor_padding_x,
        get_editor_padding_x,
        set_editor_padding_x,
        u64
    );
    setting!(
        autocomplete_max_visible,
        set_autocomplete_max_visible,
        get_autocomplete_max_visible,
        set_autocomplete_max_visible,
        u64
    );
    setting!(
        quiet_startup,
        set_quiet_startup,
        get_quiet_startup,
        set_quiet_startup,
        bool
    );
    str_setting!(
        idle_eviction_minutes,
        set_idle_eviction_minutes,
        get_idle_eviction_minutes,
        set_idle_eviction_minutes
    );
    str_setting!(
        mermaid_rendering_mode,
        set_mermaid_rendering_mode,
        get_mermaid_rendering_mode,
        set_mermaid_rendering_mode
    );
    str_setting!(
        tree_filter_mode,
        set_tree_filter_mode,
        get_tree_filter_mode,
        set_tree_filter_mode
    );
    str_setting!(
        chat_detail,
        set_chat_detail,
        get_chat_detail,
        set_chat_detail
    );
    setting!(
        factory_enabled,
        set_factory_enabled,
        get_factory_enabled,
        set_factory_enabled,
        bool
    );
    setting!(
        warnings_anthropic_extra_usage,
        set_warnings_anthropic_extra_usage,
        get_warnings_anthropic_extra_usage,
        set_warnings_anthropic_extra_usage,
        bool
    );

    /// `updateChannel`: the settings enum's wire value; unset reads as
    /// `None`.
    fn update_channel(&self) -> Option<String> {
        let channel = self.manager().get_update_channel()?;
        Some(
            match channel {
                pa_core::settings::UpdateChannel::Stable => "stable",
                pa_core::settings::UpdateChannel::Nightly => "nightly",
            }
            .to_string(),
        )
    }

    fn set_update_channel(&self, channel: &str) -> Result<()> {
        let channel = match channel {
            "stable" => pa_core::settings::UpdateChannel::Stable,
            "nightly" => pa_core::settings::UpdateChannel::Nightly,
            _ => anyhow::bail!("unknown update channel: {channel}"),
        };
        self.manager().set_update_channel(channel)
    }

    fn effective_update_channel(&self, version: &str) -> String {
        let preferred = self
            .manager()
            .get_update_channel()
            .map(|channel| match channel {
                pa_core::settings::UpdateChannel::Stable => {
                    pa_core::update::version::UpdateChannel::Stable
                }
                pa_core::settings::UpdateChannel::Nightly => {
                    pa_core::update::version::UpdateChannel::Nightly
                }
            });
        pa_core::update::version::resolve_update_channel(version, preferred)
            .wire_name()
            .to_string()
    }

    fn telemetry_status(&self) -> String {
        pa_core::session_engine::telemetry::telemetry_status_text(&self.manager(), &self.agent_dir)
    }

    fn telemetry_enabled(&self) -> bool {
        self.manager().get_telemetry_enabled()
    }

    fn set_telemetry_enabled(&self, enabled: bool) -> Result<String> {
        pa_core::session_engine::telemetry::set_telemetry_enabled_text(
            &mut self.manager(),
            &self.agent_dir,
            enabled,
        )
    }

    fn telemetry_notice_due(&self) -> bool {
        // TS agent-session-services: telemetry enabled, onboarding
        // already shown (a first interactive launch belongs to the
        // onboarding screen; the notice surfaces on the next launch),
        // and the notice not yet shown.
        let manager = self.manager();
        pa_core::session_engine::telemetry::telemetry_switch(&manager).enabled()
            && manager.get_onboarding_shown()
            && !manager.get_telemetry_notice_shown()
    }

    fn set_telemetry_notice_shown(&self) -> Result<()> {
        self.manager().set_telemetry_notice_shown(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn harness_fixture(source: &Path) -> Value {
        let selected = json!({
            "id": "shared",
            "kind": "memory",
            "title": "Selected memory",
            "content": "A useful note",
            "path": "notes",
            "reference": {"path": source},
            "arguments": {"key": "value"},
            "metadata": {"custom": [1, 2]},
            "source": "agent",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "version": 1,
            "future_field": {"enabled": true}
        });
        let mut kept = selected.clone();
        kept["id"] = json!("kept");
        json!({
            "schema": 1,
            "entries": {
                "memory": { "shared": selected, "kept": kept },
                "future_kind": { "untouched": {"future_field": 42} }
            },
            "refinements": [{
                "id": "refinement",
                "trigger": "test",
                "changes": ["create memory"],
                "evidence": "",
                "outcome": "",
                "created_at": "2026-01-01T00:00:00Z",
                "future_field": true
            }],
            "future_top_level_field": ["untouched"]
        })
    }

    fn write_harness_fixture(path: &Path, state: &Value) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, state.to_string()).unwrap();
    }

    #[test]
    fn harness_seam_deletes_only_the_selected_store_row() {
        let dir = tempfile::tempdir().unwrap();
        let agent_dir = dir.path().join("agent");
        let settings = CliClientSettings::new(dir.path().to_path_buf(), agent_dir.clone());
        let parent_artifacts = agent_dir.join("session-artifacts/parent");
        let parent_file = agent_dir.join("sessions/parent.jsonl");
        let parent_file = parent_file.to_str().unwrap();
        let child_file = parent_artifacts.join("rlm-child/child-log.jsonl");
        let child_file = child_file.to_str().unwrap();
        let child_harness = parent_artifacts.join("session-artifacts/child-log/harness");
        let child_path = child_harness.join("harness_state.json");
        let parent_path = parent_artifacts.join("harness/harness_state.json");
        let global_path = agent_dir.join("harness/harness_state.json");
        let source_file = dir.path().join("source.md");
        std::fs::write(&source_file, "referenced source").unwrap();

        let global = harness_fixture(&source_file);
        let mut child = global.clone();
        // The store, not a field claiming a different scope, identifies the row.
        child["entries"]["memory"]["shared"]["scope"] = json!("global");
        write_harness_fixture(&global_path, &global);
        write_harness_fixture(&parent_path, &global);
        write_harness_fixture(&child_path, &child);
        let history = child_harness.join("refinement_history.jsonl");
        std::fs::write(&history, "{\"id\":\"history\"}\n").unwrap();

        let rows = settings.harness_entries(Some(child_file)).unwrap();
        let global_rows = settings.harness_entries(None).unwrap();
        let parent_rows = settings.harness_entries(Some(parent_file)).unwrap();
        assert_eq!(parent_rows.len(), 4);
        assert_eq!(
            parent_rows
                .iter()
                .filter(|entry| entry["scope"] == "local")
                .count(),
            2
        );
        assert_eq!(rows.len(), 4);
        assert_eq!(global_rows.len(), 2);
        let selected = rows
            .iter()
            .find(|entry| entry["scope"] == "local" && entry["id"] == "shared")
            .unwrap();
        let mut expected = child["entries"]["memory"]["shared"].clone();
        expected["scope"] = json!("local");
        assert_eq!(selected, &expected);
        assert!(settings.delete_harness_entry(None, selected).is_err());

        // Edits to a neighboring record after listing must survive deletion.
        child["entries"]["memory"]["kept"]["future_field"] = json!("updated");
        write_harness_fixture(&child_path, &child);
        settings
            .delete_harness_entry(Some(child_file), selected)
            .unwrap();

        child["entries"]["memory"]
            .as_object_mut()
            .unwrap()
            .remove("shared");
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&child_path).unwrap()).unwrap(),
            child
        );
        assert_eq!(
            std::fs::read_to_string(&global_path).unwrap(),
            global.to_string()
        );
        assert_eq!(
            std::fs::read_to_string(&parent_path).unwrap(),
            global.to_string()
        );
        assert_eq!(
            std::fs::read_to_string(&source_file).unwrap(),
            "referenced source"
        );
        assert_eq!(
            std::fs::read_to_string(&history).unwrap(),
            "{\"id\":\"history\"}\n"
        );
        let rows = settings.harness_entries(Some(child_file)).unwrap();
        assert_eq!(rows.len(), 3);
        let selected = rows
            .iter()
            .find(|entry| entry["scope"] == "global" && entry["id"] == "shared")
            .unwrap();
        settings
            .delete_harness_entry(Some(child_file), selected)
            .unwrap();
        assert!(!settings
            .harness_entries(Some(child_file))
            .unwrap()
            .iter()
            .any(|entry| entry["id"] == "shared"));
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&child_path).unwrap()).unwrap(),
            child,
        );
    }

    #[test]
    fn harness_seam_rejects_a_stale_snapshot_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let settings = CliClientSettings::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let path = dir.path().join("harness/harness_state.json");
        let original = harness_fixture(&dir.path().join("source.md"));
        write_harness_fixture(&path, &original);
        let rows = settings.harness_entries(None).unwrap();
        let selected = rows.iter().find(|entry| entry["id"] == "shared").unwrap();

        for (field, value) in [
            ("version", json!(2)),
            ("content", json!("changed without a version bump")),
            ("future_field", json!({"enabled": false})),
        ] {
            let mut changed = original.clone();
            changed["entries"]["memory"]["shared"][field] = value;
            write_harness_fixture(&path, &changed);
            let before = std::fs::read(&path).unwrap();
            let error = settings.delete_harness_entry(None, selected).unwrap_err();
            assert!(error.to_string().contains("changed or was removed"));
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }

        std::fs::remove_file(&path).unwrap();
        let error = settings.delete_harness_entry(None, selected).unwrap_err();
        assert!(error.to_string().contains("changed or was removed"));
        assert!(!path.exists());
    }

    #[test]
    fn harness_seam_distinguishes_an_absent_store_from_malformed_data() {
        let dir = tempfile::tempdir().unwrap();
        let settings = CliClientSettings::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let harness_dir = dir.path().join("harness");
        let path = harness_dir.join("harness_state.json");
        assert!(settings.harness_entries(None).unwrap().is_empty());
        assert!(!harness_dir.exists());
        std::fs::create_dir(&harness_dir).unwrap();

        for malformed in [
            "}",
            r#"{"entries":[]}"#,
            r#"{"entries":{"memory":{"bad":1}}}"#,
        ] {
            std::fs::write(&path, malformed).unwrap();
            let error = settings.harness_entries(None).unwrap_err();
            assert!(error.to_string().contains("malformed harness state"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), malformed);
        }

        // An existing snapshot must not overwrite a store that became malformed.
        write_harness_fixture(&path, &harness_fixture(&dir.path().join("source.md")));
        let rows = settings.harness_entries(None).unwrap();
        let malformed = br#"{"entries":{"memory":[]}}"#;
        std::fs::write(&path, malformed).unwrap();
        let error = settings.delete_harness_entry(None, &rows[0]).unwrap_err();
        assert!(error.to_string().contains("malformed harness state"));
        assert_eq!(std::fs::read(&path).unwrap().as_slice(), malformed);
    }

    /// The seam over the real pa-core settings store.
    #[test]
    fn seam_round_trips_through_the_settings_store() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let agent_dir = dir.path().join("agent");
        std::fs::create_dir_all(&agent_dir).expect("agent dir");
        let settings = CliClientSettings::new(dir.path().to_path_buf(), agent_dir.clone());

        assert!(settings.show_images());
        assert_eq!(settings.provider_block_delivery(), "show");
        assert!(!settings.quiet_startup());
        assert_eq!(settings.idle_eviction_minutes(), "90");
        assert_eq!(settings.mermaid_rendering_mode(), "streaming");
        assert_eq!(settings.tree_filter_mode(), "user-only");
        assert!(settings.warnings_anthropic_extra_usage());

        settings.set_theme("dark").expect("theme");
        settings.set_idle_eviction_minutes("off").expect("idle");
        settings.set_tree_filter_mode("all").expect("tree filter");
        settings.set_show_images(false).expect("show images");
        settings
            .set_provider_block_target_session("hr-session")
            .expect("provider block target");
        settings
            .set_provider_block_delivery("route")
            .expect("provider block delivery");

        assert_eq!(settings.theme().as_deref(), Some("dark"));
        assert_eq!(settings.idle_eviction_minutes(), "off");
        assert_eq!(settings.tree_filter_mode(), "all");
        assert!(!settings.show_images());
        assert_eq!(settings.provider_block_delivery(), "route");
        assert_eq!(
            settings.provider_block_target_session().as_deref(),
            Some("hr-session")
        );

        let content =
            std::fs::read_to_string(agent_dir.join("settings.json")).expect("settings file");
        let value: serde_json::Value = serde_json::from_str(&content).expect("parse");
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["idleEvictionMinutes"], "off");
        assert_eq!(value["treeFilterMode"], "all");
        assert_eq!(value["providerBlockDelivery"], "route");
        assert_eq!(value["providerBlockTargetSession"], "hr-session");

        // The web search provider defaults to Serper and persists the choice.
        assert_eq!(settings.web_search_provider(), "serper");
        settings.set_web_search_provider("eco").expect("provider");
        assert_eq!(settings.web_search_provider(), "eco");
        let content =
            std::fs::read_to_string(agent_dir.join("settings.json")).expect("settings file");
        let value: serde_json::Value = serde_json::from_str(&content).expect("parse");
        assert_eq!(value["webSearchProvider"], "eco");

        // The update channel starts unset (the version infers it) and
        // pins through the /nightly off path's seam.
        assert_eq!(settings.update_channel(), None);
        settings.set_update_channel("stable").expect("channel");
        assert_eq!(settings.update_channel().as_deref(), Some("stable"));
        let content =
            std::fs::read_to_string(agent_dir.join("settings.json")).expect("settings file");
        let value: serde_json::Value = serde_json::from_str(&content).expect("parse");
        assert_eq!(value["updateChannel"], "stable");
        // ...and the /nightly on path's.
        settings.set_update_channel("nightly").expect("channel");
        assert_eq!(settings.update_channel().as_deref(), Some("nightly"));

        // The factory's opt-in gate (`/factory on|off|status`): unset reads
        // as disabled (the default off), and the write persists the exact
        // nested-camelCase key the kernel's gate and the daemon's lane
        // advertisement read -- over the same document, leaving the other
        // keys alone.
        assert!(!settings.factory_enabled());
        settings.set_factory_enabled(true).expect("factory enabled");
        assert!(settings.factory_enabled());
        let content =
            std::fs::read_to_string(agent_dir.join("settings.json")).expect("settings file");
        let value: serde_json::Value = serde_json::from_str(&content).expect("parse");
        assert_eq!(value["factory"]["enabled"], true);
        assert_eq!(value["theme"], "dark", "the write leaves the other keys");
        settings
            .set_factory_enabled(false)
            .expect("factory disabled");
        assert!(!settings.factory_enabled());
        let content =
            std::fs::read_to_string(agent_dir.join("settings.json")).expect("settings file");
        let value: serde_json::Value = serde_json::from_str(&content).expect("parse");
        assert_eq!(value["factory"]["enabled"], false);
    }
}
