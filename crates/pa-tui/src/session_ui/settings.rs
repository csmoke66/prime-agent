//! The `/settings` menu, `/import` confirmation, and `/reload` flow.
use crate::settings_menu::GlobalModelUpdate;

use super::{
    key_event_to_id, AgentView, DaemonCommand, Duration, KeyEvent, Map, PathBuf, Result, SessionUi,
    Value, UI_REQUEST_TIMEOUT_MS,
};

fn rlm_depth_label(status: &Value) -> Option<String> {
    let depth = status.get("maxDepth")?.as_u64()?;
    Some(match status.get("source").and_then(Value::as_str) {
        Some(source) => format!("{depth} ({source})"),
        None => depth.to_string(),
    })
}

/// The `/reload` task's report: the daemon reloaded the session's live
/// inputs, or the failure message.
pub(crate) type ReloadNote = Result<(), String>;

/// The question a pending confirm answers (callers await inline in TS;
/// the TUI loop parks the continuation instead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PendingConfirm {
    /// `/import <path>`: replace the current session with the JSONL file.
    Import { path: String },
    /// The import's stored session cwd is gone: `Yes` retries with the
    /// fallback cwd.
    ImportCwdFallback { path: String, fallback_cwd: String },
}

impl SessionUi {
    /// The shipped CHANGELOG.md path: the package directory (`PI_PACKAGE_DIR`
    /// wins, else the running executable's directory) plus `CHANGELOG.md`.
    pub(super) fn changelog_path() -> std::path::PathBuf {
        let package_dir = match std::env::var("PI_PACKAGE_DIR") {
            Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
                .unwrap_or_else(|| PathBuf::from(".")),
        };
        package_dir.join("CHANGELOG.md")
    }

    // Session import (/import)

    /// Parse the path, park the confirm, and let the panel answer it.
    pub(super) fn open_import_confirm(&mut self, command_text: &str, view: &mut AgentView) {
        let Some(input_path) = crate::export_share::path_command_argument(command_text, "/import")
        else {
            self.error_row("Usage: /import <path.jsonl>", view);
            return;
        };
        view.editor.set_text("");
        view.confirm = Some(crate::confirm::ConfirmPanel::yes_no(
            "Import session",
            &format!("Replace current session with {input_path}?"),
        ));
        self.pending_confirm = Some(PendingConfirm::Import { path: input_path });
        self.dirty = true;
    }

    /// One key press while the confirm panel owns the frame.
    pub(super) async fn handle_confirm_key(
        &mut self,
        key: KeyEvent,
        view: &mut AgentView,
    ) -> Result<()> {
        let Some(id) = key_event_to_id(&key) else {
            return Ok(());
        };
        let action = {
            let Some(confirm) = view.confirm.as_mut() else {
                return Ok(());
            };
            let kb = view.editor.keybindings();
            confirm.handle_key(kb, &id)
        };
        match action {
            crate::confirm::ConfirmAction::None => {}
            crate::confirm::ConfirmAction::Cancel => {
                view.confirm = None;
                self.pending_confirm = None;
            }
            crate::confirm::ConfirmAction::Select(option) => {
                let pending = self.pending_confirm.take();
                view.confirm = None;
                if option == "Yes" {
                    match pending {
                        Some(PendingConfirm::Import { path }) => {
                            self.run_import(&path, None, view).await?;
                        }
                        Some(PendingConfirm::ImportCwdFallback { path, fallback_cwd }) => {
                            self.run_import(&path, Some(&fallback_cwd), view).await?;
                        }
                        None => {}
                    }
                }
            }
        }
        self.dirty = true;
        Ok(())
    }

