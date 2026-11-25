//! The client-settings seam: pa-tui depends on pa-types only, so
//! interactive commands that read or persist user settings call this
//! trait; the composition root (pa-cli) implements it over the local stores.

use anyhow::Result;
use pa_types::ai::ModelThinkingLevel;
use serde_json::Value;

/// What an attached session inherits from the global `allowedModels` setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InheritedChildModels {
    All,
    Patterns(usize),
    Unreadable,
}

/// The policy of the attached session's root, before loading the model catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildPolicy {
    /// None inherits, zero explicitly allows all, other counts restrict.
    pub model_rules: Option<usize>,
    pub inherited: InheritedChildModels,
    pub effort: Option<ModelThinkingLevel>,
}

/// One model choice and its membership in the current and inherited policies.
#[derive(Debug)]
pub struct ChildModelChoice {
    pub selector: String,
    pub name: String,
    pub allowed: bool,
    pub inherited: bool,
}

#[derive(Debug)]
pub struct ChildModelChoices {
    pub policy: ChildPolicy,
    pub models: Vec<ChildModelChoice>,
}

/// Global allowlist state and checkmarks, without a session override.
#[derive(Debug)]
pub struct GlobalModelChoices {
    pub unrestricted: bool,
    pub models: Vec<ChildModelChoice>,
}

pub trait ClientSettings: Send + Sync {
    /// Load model choices without filtering out models blocked by the global policy.
    fn global_model_choices(&self) -> Result<GlobalModelChoices> {
        anyhow::bail!("Global model settings are unavailable")
    }

    /// Save the global model list; an empty list allows all models.
    fn set_global_models(&self, _models: Vec<String>) -> Result<()> {
        anyhow::bail!("Global model settings are unavailable")
    }

    /// Read the child policy shared by this attached session's root family.
    fn child_policy(&self, _session_file: &str) -> Result<ChildPolicy> {
        anyhow::bail!("Session subagent settings are unavailable")
    }

    /// Load the authenticated child-model catalog and effective checkmarks.
    fn child_model_choices(&self, _session_file: &str) -> Result<ChildModelChoices> {
        anyhow::bail!("Session subagent settings are unavailable")
    }

    /// Save checked selectors; None inherits the global policy, Some(empty) allows all.
    fn set_child_models(&self, _session_file: &str, _models: Option<Vec<String>>) -> Result<()> {
        anyhow::bail!("Session subagent settings are unavailable")
    }

    /// Save a root-family child effort override; None uses normal spawn/parent resolution.
    fn set_child_effort(
        &self,
        _session_file: &str,
        _effort: Option<ModelThinkingLevel>,
    ) -> Result<()> {
        anyhow::bail!("Session subagent settings are unavailable")
    }

    /// Master harness refinement switch, enabled by default.
    fn harness_refinements(&self) -> bool {
        true
    }
    /// Persist the master harness refinement switch.
    ///
    /// # Errors
    /// Returns an error if the settings store fails or is unavailable.
    fn set_harness_refinements(&self, _enabled: bool) -> Result<()> {
        anyhow::bail!("Harness refinement settings are unavailable")
    }
    /// Sanitized provider-rejection delivery mode: show, silence, or route.
    fn provider_block_delivery(&self) -> String {
        "show".into()
    }
    /// Persist the provider-rejection delivery mode.
    ///
    /// # Errors
    /// Returns an error if the settings store fails or the mode is invalid.
    fn set_provider_block_delivery(&self, _mode: &str) -> Result<()> {
        anyhow::bail!("Provider-rejection delivery settings are unavailable")
    }
    /// Durable selector of the existing session receiving routed reports.
    fn provider_block_target_session(&self) -> Option<String> {
        None
    }
    /// Persist the existing target session selector.
    ///
    /// # Errors
    /// Returns an error if the settings store fails or the selector is invalid.
    fn set_provider_block_target_session(&self, _target: &str) -> Result<()> {
        anyhow::bail!("Provider-rejection target settings are unavailable")
    }

    /// Load raw harness entries from the global and active session's local stores.
    ///
    /// # Errors
    /// Returns an error if the harness store fails or is unavailable.
    fn harness_entries(&self, _session_file: Option<&str>) -> Result<Vec<Value>> {
        anyhow::bail!("Harness storage is unavailable")
    }