    /// The import request and its outcomes: the cancelled note, the typed
    /// error surfaces, and the successful rebuild + status.
    async fn run_import(
        &mut self,
        input_path: &str,
        cwd_override: Option<&str>,
        view: &mut AgentView,
    ) -> Result<()> {
        let response = self
            .client
            .request(DaemonCommand::ImportJsonl {
                id: None,
                active_session_id: self.active_session_id.clone(),
                input_path: input_path.to_string(),
                cwd_override: cwd_override.map(str::to_string),
                rest: Map::default(),
            })
            .await?;
        if !response.success {
            let error = response.error.unwrap_or_default();
            match response.error_info {
                Some(pa_types::daemon::DaemonErrorInfo::SessionImportFileNotFound {
                    file_path,
                }) => {
                    self.error_row(
                        &format!("Failed to import session: File not found: {file_path}"),
                        view,
                    );
                }
                Some(pa_types::daemon::DaemonErrorInfo::MissingSessionCwd { issue }) => {
                    // The confirm carries the issue's text; `Yes` retries with the fallback
                    // cwd as the override.
                    let session_cwd = issue
                        .get("sessionCwd")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let fallback_cwd = issue
                        .get("fallbackCwd")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    view.confirm = Some(crate::confirm::ConfirmPanel::yes_no(
                        "Session cwd not found",
                        &format!(
                            "cwd from session file does not exist\n{session_cwd}\n\ncontinue in current cwd\n{fallback_cwd}"
                        ),
                    ));
                    self.pending_confirm = Some(PendingConfirm::ImportCwdFallback {
                        path: input_path.to_string(),
                        fallback_cwd,
                    });
                    self.dirty = true;
                }
                _ => {
                    self.error_row(&format!("Failed to import session: {error}"), view);
                }
            }
            return Ok(());
        }
        if response
            .data
            .as_ref()
            .and_then(|data| data.get("cancelled"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            self.note("Import cancelled", view);
            return Ok(());
        }
        // The replacement's fresh branch renders from scratch, then the status row
        // lands.
        self.rebuild_transcript(view).await;
        self.refresh_stats().await;
        // The transcript rebuild ran before the refresh, so the refreshed context usage rides the
        // chrome through this tray rebuild — without it the tray keeps the pre-import usage until
        // the next settled turn.
        self.rebuild_tray(view);
        self.note(&format!("Session imported from: {input_path}"), view);
        Ok(())
    }

    // Settings (/settings)

    /// Read the daemon state and the settings seam, then mount the menu.
    pub(super) async fn open_settings_menu(&mut self, view: &mut AgentView) {
        let Some(state) = self.connection_state(view).await else {
            return;
        };
        let settings = self.client_settings.clone();
        let model = state.get("model");
        let model_id = model
            .and_then(|model| model.get("id"))
            .and_then(Value::as_str)
            .or(view.chrome.model_id.as_deref());
        let provider = model
            .and_then(|model| model.get("provider"))
            .and_then(Value::as_str)
            .or(view.chrome.model_provider.as_deref());
        let primary_model = match (provider, model_id) {
            (Some(provider), Some(model)) => format!("{provider}/{model}"),
            (_, Some(model)) => model.to_owned(),
            _ => "Choose model".into(),
        };
        let rlm_depth = match self
            .bounded_request(
                Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                DaemonCommand::GetRlmMaxDepthStatus {
                    id: None,
                    active_session_id: self.active_session_id.clone(),
                    rest: Map::default(),
                },
            )
            .await
        {
            Ok(status) => rlm_depth_label(&status),
            Err(error) => {
                self.error_row(&format!("Cannot read RLM max depth: {error:#}"), view);
                None
            }
        };
        let provider_block_targets = match self
            .bounded_request(
                Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                DaemonCommand::List {
                    id: None,
                    all: Some(true),
                    cwd: None,
                    session_dir: None,
                    include_client_owned: Some(true),
                    rest: Map::default(),
                },
            )
            .await
        {
            Ok(data) => {
                let own_session_id = state.get("sessionId").and_then(Value::as_str);
                let own_active_id = state.get("activeSessionId").and_then(Value::as_str);
                let mut choices = data
                    .get("sessions")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|session| {
                        session
                            .get("runtimeKind")
                            .and_then(Value::as_str)
                            .is_none_or(|kind| kind == "top-level")
                            && session
                                .get("parentSessionId")
                                .and_then(Value::as_str)
                                .is_none()
                    })
                    .filter_map(|session| {
                        let active = session.get("id").and_then(Value::as_str)?;
                        let durable = session
                            .get("sessionId")
                            .and_then(Value::as_str)
                            .filter(|id| !id.is_empty())
                            .unwrap_or(active);
                        if Some(active) == own_active_id || Some(durable) == own_session_id {
                            return None;
                        }
                        let name = session
                            .get("sessionName")
                            .and_then(Value::as_str)
                            .filter(|name| !name.is_empty())
                            .unwrap_or(durable);
                        let cwd = session
                            .get("cwd")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let short = durable.chars().take(12).collect::<String>();
                        Some(crate::settings_menu::ProviderBlockTargetChoice {
                            selector: durable.to_owned(),
                            label: if cwd.is_empty() {
                                format!("{name} · {short}")
                            } else {
                                format!("{name} · {short} · {cwd}")
                            },
                        })
                    })
                    .collect::<Vec<_>>();
                choices.sort_by(|left, right| left.label.cmp(&right.label));
                choices.dedup_by(|left, right| left.selector == right.selector);
                choices
            }
            Err(error) => {
                self.error_row(
                    &format!("Cannot list provider-rejection target sessions: {error:#}"),
                    view,
                );
                Vec::new()
            }
        };
        let mut values = crate::settings_menu::SettingsCurrentValues {
            autocompact: state
                .get("autoCompactionEnabled")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            steering_mode: state
                .get("steeringMode")
                .and_then(Value::as_str)
                .unwrap_or("all")
                .to_string(),
            follow_up_mode: state
                .get("followUpMode")
                .and_then(Value::as_str)
                .unwrap_or("one-at-a-time")
                .to_string(),
            thinking_level: state
                .get("thinkingLevel")
                .and_then(Value::as_str)
                .map(str::to_string),
            available_thinking_levels: state
                .get("availableThinkingLevels")
                .and_then(Value::as_array)
                .map(|levels| {
                    levels
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            primary_model,
            session_service_tier: state
                .get("serviceTier")
                .and_then(Value::as_str)
                .unwrap_or("default")
                .to_owned(),
            available_service_tiers: self.available_service_tiers(view),
            fast_mode_available: self
                .current_model_entry(view)
                .is_some_and(pa_types::ai::supports_fast_mode),
            rlm_depth: rlm_depth.unwrap_or_else(|| "Unavailable".into()),
            provider_block_targets,
            ..Default::default()
        };
        values.speed_display = self.speed_display_enabled;
        if let Some(traces) = &self.traces {
            values.trace_sharing = Some(traces.0.enabled().await);
        }
        // A missing settings seam keeps the TS defaults.
        if let Some(settings) = &settings {
            values.factory_enabled = Some(settings.factory_enabled());
            values.telemetry_enabled = Some(settings.telemetry_enabled());
            values.compaction_threshold_percent = settings.compaction_threshold_percent();
            values.tool_output_reminder_percent = Some(settings.tool_output_reminder_percent());
            values.show_images = settings.show_images();
            values.auto_resize_images = settings.image_auto_resize();
            values.block_images = settings.block_images();
            values.skill_commands = settings.enable_skill_commands();
            values.builtin_skills = settings.enable_builtin_skills();
            values.harness_refinements = settings.harness_refinements();
            values.provider_block_delivery = settings.provider_block_delivery();
            values.provider_block_target_session = settings.provider_block_target_session();
            values.web_search_provider = settings.web_search_provider();
            values.hardware_cursor = settings.show_hardware_cursor();
            values.emoji_shortcodes = settings.emoji_shortcodes();
            values.editor_padding = settings.editor_padding_x();
            values.autocomplete_max_visible = settings.autocomplete_max_visible();
            values.clear_on_shrink = settings.clear_on_shrink();
            values.terminal_progress = settings.show_terminal_progress();
            values.idle_eviction_minutes = settings.idle_eviction_minutes();
            values.mermaid = settings.mermaid_rendering_mode();
            values.quiet_startup = settings.quiet_startup();
            values.tree_filter_mode = settings.tree_filter_mode();
            values.warnings_anthropic_extra_usage = settings.warnings_anthropic_extra_usage();
            values.theme = settings.theme().unwrap_or_else(|| "prime".to_string());
            // The tier row preselects the saved default instead of the struct default.
            values.default_service_tier = settings.default_service_tier();
        } else {
            values.tool_output_reminder_percent =
                Some(pa_types::compaction::DEFAULT_TOOL_OUTPUT_REMINDER_PERCENT);
            values.show_images = true;
            values.auto_resize_images = true;
            values.skill_commands = true;
            values.builtin_skills = true;
            values.harness_refinements = true;
            values.provider_block_delivery = "show".to_string();
            values.provider_block_target_session = None;
            values.emoji_shortcodes = true;
            values.web_search_provider = "serper".to_string();
            values.idle_eviction_minutes = "90".to_string();
            values.mermaid = "streaming".to_string();
            values.tree_filter_mode = "user-only".to_string();
            values.warnings_anthropic_extra_usage = true;
            values.theme = "prime".to_string();
            // No settings seam: the tier row reads "default", never a blank value.
            values.default_service_tier = "default".to_string();
        }
        // This surface ships the builtin themes.
        values.available_themes = pa_types::themes::BUILTIN_THEME_NAMES
            .iter()
            .map(ToString::to_string)
            .collect();
        let rows = crate::settings_menu::settings_menu_rows(&values);
        let mut menu = crate::settings_menu::SettingsMenu::new(rows);
        if let (Some(settings), Some(file)) = (
            &settings,
            state.get("sessionFile").and_then(Value::as_str).filter(|file| !file.is_empty()),
        ) {
            match settings.child_policy(file) {
                Ok(policy) => menu.bind_child_policy(file.to_owned(), &policy),
                Err(error) => self.error_row(&format!("Cannot read child settings: {error:#}"), view),
            }
        }
        view.settings_menu = Some(menu);
        self.track_menu_opened("settings", "command");
        self.dirty = true;
    }

    /// One key press while the settings menu is open.
    pub(super) async fn handle_settings_menu_key(
        &mut self,
        key: KeyEvent,
        view: &mut AgentView,
    ) -> Result<()> {
        let Some(id) = key_event_to_id(&key) else {
            return Ok(());
        };
        let action = {
            let Some(menu) = view.settings_menu.as_mut() else {
                return Ok(());
            };
            menu.handle_key(&id, view.editor.keybindings())
        };
        match action {
            // No-op closes: a bare None and Esc inside a submenu keep the menu open.
            crate::settings_menu::SettingsMenuAction::None
            | crate::settings_menu::SettingsMenuAction::SubmenuClosed => {}
            crate::settings_menu::SettingsMenuAction::Cancel => {
                view.settings_menu = None;
            }
            crate::settings_menu::SettingsMenuAction::PreviewTheme { name } => {
                // Switch live without persisting.
                view.theme = crate::app::load_theme(&name);
            }
            crate::settings_menu::SettingsMenuAction::RestoreTheme { name } => {
                // Preview the row's theme back.
                view.theme = crate::app::load_theme(&name);
            }
            crate::settings_menu::SettingsMenuAction::Change { id, value } => {
                self.apply_settings_change(id, &value, view).await;
            }
            crate::settings_menu::SettingsMenuAction::ShowTelemetryStatus => {
                if let Some(settings) = &self.client_settings {
                    let report = settings.telemetry_status();
                    view.settings_menu = None;
                    self.note(&report, view);
                }
            }
            crate::settings_menu::SettingsMenuAction::OpenPrimaryModel => {
                view.settings_menu = None;
                self.open_model_picker(view, "").await?;
                self.track_menu_opened("model-picker", "settings");
            }
            crate::settings_menu::SettingsMenuAction::OpenPromptSegments => {
                self.request_prompt_segments(None, view);
            }
            crate::settings_menu::SettingsMenuAction::ChangePromptSegment(edit) => {
                self.request_prompt_segments(Some(edit), view);
            }
            crate::settings_menu::SettingsMenuAction::OpenGlobalModels => {
                let (Some(settings), Some(menu)) =
                    (self.client_settings.clone(), view.settings_menu.as_mut())
                else {
                    return Ok(());
                };
                menu.global_model_job = Some(tokio::task::spawn_blocking(move || {
                    GlobalModelUpdate::Loaded(settings.global_model_choices())
                }));
            }
            crate::settings_menu::SettingsMenuAction::SaveGlobalModels(models) => {
                let (Some(settings), Some(menu)) =
                    (self.client_settings.clone(), view.settings_menu.as_mut())
                else {
                    return Ok(());
                };
                menu.global_model_job = Some(tokio::task::spawn_blocking(move || {
                    GlobalModelUpdate::Saved(settings.set_global_models(models))
                }));
            }
            crate::settings_menu::SettingsMenuAction::SaveRlmDepth { depth, global } => {
                self.set_rlm_max_depth(depth, global, view).await;
            }
            crate::settings_menu::SettingsMenuAction::OpenChildModels => {
                let (Some(settings), Some(menu)) =
                    (&self.client_settings, view.settings_menu.as_mut())
                else {
                    return Ok(());
                };
                let Some(file) = menu.child_session_file.as_deref() else {
                    return Ok(());
                };
                match settings.child_model_choices(file) {
                    Ok(choices) => {
                        menu.open_child_models(choices);
                        self.track_menu_opened("child-models", "settings");
                    }
                    Err(error) => {
                        self.error_row(&format!("Cannot load child models: {error:#}"), view);
                    }
                }
            }
            crate::settings_menu::SettingsMenuAction::SaveChildModels(models) => {
                let (Some(settings), Some(menu)) =
                    (&self.client_settings, view.settings_menu.as_mut())
                else {
                    return Ok(());
                };
                let Some(file) = menu.child_session_file.as_deref() else {
                    return Ok(());
                };
                let model_rules = models.as_ref().map(Vec::len);
                match settings.set_child_models(file, models) {
                    Ok(()) => menu.child_models_saved(model_rules),
                    Err(error) => {
                        self.error_row(&format!("Cannot save child models: {error:#}"), view);
                    }
                }
            }
            crate::settings_menu::SettingsMenuAction::SaveChildEffort(effort) => {
                let (Some(settings), Some(menu)) =
                    (&self.client_settings, view.settings_menu.as_mut())
                else {
                    return Ok(());
                };
                let Some(file) = menu.child_session_file.as_deref() else {
                    return Ok(());
                };
                match settings.set_child_effort(file, effort) {
                    Ok(()) => menu.child_effort_saved(effort),
                    Err(error) => {
                        self.error_row(&format!("Cannot save child effort: {error:#}"), view);
                    }
                }
            }
            crate::settings_menu::SettingsMenuAction::SaveProviderBlockTarget {
                selector,
                label,
            } => {
                let Some(settings) = &self.client_settings else {
                    return Ok(());
                };
                match settings.set_provider_block_target_session(&selector) {
                    Ok(()) => {
                        if let Some(menu) = &mut view.settings_menu {
                            menu.set_row_value("provider-block-target", &label);
                        }
                        self.note(
                            &format!("Provider rejection reports will route to {label}."),
                            view,
                        );
                    }
                    Err(error) => {
                        self.error_row(
                            &format!("Cannot save provider-rejection target: {error:#}"),
                            view,
                        );
                    }
                }
            }
        }
        self.dirty = true;
        Ok(())
    }

    fn request_prompt_segments(
        &self,
        edit: Option<pa_types::daemon::PromptSegmentEdit>,
        view: &mut AgentView,
    ) {
        let Some(menu) = view.settings_menu.as_mut() else {
            return;
        };
        let client = self.client.clone();
        let command = DaemonCommand::SessionPromptSegments {
            id: None,
            active_session_id: self.active_session_id.clone(),
            edit,
            rest: Map::default(),
        };
        menu.prompt_segment_job = Some(tokio::spawn(async move {
            let value = tokio::time::timeout(
                Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                client.request_ok(command),
            )
            .await
            .map_err(|_| anyhow::anyhow!("Timed out waiting for prompt segments"))??;
            Ok(serde_json::from_value(value)?)
        }));
    }

    pub(crate) fn apply_prompt_segment_update(
        &mut self,
        update: Result<
            anyhow::Result<Vec<pa_types::daemon::SessionPromptSegment>>,
            tokio::task::JoinError,
        >,
        view: &mut AgentView,
    ) {
        let Some(menu) = view.settings_menu.as_mut() else {
            return;
        };
        menu.prompt_segment_job = None;
        match update.map_err(anyhow::Error::from).and_then(|rows| rows) {
            Ok(rows) => menu.apply_prompt_segments(rows),
            Err(error) => self.error_row(&format!("System prompt segments: {error:#}"), view),
        }
        self.dirty = true;
    }

    pub(crate) fn apply_global_model_update(
        &mut self,
        update: Result<GlobalModelUpdate, tokio::task::JoinError>,
        view: &mut AgentView,
    ) {
        let Some(menu) = view.settings_menu.as_mut() else {
            return;
        };
        menu.global_model_job = None;
        match update {
            Ok(GlobalModelUpdate::Loaded(Ok(choices))) => menu.open_global_models(choices),
            Ok(GlobalModelUpdate::Saved(Ok(()))) => {
                menu.global_models_saved();
                self.spawn_model_catalog_refresh();
                self.note("Global model settings saved.", view);
            }
            Ok(GlobalModelUpdate::Loaded(Err(error))) => {
                self.error_row(
                    &format!("Cannot load global model settings: {error:#}"),
                    view,
                );
            }
            Ok(GlobalModelUpdate::Saved(Err(error))) => {
                self.error_row(
                    &format!("Cannot save global model settings: {error:#}"),
                    view,
                );
            }
            Err(error) => {
                self.error_row(
                    &format!("Global model settings job failed: {error:#}"),
                    view,
                );
            }
        }
        self.dirty = true;
    }

    /// One settings row's change: daemon commands for session-owned switches,
    /// the settings seam for persisted preferences.
    async fn apply_settings_change(&mut self, id: &str, value: &str, view: &mut AgentView) {
        match id {
            "speed-display" => self.set_speed_display(value == "true", view),
            "autonomous" => {
                view.settings_menu = None;
                if let Err(error) = self.send_prompt(
                    &format!("/autonomous {value}"),
                    super::prompt::SubmitBehavior::Steer,
                    view,
                ) {
                    self.error_row(&error.to_string(), view);
                }
            }
            "provider-block-delivery" => {
                let Some(settings) = &self.client_settings else {
                    return;
                };
                if let Err(error) = settings.set_provider_block_delivery(value) {
                    self.error_row(
                        &format!("Cannot save provider-rejection delivery: {error:#}"),
                        view,
                    );
                }
            }
            "trace-sharing" => {
                view.settings_menu = None;
                if let Err(err) = self.set_trace_sharing(value == "true", view).await {
                    self.error_row(&format!("{err:#}"), view);
                }
            }
            "factory" | "telemetry" => {
                let Some(settings) = self.client_settings.clone() else { return };
                let enabled = value == "true";
                let previous = match id {
                    "factory" => settings.factory_enabled().to_string(),
                    _ => settings.telemetry_enabled().to_string(),
                };
                let result = match id {
                    "factory" => {
                        let guard = if enabled { Ok(()) } else { match self.live_factory_runs().await {
                            Some(n) if n > 0 => Err(anyhow::anyhow!("Cannot disable the factory while {n} {} still live — stop the runs first.", if n == 1 { "run is" } else { "runs are" })),
                            None if self.factory_activity_supported() => Err(anyhow::anyhow!("Cannot verify live factory runs; factory remains enabled.")),
                            _ => Ok(()),
                        }};
                        guard.and_then(|()| settings.set_factory_enabled(enabled)).map(|()| {
                            if enabled { "The factory is enabled. Restart the client to surface the factory group." }
                            else { "The factory is disabled." }.to_owned()
                        })
                    }
                    _ => settings.set_telemetry_enabled(enabled),
                };
                match result {
                    Ok(message) => self.note(&message, view),
                    Err(error) => {
                        if let Some(menu) = &mut view.settings_menu { menu.set_row_value(id, &previous); }
                        self.error_row(&format!("{error:#}"), view);
                    }
                }
            }
            "autocompact-threshold" => {
                if let Some(settings) = &self.client_settings {
                    let result = value
                        .parse::<f64>()
                        .map_err(anyhow::Error::from)
                        .and_then(|percent| settings.set_compaction_threshold_percent(percent));
                    if let Err(error) = result {
                        self.error_row(&format!("{error:#}"), view);
                    }
                }
            }
            "tool-output-reminder" => {
                if let Some(settings) = &self.client_settings {
                    let result = value
                        .parse::<u8>()
                        .map_err(anyhow::Error::from)
                        .and_then(|percent| settings.set_tool_output_reminder_percent(percent));
                    if let Err(error) = result {
                        self.error_row(&format!("{error:#}"), view);
                    }
                }
            }
            "autocompact" => {
                self.daemon_switch(
                    DaemonCommand::SetAutoCompaction {
                        id: None,
                        active_session_id: self.active_session_id.clone(),
                        enabled: value == "true",
                        rest: Map::default(),
                    },
                    view,
                )
                .await;
            }
            "show-images" => {
                if let Some(settings) = &self.client_settings {
                    if let Err(error) = settings.set_show_images(value == "true") {
                        self.error_row(&format!("{error:#}"), view);
                        return;
                    }
                }
                self.show_images = value == "true";
                view.show_images = value == "true";
            }
            "auto-resize-images" => {
                self.persist_bool_setting(
                    |settings, enabled| settings.set_image_auto_resize(enabled),
                    value,
                    view,
                );
            }
            "block-images" => {
                self.persist_bool_setting(
                    |settings, blocked| settings.set_block_images(blocked),
                    value,
                    view,
                );
            }
            "skill-commands" => {
                self.persist_bool_setting(
                    |settings, enabled| settings.set_enable_skill_commands(enabled),
                    value,
                    view,
                );
                // The cached skill list re-applies under the new setting value (no daemon
                // round trip).
                let enabled = self
                    .client_settings
                    .as_ref()
                    .is_some_and(|settings| settings.enable_skill_commands());
                let skills = if enabled {
                    self.skill_commands_cache.clone()
                } else {
                    Vec::new()
                };
                view.editor.set_autocomplete_skill_commands(skills);
                self.dirty = true;
            }
            "harness-refinements" => {
                self.persist_bool_setting(
                    |settings, enabled| settings.set_harness_refinements(enabled),
                    value,
                    view,
                );
            }
            "web-search-provider" => {
                if let Some(settings) = &self.client_settings {
                    if let Err(error) = settings.set_web_search_provider(value) {
                        self.error_row(&format!("{error:#}"), view);
                    }
                }
            }
            "builtin-skills" => {
                self.persist_bool_setting(
                    |settings, enabled| settings.set_enable_builtin_skills(enabled),
                    value,
                    view,
                );
                // The toggle takes effect after a reload.
                let _ = self.handle_reload_command(view);
            }
            "show-hardware-cursor" => {
                // A failed persist changes nothing: the live flag flips only when the
                // setting actually persisted.
                if let Some(settings) = &self.client_settings {
                    if let Err(error) = settings.set_show_hardware_cursor(value == "true") {
                        self.error_row(&format!("{error:#}"), view);
                        return;
                    }
                }
                // The very next frame shows or hides the hardware cursor at the focused
                // caret.
                view.show_hardware_cursor = value == "true";
            }
            "emoji-shortcodes" => {
                if let Some(settings) = &self.client_settings {
                    if let Err(error) = settings.set_emoji_shortcodes(value == "true") {
                        self.error_row(&format!("{error:#}"), view);
                        return;
                    }
                }
                view.editor.set_emoji_shortcodes(value == "true");
            }
            "editor-padding" => {
                if let Some(settings) = &self.client_settings {
                    if let Ok(padding) = value.parse::<u64>() {
                        if let Err(error) = settings.set_editor_padding_x(padding) {
                            self.error_row(&format!("{error:#}"), view);
                        }
                    }
                }
            }
            "autocomplete-max-visible" => {
                if let Some(settings) = &self.client_settings {
                    if let Ok(max_visible) = value.parse::<u64>() {
                        if let Err(error) = settings.set_autocomplete_max_visible(max_visible) {
                            self.error_row(&format!("{error:#}"), view);
                        }
                    }
                }
            }
            "clear-on-shrink" => {
                self.persist_bool_setting(
                    |settings, enabled| settings.set_clear_on_shrink(enabled),
                    value,
                    view,
                );
            }
            "terminal-progress" => {
                self.persist_bool_setting(
                    |settings, enabled| settings.set_show_terminal_progress(enabled),
                    value,
                    view,
                );
            }
            "idle-eviction-minutes" => {
                if let Some(settings) = &self.client_settings {
                    if let Err(error) = settings.set_idle_eviction_minutes(value) {
                        self.error_row(&format!("{error:#}"), view);
                    }
                }
            }
            "steering-mode" => {
                self.daemon_switch(
                    DaemonCommand::SetSteeringMode {
                        id: None,
                        active_session_id: self.active_session_id.clone(),
                        mode: serde_json::Value::String(value.to_string()),
                        rest: Map::default(),
                    },
                    view,
                )
                .await;
                // Refresh the steering-mode cache the queued-input event reads, so a
                // submission right after the switch reports the new mode.
                let _ = self.connection_state(view).await;
            }
            "follow-up-mode" => {
                self.daemon_switch(
                    DaemonCommand::SetFollowUpMode {
                        id: None,
                        active_session_id: self.active_session_id.clone(),
                        mode: serde_json::Value::String(value.to_string()),
                        rest: Map::default(),
                    },
                    view,
                )
                .await;
            }
            "transport" => {
                let Ok(transport) = serde_json::from_value::<pa_types::ai::Transport>(
                    serde_json::Value::String(value.to_string()),
                ) else {
                    return;
                };
                self.daemon_switch(
                    DaemonCommand::SetTransport {
                        id: None,
                        active_session_id: self.active_session_id.clone(),
                        transport,
                        rest: Map::default(),
                    },
                    view,
                )
                .await;
            }
            "default-service-tier" => {
                // Preserve the existing behavior: save the default first, then switch the session.
                if let Some(settings) = &self.client_settings {
                    if let Err(error) = settings.set_default_service_tier(value) {
                        self.error_row(&format!("{error:#}"), view);
                        return;
                    }
                }
                if let Some(applied) = self.set_session_service_tier(value, view).await {
                    self.note(&format!("Default service tier: {value} (session: {applied})"), view);
                }
            }
            "session-service-tier" => {
                if let Some(applied) = self.set_session_service_tier(value, view).await {
                    self.note(&format!("Service tier: {applied}"), view);
                }
            }
            "fast-mode" => {
                if let Some(applied) = self.set_session_service_tier(value, view).await {
                    let status = if applied == "priority" { "on" } else { "off" };
                    self.note(&format!("Fast mode: {status}"), view);
                }
            }

            "mermaid-rendering" => {
                if let Some(settings) = &self.client_settings {
                    if let Err(error) = settings.set_mermaid_rendering_mode(value) {
                        self.error_row(&format!("{error:#}"), view);
                    }
                }
            }
            "quiet-startup" => {
                self.persist_bool_setting(
                    |settings, quiet| settings.set_quiet_startup(quiet),
                    value,
                    view,
                );
            }
            "tree-filter-mode" => {
                if let Some(settings) = &self.client_settings {
                    if let Err(error) = settings.set_tree_filter_mode(value) {
                        self.error_row(&format!("{error:#}"), view);
                        return;
                    }
                }
                // The `/tree` selector reads the live field.
                self.tree_filter_mode = crate::tree_list::filter_mode_from_str(value);
            }
            "warnings-anthropic-extra-usage" => {
                self.persist_bool_setting(
                    |settings, enabled| settings.set_warnings_anthropic_extra_usage(enabled),
                    value,
                    view,
                );
            }
            "thinking" => {
                self.apply_thinking_level(value, view).await;
            }
            "theme" => {
                if let Some(settings) = &self.client_settings {
                    if let Err(error) = settings.set_theme(value) {
                        self.error_row(&format!("{error:#}"), view);
                        return;
                    }
                }
                view.theme = crate::app::load_theme(value);
            }
            other => {
                self.error_row(&format!("Unknown setting: {other}"), view);
            }
        }
    }

    /// Persist one boolean row through the settings seam, surfacing errors.
    fn persist_bool_setting(
        &mut self,
        set: impl FnOnce(&dyn crate::client_settings::ClientSettings, bool) -> anyhow::Result<()>,
        value: &str,
        view: &mut AgentView,
    ) {
        if let Some(settings) = &self.client_settings {
            if let Err(error) = set(settings.as_ref(), value == "true") {
                self.error_row(&format!("{error:#}"), view);
            }
        }
    }

    /// A session-switch daemon command: the result never blocks the menu.
    async fn daemon_switch(&mut self, command: DaemonCommand, view: &mut AgentView) {
        if let Err(error) = self
            .bounded_request(Duration::from_millis(UI_REQUEST_TIMEOUT_MS), command)
            .await
        {
            self.error_row(&format!("{error:#}"), view);
        }
    }

    /// Apply a tier and confirm the daemon's effective value before updating the tray/menu.
    async fn set_session_service_tier(
        &mut self,
        value: &str,
        view: &mut AgentView,
    ) -> Option<String> {
        let tier = match serde_json::from_value::<pa_types::ai::ServiceTier>(
            Value::String(value.to_owned()),
        ) {
            Ok(tier) => tier,
            Err(error) => {
                self.error_row(&format!("{error:#}"), view);
                return None;
            }
        };
        let switched = self
            .bounded_request(
                Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                DaemonCommand::SetServiceTier {
                    id: None,
                    active_session_id: self.active_session_id.clone(),
                    service_tier: Some(tier),
                    rest: Map::default(),
                },
            )
            .await;
        if let Err(error) = switched {
            self.error_row(&format!("{error:#}"), view);
            return None;
        }
        let state = self.connection_state(view).await?;
        let applied = state.get("serviceTier")?.as_str()?;
        self.service_tier = Some(applied.to_owned());
        view.chrome.service_tier.clone_from(&self.service_tier);
        let fast_available = self
            .current_model_entry(view)
            .is_some_and(pa_types::ai::supports_fast_mode);
        if let Some(menu) = view.settings_menu.as_mut() {
            menu.set_row_value("session-service-tier", applied);
            if fast_available {
                menu.set_row_value("fast-mode", if applied == "priority" { "on" } else { "off" });
            }
        }
        Some(applied.to_owned())
    }

    /// Apply the session depth even if persisting the optional global default fails.
    async fn set_rlm_max_depth(&mut self, max_depth: u64, global: bool, view: &mut AgentView) {
        match self
            .bounded_request(
                Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                DaemonCommand::SetRlmMaxDepth {
                    id: None,
                    active_session_id: self.active_session_id.clone(),
                    max_depth,
                    global: Some(global),
                    rest: Map::default(),
                },
            )
            .await
        {
            Ok(data) => {
                if let (Some(menu), Some(label)) = (view.settings_menu.as_mut(), rlm_depth_label(&data)) {
                    menu.set_row_value("rlm-max-depth", &label);
                }
                let saved = data
                    .get("globalSaved")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.plain_row(
                    &format!(
                        "RLM max depth set: {max_depth}{}",
                        if saved {
                            " and saved as global default"
                        } else {
                            ""
                        }
                    ),
                    view,
                );
                if let Some(error) = data.get("globalError").and_then(Value::as_str) {
                    self.error_row(
                        &format!("RLM max depth set for this chat, but the global default was not saved: {error}"),
                        view,
                    );
                }
            }
            Err(error) => self.error_row(&format!("{error:#}"), view),
        }
    }

    /// The Ctrl+O cycle saves the new level as the global `chatDetail` setting, so every later chat
    /// opens at it. A failed save lands only in the settings store's diagnostics.
    pub(crate) fn save_chat_detail(&self, view: &AgentView) {
        if let Some(settings) = &self.client_settings {
            let _ = settings.set_chat_detail(view.detail.wire_name());
        }
    }

    /// The reload box replaces the editor while the daemon reload runs; the
    /// run loop folds the outcome in when it lands.
    pub(super) fn handle_reload_command(&mut self, view: &mut AgentView) -> Result<()> {
        view.reload_box = Some("Reloading keybindings, skills, prompts, themes...".to_string());
        self.dirty = true;
        let client = self.client.clone();
        let active_session_id = self.active_session_id.clone();
        let notes = self.reload_notes.clone();
        let task = tokio::spawn(async move {
            let outcome = client
                .request_ok(DaemonCommand::Reload {
                    id: None,
                    active_session_id,
                    rest: Map::default(),
                })
                .await
                .map(|_| ())
                .map_err(|error| format!("{error:#}"));
            let _ = notes.send(outcome);
        });
        self.reload = Some(task);
        Ok(())
    }

    /// Drop the box, re-read the user keybindings and theme, refresh the model
    /// catalog, and surface the status row.
    pub(crate) async fn apply_reload_outcome(&mut self, outcome: ReloadNote, view: &mut AgentView) {
        self.reload = None;
        view.reload_box = None;
        match outcome {
            Ok(()) => {
                // The view rebuilds from the durable session store, so client status rows
                // drop exactly like the TS re-mount.
                self.rebuild_transcript(view).await;
                // The Rust editor consumes the keybinding set, so the reloaded manager
                // replaces it.
                let mut keybindings = view.editor.keybindings().clone();
                keybindings.reload();
                view.editor.set_keybindings(keybindings);
                // An unknown theme name keeps the current theme (the startup loader's
                // fallback).
                if let Some(settings) = &self.client_settings {
                    if let Some(name) = settings.theme() {
                        view.theme = crate::app::load_theme(&name);
                    }
                }
                // The model catalog re-fetch lands through the run loop's channel.
                self.spawn_model_catalog_refresh();
                // The same refresh re-fetches the slash-command catalog (skills may have
                // changed).
                self.spawn_command_catalog_refresh();
                // Tracked, so a back-to-back status rewrites it in place.
                self.note("Reloaded keybindings, skills, prompts, themes", view);
            }
            Err(error) => {
                self.error_row(&format!("Reload failed: {error}"), view);
            }
        }
        self.dirty = true;
    }
}