    /// Delete the selected entry from the store it came from if it is unchanged.
    ///
    /// # Errors
    /// Returns an error if the selection is stale or the harness store fails or is unavailable.
    fn delete_harness_entry(&self, _session_file: Option<&str>, _entry: &Value) -> Result<()> {
        anyhow::bail!("Harness storage is unavailable")
    }

    /// `webSearchProvider` (default `serper`): the service the
    /// websearch skill queries in new sessions.
    fn web_search_provider(&self) -> String {
        "serper".to_string()
    }
    /// Persist the web search provider globally.
    ///
    /// # Errors
    /// Returns an error for an unknown provider or a failed or unavailable store.
    fn set_web_search_provider(&self, _provider: &str) -> Result<()> {
        anyhow::bail!("Web search settings are unavailable")
    }

    /// Automatic compaction percentage ceiling; output headroom may trigger earlier.
    fn compaction_threshold_percent(&self) -> f64;
    /// Persist the percentage globally.
    ///
    /// # Errors
    /// Returns an error for invalid percentages or failed persistence.
    fn set_compaction_threshold_percent(&self, percent: f64) -> Result<()>;
    /// Advisory reminder threshold for live tool results as a percentage of the model window;
    /// zero disables it.
    fn tool_output_reminder_percent(&self) -> u8 {
        pa_types::compaction::DEFAULT_TOOL_OUTPUT_REMINDER_PERCENT
    }
    /// Persist the advisory tool-result reminder percentage globally.
    ///
    /// # Errors
    /// Returns an error for invalid percentages or failed persistence.
    fn set_tool_output_reminder_percent(&self, _percent: u8) -> Result<()> {
        anyhow::bail!("Tool-result reminder settings are unavailable")
    }
    /// `theme` (TS `getTheme`/`setTheme`).
    fn theme(&self) -> Option<String>;
    /// Persists `theme` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_theme(&self, theme: &str) -> Result<()>;
    /// `terminal.showImages` (TS default true).
    fn show_images(&self) -> bool;
    /// Persists `terminal.showImages` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_show_images(&self, enabled: bool) -> Result<()>;
    /// `terminal.clearOnShrink` (TS default false).
    fn clear_on_shrink(&self) -> bool;
    /// Persists `terminal.clearOnShrink` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_clear_on_shrink(&self, enabled: bool) -> Result<()>;
    /// `terminal.showTerminalProgress` (TS default false).
    fn show_terminal_progress(&self) -> bool;
    /// Persists `terminal.showTerminalProgress` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_show_terminal_progress(&self, enabled: bool) -> Result<()>;
    /// `images.autoResize` (TS default true).
    fn image_auto_resize(&self) -> bool;
    /// Persists `images.autoResize` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_image_auto_resize(&self, enabled: bool) -> Result<()>;
    /// `images.blockImages` (TS default false).
    fn block_images(&self) -> bool;
    /// Persists `images.blockImages` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_block_images(&self, blocked: bool) -> Result<()>;
    /// `imageModel` ("provider/model-id" or a bare id; unset by default),
    /// read-only for the paste-time hint.
    fn image_model(&self) -> Option<String>;
    /// `enableSkillCommands` (TS default true).
    fn enable_skill_commands(&self) -> bool;
    /// Persists `enableSkillCommands` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_enable_skill_commands(&self, enabled: bool) -> Result<()>;
    /// `enableBuiltinSkills` (TS default true).
    fn enable_builtin_skills(&self) -> bool;
    /// Persists `enableBuiltinSkills` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_enable_builtin_skills(&self, enabled: bool) -> Result<()>;
    /// `showHardwareCursor` (TS default false).
    fn show_hardware_cursor(&self) -> bool;
    /// Persists `showHardwareCursor` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_show_hardware_cursor(&self, enabled: bool) -> Result<()>;
    /// `emojiShortcodes` (default true).
    fn emoji_shortcodes(&self) -> bool {
        true
    }
    /// Persists `emojiShortcodes` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails or is unavailable.
    fn set_emoji_shortcodes(&self, _enabled: bool) -> Result<()> {
        anyhow::bail!("Emoji shortcode settings are unavailable")
    }
    /// `editorPaddingX` (TS default 0).
    fn editor_padding_x(&self) -> u64;
    /// Persists `editorPaddingX` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_editor_padding_x(&self, padding: u64) -> Result<()>;
    /// `autocompleteMaxVisible` (TS default 5).
    fn autocomplete_max_visible(&self) -> u64;
    /// Persists `autocompleteMaxVisible` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_autocomplete_max_visible(&self, max_visible: u64) -> Result<()>;
    /// `quietStartup` (TS default false).
    fn quiet_startup(&self) -> bool;
    /// Persists `quietStartup` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_quiet_startup(&self, quiet: bool) -> Result<()>;
    /// `idleEvictionMinutes` wire form (a number or `off`).
    fn idle_eviction_minutes(&self) -> String;
    /// Persists the `idleEvictionMinutes` wire form to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_idle_eviction_minutes(&self, value: &str) -> Result<()>;
    /// `markdown.mermaid` (`off`/`final`/`streaming`).
    fn mermaid_rendering_mode(&self) -> String;
    /// Persists `markdown.mermaid` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_mermaid_rendering_mode(&self, mode: &str) -> Result<()>;
    /// `treeFilterMode` (`default`/`no-tools`/`user-only`/`labeled-only`/`all`).
    fn tree_filter_mode(&self) -> String;
    /// Persists `treeFilterMode` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_tree_filter_mode(&self, mode: &str) -> Result<()>;
    /// `defaultServiceTier` (default "default"): the wire name of the
    /// persisted default tier.
    fn default_service_tier(&self) -> String;
    /// Persists the wire name to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the tier name is not a known service tier or
    /// the settings store fails.
    fn set_default_service_tier(&self, tier: &str) -> Result<()>;
    /// `factory.enabled` (the agent factory's opt-in gate; unset reads
    /// as disabled).
    fn factory_enabled(&self) -> bool;
    /// Persists `factory.enabled` to the global scope — the same shared
    /// settings key the daemon's `factory_activity` lane advertisement
    /// and the kernel's factory gate read.
    ///
    /// # Errors
    ///
    /// Returns an error when opening or persisting the settings store
    /// fails.
    fn set_factory_enabled(&self, enabled: bool) -> Result<()>;
    /// `chatDetail` (`overview`/`details`/`all`; TS #2709 default
    /// `details`): the conversation-detail level the chat starts at.
    /// `chatDetail` (`overview`/`details`/`all`; the default reads as
    /// `overview`, the collapse mode): the conversation-detail level the
    /// chat starts at.
    fn chat_detail(&self) -> String;
    /// Persists `chatDetail` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_chat_detail(&self, detail: &str) -> Result<()>;
    /// `warnings.anthropicExtraUsage` (TS default true).
    fn warnings_anthropic_extra_usage(&self) -> bool;
    /// Persists `warnings.anthropicExtraUsage` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_warnings_anthropic_extra_usage(&self, enabled: bool) -> Result<()>;
    /// `updateChannel` (`stable`/`nightly`; unset infers the channel from
    /// the running version).
    fn update_channel(&self) -> Option<String>;
    /// Persists `updateChannel` to the global scope.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the settings store fails.
    fn set_update_channel(&self, channel: &str) -> Result<()>;
    /// The effective update channel for the running version, as its wire
    /// name (a preferred channel wins, else the prerelease infers).
    fn effective_update_channel(&self, version: &str) -> String;
    /// The `/telemetry status` report: on or off and why, the endpoint,
    /// and the installation id.
    fn telemetry_status(&self) -> String;
    fn telemetry_enabled(&self) -> bool { true }
    /// Persists `telemetry.enabled` (the TS key) and returns the report
    /// after the write (it names an environment variable or project
    /// setting that still decides).
    ///
    /// # Errors
    ///
    /// Returns `Err` when opening or persisting the settings
    /// store fails.
    fn set_telemetry_enabled(&self, enabled: bool) -> Result<String>;
    /// Whether the first-run telemetry disclosure is due in this
    /// interactive view (TS agent-session-services: telemetry enabled,
    /// onboarding already shown, notice not yet shown). Defaults to
    /// false: a client without the disclosure surface never shows it.
    #[must_use]
    fn telemetry_notice_due(&self) -> bool {
        false
    }
    /// Persist the notice-shown marker once the disclosure renders
    /// (TS `setTelemetryNoticeShown`).
    ///
    /// # Errors
    ///
    /// Returns `Err` when opening or persisting the settings
    /// store fails.
    fn set_telemetry_notice_shown(&self) -> Result<()> {
        Ok(())
    }
}
