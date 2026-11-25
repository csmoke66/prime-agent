//! The `/settings` inline menu: a tab strip under the bordered search field, each tab a list of
//! label/value rows the arrows and Enter/Space cycle (the operator's 2026-09-28 rebind: tabs move
//! on Tab/number keys only), Enter opens submenus, type-to-search per tab. The caller owns the row
//! data and change actions; this module navigation and rendering.

mod model_policy;
mod tabs;

use crate::client_settings::{ChildModelChoices, ChildPolicy, GlobalModelChoices};
use crate::keybindings::{format_key_text, KeybindingsManager};
use crate::search_input::SearchInput;
use crate::theme::{Theme, ThemeColor};
use crate::width::wrap_text;
use model_policy::{policy_label, ModelPolicyPicker};
use pa_types::ai::ModelThinkingLevel;

/// The reasoning-level descriptions the TS thinking submenu lists.
fn thinking_description(level: &str) -> &'static str {
    match level {
        "off" => "No reasoning",
        "minimal" => "Very brief reasoning",
        "low" => "Light reasoning",
        "medium" => "Moderate reasoning",
        "high" => "Deep reasoning",
        "xhigh" => "Very deep reasoning",
        "max" => "Maximum reasoning",
        _ => "",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderBlockTargetChoice {
    pub selector: String,
    pub label: String,
}

/// A submenu a row opens with Enter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsSubmenu {
    /// "Thinking Level" (the session's available levels).
    Thinking { levels: Vec<String> },
    /// "Theme" (the registered themes; selection previews).
    Theme { themes: Vec<String> },
    /// "Warnings" (the single warning toggle as its own list).
    Warnings,
    /// "Default Service Tier" (TS `SERVICE_TIER_OPTIONS`).
    ServiceTier,
    /// Service tiers supported by the session's current model.
    SessionServiceTier { tiers: Vec<&'static str> },
    /// Reasoning override for newly spawned children.
    ChildEffort,
    /// Existing independent sessions eligible to receive provider-rejection reports.
    ProviderBlockTarget {
        sessions: Vec<ProviderBlockTargetChoice>,
    },
}

/// The service-tier descriptions the TS settings submenu lists.
#[must_use]
pub fn service_tier_description(tier: &str) -> &'static str {
    match tier {
        "default" => "Standard processing",
        "flex" => "Cheaper, slower, may hit capacity limits",
        "priority" => "Faster, more expensive (fast mode)",
        "auto" => "Provider picks the tier",
        _ => "",
    }
}

/// The settings-row and autocomplete choice order.
pub const SERVICE_TIER_CHOICES: [&str; 4] = ["default", "flex", "priority", "auto"];

const CHILD_EFFORT_CHOICES: [Option<ModelThinkingLevel>; 8] = [
    None,
    Some(ModelThinkingLevel::Off),
    Some(ModelThinkingLevel::Minimal),
    Some(ModelThinkingLevel::Low),
    Some(ModelThinkingLevel::Medium),
    Some(ModelThinkingLevel::High),
    Some(ModelThinkingLevel::Xhigh),
    Some(ModelThinkingLevel::Max),
];

fn child_effort_label(effort: Option<ModelThinkingLevel>) -> &'static str {
    effort.map_or("inherit", ModelThinkingLevel::wire_name)
}

/// One settings row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsMenuRow {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// The displayed current value (right side).
    pub current: String,
    /// The values Enter/Space cycles through (`None` = submenu row).
    pub values: Option<Vec<String>>,
    pub submenu: Option<SettingsSubmenu>,
}

/// One key press while the menu is open. The theme submenu's live preview
/// and its Esc restore come through as their own actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsMenuAction {
    None,
    /// Esc from the top level.
    Cancel,
    /// Enter/Space changed a row (or a submenu selected a value).
    Change {
        id: &'static str,
        value: String,
    },
    /// Open the session model picker.
    OpenPrimaryModel,
    OpenGlobalModels,
    SaveGlobalModels(Vec<String>),
    ShowTelemetryStatus,
    /// Apply a depth to this session, optionally also saving the global default.
    SaveRlmDepth { depth: u64, global: bool },
    /// Load the authenticated catalog when the child-model row is opened.
    OpenChildModels,
    /// Persist the child model draft. None inherits, an explicit empty list allows all.
    SaveChildModels(Option<Vec<String>>),
    /// Persist the child effort; None inherits the spawn request or parent.
    SaveChildEffort(Option<ModelThinkingLevel>),
    SaveProviderBlockTarget {
        selector: String,
        label: String,
    },
    /// The theme submenu's selection moved: preview the theme live.
    PreviewTheme {
        name: String,
    },
    /// Esc inside a submenu closed it; the menu itself stays open.
    SubmenuClosed,
    /// The theme submenu closed with Esc: restore the row's theme.
    RestoreTheme {
        name: String,
    },
}

/// The open submenu's own selection state.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SubmenuState {
    row: usize,
    kind: SettingsSubmenu,
    selected: usize,
}

#[derive(Debug)]
pub(crate) enum GlobalModelUpdate {
    Loaded(anyhow::Result<GlobalModelChoices>),
    Saved(anyhow::Result<()>),
}

#[derive(Debug)]
struct DepthEdit {
    global: bool,
    input: SearchInput,
    invalid: bool,
}

/// The settings menu: the strip rides under the search field, each tab
/// owns its own search input, filtered window, and selection.
#[derive(Debug)]
pub struct SettingsMenu {
    rows: Vec<SettingsMenuRow>,
    tabs: Vec<SettingsTab>,
    tab: usize,
    sub: Option<SubmenuState>,
    depth_edit: Option<DepthEdit>,
    autonomous_edit: Option<SearchInput>,
    model_policy: Option<ModelPolicyPicker>,
    pub(crate) global_model_job: Option<tokio::task::JoinHandle<GlobalModelUpdate>>,
    pub(crate) child_session_file: Option<String>,
    max_visible: usize,
}

/// One tab: the row indices it groups with its own search input, filtered window, and selection —
/// switching moves the focus only, so coming back restores where the user was.
#[derive(Debug)]
struct SettingsTab {
    name: &'static str,
    rows: Vec<usize>,
    filtered: Vec<usize>,
    search: SearchInput,
    selected: usize,
}

/// The TS settings-menu rows in the TS order, with the caller-assembled
/// current values.
pub fn settings_menu_rows(current: &SettingsCurrentValues) -> Vec<SettingsMenuRow> {
    let bool_value = || vec!["true".to_string(), "false".to_string()];
    let mut idle_values: Vec<String> = ["off"]
        .iter()
        .map(ToString::to_string)
        .chain([30, 60, 90, 180, 360].iter().map(ToString::to_string))
        .collect();
    if let Ok(minutes) = current.idle_eviction_minutes.parse::<u32>() {
        if minutes > 0 && !idle_values.contains(&minutes.to_string()) {
            idle_values.push(minutes.to_string());
            idle_values[1..].sort_by_key(|value| value.parse::<u32>().unwrap_or(0));
        }
    }
    let percent =
        if pa_types::compaction::is_valid_threshold_percent(current.compaction_threshold_percent) {
            current.compaction_threshold_percent
        } else {
            95.0
        };
    let mut thresholds: Vec<f64> = (5..=100).step_by(5).map(f64::from).collect();
    if !thresholds.contains(&percent) {
        thresholds.push(percent);
        thresholds.sort_by(f64::total_cmp);
    }
    let reminder = current
        .tool_output_reminder_percent
        .filter(|percent| *percent <= 100)
        .unwrap_or(pa_types::compaction::DEFAULT_TOOL_OUTPUT_REMINDER_PERCENT);
    let mut reminder_values: Vec<u8> = (0..=100).step_by(5).collect();
    if !reminder_values.contains(&reminder) {
        reminder_values.push(reminder);
        reminder_values.sort_unstable();
    }
    let toggle = |id, label, description, value: Option<bool>| SettingsMenuRow {
        id, label, description,
        current: value.map_or_else(|| "Unavailable".into(), |v| v.to_string()),
        values: value.map(|_| bool_value()), submenu: None,
    };
    vec![
        toggle("factory", "Factory", "Enable factory orchestration for agents", current.factory_enabled),
        toggle("speed-display", "Output speed", "Show output tokens per second in the footer", Some(current.speed_display)),
        toggle("telemetry", "Usage telemetry", "Persist the telemetry preference; environment overrides still apply", current.telemetry_enabled),
        SettingsMenuRow {
            id: "telemetry-status", label: "Telemetry status", description: "View effective reporting status and any overrides without changing the preference",
            current: if current.telemetry_enabled.is_some() { "View" } else { "Unavailable" }.into(),
            values: None, submenu: None,
        },
        toggle("trace-sharing", "Trace sharing", "Automatically upload session traces", current.trace_sharing),
        SettingsMenuRow {
            id: "provider-block-delivery",
            label: "Provider rejection delivery",
            description: "Show in this chat, silence locally, or route a sanitized report to another existing session",
            current: if current.provider_block_delivery.is_empty() {
                "show".into()
            } else {
                current.provider_block_delivery.clone()
            },
            values: Some(vec!["show".into(), "silence".into(), "route".into()]),
            submenu: None,
        },
        SettingsMenuRow {
            id: "provider-block-target",
            label: "Provider rejection target",
            description: "Independent existing session that receives routed reports through its inbox",
            current: current.provider_block_target_session.as_ref().and_then(|target| {
                current.provider_block_targets.iter()
                    .find(|choice| &choice.selector == target)
                    .map(|choice| choice.label.clone())
            }).unwrap_or_else(|| current.provider_block_target_session.clone().unwrap_or_else(|| "Not selected".into())),
            values: None,
            submenu: (!current.provider_block_targets.is_empty()).then(|| SettingsSubmenu::ProviderBlockTarget {
                sessions: current.provider_block_targets.clone(),
            }),
        },
        SettingsMenuRow {
            id: "update-channel", label: "Update channel", description: "Stable releases or nightly pre-releases",
            current: current.update_channel.clone().unwrap_or_else(|| "Unavailable".into()),
            values: current.update_channel.as_ref().map(|_| vec!["stable".into(), "nightly".into()]), submenu: None,
        },
        SettingsMenuRow {
            id: "autonomous", label: "Autonomous mode", description: "Inspect or configure continuous work, budgets and verification gates",
            current: "Configure".into(), values: None, submenu: None,
        },
        SettingsMenuRow {
            id: "autocompact-threshold",
            label: "Auto-compact threshold (%)",
            description: "Context percentage (1–100); retained history and model output budgets bound the trigger",
            current: percent.to_string(),
            values: Some(thresholds.into_iter().map(|value| value.to_string()).collect()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "tool-output-reminder",
            label: "Tool-result reminder (%)",
            description: "Remind on every request at or above this percentage of the model window; 0 disables",
            current: reminder.to_string(),
            values: Some(reminder_values.into_iter().map(|value| value.to_string()).collect()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "autocompact",
            label: "Auto-compact",
            description: "Automatically compact context when it gets too large",
            current: current.autocompact.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "show-images",
            label: "Show image metadata",
            description: "Show image type and dimensions in terminal",
            current: current.show_images.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "auto-resize-images",
            label: "Auto-resize images",
            description: "Resize large images to 2000x2000 max for better model compatibility",
            current: current.auto_resize_images.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "block-images",
            label: "Block images",
            description: "Prevent images from being sent to LLM providers",
            current: current.block_images.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "skill-commands",
            label: "Skill commands",
            description: "Register skills as /skill:name commands",
            current: current.skill_commands.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "harness-refinements",
            label: "Harness refinements",
            description: "Create and load harness refinements; prior replies and summaries retain their influence",
            current: current.harness_refinements.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "web-search-provider",
            label: "Web search provider",
            description: "Search in new sessions. Claude and ChatGPT use your logins; Serper and Eco use /mcp keys. Failed searches can fall back to ChatGPT.",
            current: current.web_search_provider.clone(),
            values: Some(vec!["serper".into(), "eco".into(), "chatgpt".into(), "claude".into()]),
            submenu: None,
        },
        SettingsMenuRow {
            id: "builtin-skills",
            label: "Built-in skills",
            description: "Load built-in skills shipped with prime-agent (takes effect after reload)",
            current: current.builtin_skills.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "show-hardware-cursor",
            label: "Show hardware cursor",
            description: "Show the terminal cursor while still positioning it for IME support",
            current: current.hardware_cursor.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "editor-padding",
            label: "Editor padding",
            description: "Horizontal padding for input editor (0-3)",
            current: current.editor_padding.to_string(),
            values: Some(vec!["0".into(), "1".into(), "2".into(), "3".into()]),
            submenu: None,
        },
        SettingsMenuRow {
            id: "autocomplete-max-visible",
            label: "Autocomplete max items",
            description: "Max visible items in autocomplete dropdown (3-20)",
            current: current.autocomplete_max_visible.to_string(),
            values: Some(vec!["3".into(), "5".into(), "7".into(), "10".into(), "15".into(), "20".into()]),
            submenu: None,
        },
        SettingsMenuRow {
            id: "emoji-shortcodes",
            label: "Emoji shortcodes",
            description: "Suggest emoji when typing :name in the editor",
            current: current.emoji_shortcodes.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "clear-on-shrink",
            label: "Clear on shrink",
            description: "Clear empty rows when content shrinks (may cause flicker)",
            current: current.clear_on_shrink.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "terminal-progress",
            label: "Terminal progress",
            description: "Show OSC 9;4 progress indicators in the terminal tab bar",
            current: current.terminal_progress.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "idle-eviction-minutes",
            label: "Idle worker eviction",
            description: "Stop fully idle agent trees after this many minutes (global daemon policy)",
            current: current.idle_eviction_minutes.clone(),
            values: Some(idle_values),
            submenu: None,
        },
        SettingsMenuRow {
            id: "steering-mode",
            label: "Steering mode",
            description: "Enter while streaming queues steering messages. 'one-at-a-time': deliver one, wait for response. 'all': deliver all at once.",
            current: current.steering_mode.clone(),
            values: Some(vec!["one-at-a-time".into(), "all".into()]),
            submenu: None,
        },
        SettingsMenuRow {
            id: "follow-up-mode",
            label: "Follow-up mode",
            description: "Alt+Enter queues follow-up messages until agent stops. 'one-at-a-time': deliver one, wait for response. 'all': deliver all at once.",
            current: current.follow_up_mode.clone(),
            values: Some(vec!["one-at-a-time".into(), "all".into()]),
            submenu: None,
        },
        SettingsMenuRow {
            id: "transport",
            label: "Transport",
            description: "Preferred transport for providers that support multiple transports",
            current: current.transport.clone(),
            values: Some(vec!["sse".into(), "websocket".into(), "websocket-cached".into(), "auto".into()]),
            submenu: None,
        },
        SettingsMenuRow {
            id: "default-service-tier",
            label: "Default service tier",
            description: "Service tier for new sessions; applies to the current session when the model supports it",
            current: current.default_service_tier.clone(),
            values: None,
            submenu: Some(SettingsSubmenu::ServiceTier),
        },
        SettingsMenuRow {
            id: "mermaid-rendering",
            label: "Mermaid diagrams",
            description: "Render Mermaid code blocks as Unicode diagrams",
            current: current.mermaid.clone(),
            values: Some(vec!["off".into(), "final".into(), "streaming".into()]),
            submenu: None,
        },
        SettingsMenuRow {
            id: "quiet-startup",
            label: "Quiet startup",
            description: "Disable verbose printing at startup",
            current: current.quiet_startup.to_string(),
            values: Some(bool_value()),
            submenu: None,
        },
        SettingsMenuRow {
            id: "tree-filter-mode",
            label: "Tree filter mode",
            description: "Default filter when opening /tree",
            current: current.tree_filter_mode.clone(),
            values: Some(vec!["default".into(), "no-tools".into(), "user-only".into(), "labeled-only".into(), "all".into()]),
            submenu: None,
        },
        SettingsMenuRow {
            id: "warnings",
            label: "Warnings",
            description: "Enable or disable individual warnings",
            current: "configure".to_string(),
            values: None,
            submenu: Some(SettingsSubmenu::Warnings),
        },
        SettingsMenuRow {
            id: "thinking",
            label: "Thinking level",
            description: "Current session's reasoning effort",
            current: current.thinking_level.clone().unwrap_or_default(),
            values: None,
            submenu: Some(SettingsSubmenu::Thinking {
                levels: current.available_thinking_levels.clone(),
            }),
        },
        SettingsMenuRow {
            id: "primary-model",
            label: "Primary model",
            description: "Enter: choose this session's model. Space: edit which models are allowed globally.",
            current: current.primary_model.clone(),
            values: None,
            submenu: None,
        },
        SettingsMenuRow {
            id: "session-service-tier",
            label: "Session service tier",
            description: "Service tier for the current model in this session",
            current: current.session_service_tier.clone(),
            values: None,
            submenu: Some(SettingsSubmenu::SessionServiceTier {
                tiers: current.available_service_tiers.clone(),
            }),
        },
        SettingsMenuRow {
            id: "fast-mode",
            label: "Fast mode",
            description: "Use the priority service tier in this session when the model supports fast mode",
            current: match (current.fast_mode_available, current.session_service_tier.as_str()) {
                (false, _) => "Unavailable",
                (true, "priority") => "on",
                (true, _) => "off",
            }
            .into(),
            values: None,
            submenu: None,
        },
        SettingsMenuRow {
            id: "rlm-max-depth",
            label: "RLM max depth",
            description: "Set the maximum child nesting depth for this session",
            current: current.rlm_depth.clone(),
            values: None,
            submenu: None,
        },
        SettingsMenuRow {
            id: "rlm-max-depth-global",
            label: "RLM max depth (global)",
            description: "Set the depth for this session and save it as the global default",
            current: "configure".into(),
            values: None,
            submenu: None,
        },
        SettingsMenuRow {
            id: "theme",
            label: "Theme",
            description: "Color theme for the interface",
            current: current.theme.clone(),
            values: None,
            submenu: Some(SettingsSubmenu::Theme {
                themes: current.available_themes.clone(),
            }),
        },
        SettingsMenuRow {
            id: "child-models",
            label: "Child allowed models",
            description: "Select which models subagents can use in this session family",
            current: "Unavailable".into(),
            values: None,
            submenu: None,
        },
        SettingsMenuRow {
            id: "child-effort",
            label: "Child effort",
            description: "Override reasoning effort for newly spawned subagents in this session family",
            current: "Unavailable".into(),
            values: None,
            submenu: None,
        },
    ]
}

/// The row values the menu opens with: the daemon state plus the settings
/// seam reads.
#[derive(Debug, Clone, Default)]
pub struct SettingsCurrentValues {
    pub factory_enabled: Option<bool>,
    pub speed_display: bool,
    pub telemetry_enabled: Option<bool>,
    pub trace_sharing: Option<bool>,
    pub update_channel: Option<String>,
    pub autocompact: bool,
    pub compaction_threshold_percent: f64,
    pub tool_output_reminder_percent: Option<u8>,
    pub show_images: bool,
    pub auto_resize_images: bool,
    pub block_images: bool,
    pub skill_commands: bool,
    pub builtin_skills: bool,
    pub harness_refinements: bool,
    pub provider_block_delivery: String,
    pub provider_block_target_session: Option<String>,
    pub provider_block_targets: Vec<ProviderBlockTargetChoice>,
    pub web_search_provider: String,
    pub hardware_cursor: bool,
    pub emoji_shortcodes: bool,
    pub editor_padding: u64,
    pub autocomplete_max_visible: u64,
    pub clear_on_shrink: bool,
    pub terminal_progress: bool,
    pub idle_eviction_minutes: String,
    pub steering_mode: String,
    pub follow_up_mode: String,
    pub transport: String,
    pub default_service_tier: String,
    pub primary_model: String,
    pub session_service_tier: String,
    pub available_service_tiers: Vec<&'static str>,
    pub fast_mode_available: bool,
    pub rlm_depth: String,
    pub mermaid: String,
    pub quiet_startup: bool,
    pub tree_filter_mode: String,
    pub warnings_anthropic_extra_usage: bool,
    pub thinking_level: Option<String>,
    pub available_thinking_levels: Vec<String>,
    pub theme: String,
    pub available_themes: Vec<String>,
}

impl SettingsMenu {
    #[must_use]
    pub fn new(rows: Vec<SettingsMenuRow>) -> Self {
        let tabs = tabs::row_indices(&rows)
            .into_iter()
            .map(|(name, rows)| SettingsTab {
                name,
                filtered: rows.clone(),
                rows,
                search: SearchInput::new(),
                selected: 0,
            })
            .collect();
        SettingsMenu {
            tabs,
            tab: 0,
            rows,
            sub: None,
            depth_edit: None,
            autonomous_edit: None,
            model_policy: None,
            global_model_job: None,
            child_session_file: None,
            max_visible: 10,
        }
    }

    pub(crate) fn bind_child_policy(&mut self, session_file: String, policy: &ChildPolicy) {
        self.child_session_file = Some(session_file);
        for row in &mut self.rows {
            match row.id {
                "child-models" => row.current = policy_label(policy),
                "child-effort" => {
                    row.current = child_effort_label(policy.effort).into();
                    row.submenu = Some(SettingsSubmenu::ChildEffort);
                }
                _ => {}
            }
        }
    }

    pub(crate) fn open_child_models(&mut self, choices: ChildModelChoices) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == "child-models") {
            row.current = policy_label(&choices.policy);
        }
        self.model_policy = Some(ModelPolicyPicker::child(choices));
    }

    pub(crate) fn open_global_models(&mut self, choices: GlobalModelChoices) {
        self.model_policy = Some(ModelPolicyPicker::global(choices));
    }

    pub(crate) fn global_models_saved(&mut self) {
        self.model_policy = None;
    }

    pub(crate) fn child_models_saved(&mut self, model_rules: Option<usize>) {
        let Some(mut policy) = self
            .model_policy
            .take()
            .and_then(ModelPolicyPicker::into_child_policy)
        else {
            return;
        };
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == "child-models") {
            policy.model_rules = model_rules;
            row.current = policy_label(&policy);
        }
    }

    pub(crate) fn child_effort_saved(&mut self, effort: Option<ModelThinkingLevel>) {
        if let Some(sub) = self.sub.take() {
            self.rows[sub.row].current = child_effort_label(effort).into();
        }
    }

    fn active_mut(&mut self) -> &mut SettingsTab {
        &mut self.tabs[self.tab]
    }

    pub(crate) fn set_row_value(&mut self, id: &str, value: &str) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
            row.current.clear();
            row.current.push_str(value);
        }
    }

    /// One key id: the submenu, when open, takes it first.
    pub fn handle_key(&mut self, key: &str, kb: &KeybindingsManager) -> SettingsMenuAction {
        if self.global_model_job.is_some() {
            // A load can be discarded; once Enter confirms a save, wait for its result.
            if self.model_policy.is_none()
                && (kb.matches(key, "tui.select.cancel") || key == "ctrl+c")
            {
                self.global_model_job = None;
                return SettingsMenuAction::SubmenuClosed;
            }
            return SettingsMenuAction::None;
        }
        if let Some(input) = &mut self.autonomous_edit {
            if kb.matches(key, "tui.select.cancel") || key == "ctrl+c" {
                self.autonomous_edit = None;
                return SettingsMenuAction::SubmenuClosed;
            }
            if kb.matches(key, "tui.select.confirm") {
                let value = input.value().trim().to_owned();
                self.autonomous_edit = None;
                return SettingsMenuAction::Change { id: "autonomous", value };
            }
            input.handle_key(key, kb);
            return SettingsMenuAction::None;
        }
        if let Some(edit) = &mut self.depth_edit {
            if kb.matches(key, "tui.select.cancel") || key == "ctrl+c" {
                self.depth_edit = None;
                return SettingsMenuAction::SubmenuClosed;
            }
            if kb.matches(key, "tui.select.confirm") {
                let value = edit.input.value();
                if let Some(depth) = value.parse::<u64>().ok().filter(|_| value.bytes().all(|byte| byte.is_ascii_digit())) {
                    let global = edit.global;
                    self.depth_edit = None;
                    return SettingsMenuAction::SaveRlmDepth { depth, global };
                }
                edit.invalid = true;
            } else {
                edit.input.handle_key(key, kb);
                edit.invalid = false;
            }
            return SettingsMenuAction::None;
        }
        if let Some(picker) = &mut self.model_policy {
            let action = picker.handle_key(key, kb);
            if action == SettingsMenuAction::SubmenuClosed {
                self.model_policy = None;
            }
            return action;
        }
        if let Some(mut sub) = self.sub.take() {
            let action = self.handle_submenu_key(&mut sub, key, kb);
            // A value select, a theme restore, or the submenu's Esc closes the
            // submenu; a plain navigation or live preview keeps it open.
            let closed = matches!(
                action,
                SettingsMenuAction::Change { .. }
                    | SettingsMenuAction::RestoreTheme { .. }
                    | SettingsMenuAction::SubmenuClosed
            );
            if !closed {
                self.sub = Some(sub);
            }
            return action;
        }
        // An empty row set carries no tabs: only the close keys act.
        if self.tabs.is_empty() {
            if kb.matches(key, "tui.select.cancel") || key == "ctrl+c" {
                return SettingsMenuAction::Cancel;
            }
            return SettingsMenuAction::None;
        }
        // Tab switching (the operator's 2026-09-28 rebind: the tabs move with Tab and the number
        // keys ONLY, freeing the arrows for value cycling): digits jump straight to their tab while
        // the search field is empty (an active query takes digits as search text).
        match key {
            "shift+tab" => {
                self.switch_tab((self.tab + self.tabs.len() - 1) % self.tabs.len());
                return SettingsMenuAction::None;
            }
            "tab" => {
                self.switch_tab((self.tab + 1) % self.tabs.len());
                return SettingsMenuAction::None;
            }
            _ => {}
        }
        // The arrows cycle the focused setting's value in place (a submenu row
        // has no inline values, so they no-op there).
        if key == "left" {
            return self.cycle_selected(-1);
        }
        if key == "right" {
            return self.cycle_selected(1);
        }
        if self.tabs[self.tab].search.value().is_empty() {
            if let [character] = key.chars().collect::<Vec<char>>()[..] {
                if let Some(tab) = character
                    .to_digit(10)
                    .filter(|digit| *digit > 0)
                    .map(|digit| digit as usize - 1)
                    .filter(|tab| *tab < self.tabs.len())
                {
                    self.switch_tab(tab);
                    return SettingsMenuAction::None;
                }
            }
        }
        if kb.matches(key, "tui.select.up") {
            let tab = self.active_mut();
            if !tab.filtered.is_empty() {
                tab.selected = if tab.selected == 0 {
                    tab.filtered.len() - 1
                } else {
                    tab.selected - 1
                };
            }
            return SettingsMenuAction::None;
        }
        if kb.matches(key, "tui.select.down") {
            let tab = self.active_mut();
            if !tab.filtered.is_empty() {
                tab.selected = (tab.selected + 1) % tab.filtered.len();
            }
            return SettingsMenuAction::None;
        }
        if key == "space" || kb.matches(key, "tui.select.confirm") {
            return self.activate_selected(key);
        }
        if kb.matches(key, "tui.select.cancel") || key == "ctrl+c" {
            return SettingsMenuAction::Cancel;
        }
        // TS `SettingsList.handleInput`: Space stays the row activation
        // above; every other key id goes whole to the active tab's search
        // input.
        let tab = self.active_mut();
        let previous = tab.search.value().to_string();
        tab.search.handle_key(key, kb);
        // The filter re-runs only when the query changed (the config
        // selector's rule): a caret-only key keeps the selection.
        let changed = tab.search.value() != previous;
        if changed {
            self.apply_filter();
        }
        SettingsMenuAction::None
    }

    /// One bracketed paste into the active tab's search field (TS routes
    /// the raw paste data to the `Input`): the sanitize strips the spaces
    /// (Space stays the row activation), and the filter re-runs when the
    /// query changed.
    pub fn paste(&mut self, text: &str) {
        if self.global_model_job.is_some() {
            return;
        }
        if let Some(input) = &mut self.autonomous_edit {
            input.paste(text);
            return;
        }
        if let Some(edit) = &mut self.depth_edit {
            edit.input.paste(text);
            return;
        }
        if let Some(picker) = &mut self.model_policy {
            picker.paste(text);
            return;
        }
        // The submenu owns the frame while open (its key dispatch takes
        // every key first): a paste never edits the hidden parent list.
        if self.sub.is_some() {
            return;
        }
        // An empty row set carries no tabs (the menu renders its empty
        // state): nothing to paste into.
        if self.tabs.is_empty() {
            return;
        }
        let sanitized = text.replace(' ', "");
        let tab = self.active_mut();
        let previous = tab.search.value().to_string();
        tab.search.paste(&sanitized);
        let changed = tab.search.value() != previous;
        if changed {
            self.apply_filter();
        }
    }

    /// Enter/Space on the selection (TS `activateItem`): submenus open;
    /// value rows cycle to the next value.
    fn activate_selected(&mut self, key: &str) -> SettingsMenuAction {
        let Some(tab) = self.tabs.get(self.tab) else {
            return SettingsMenuAction::None;
        };
        let Some(&row_index) = tab.filtered.get(tab.selected) else {
            return SettingsMenuAction::None;
        };
        let row = &mut self.rows[row_index];
        match row.id {
            "telemetry-status" if row.current != "Unavailable" => return SettingsMenuAction::ShowTelemetryStatus,
            "autonomous" => {
                self.autonomous_edit = Some(SearchInput::new());
                return SettingsMenuAction::None;
            }
            "primary-model" if key == "space" => return SettingsMenuAction::OpenGlobalModels,
            "primary-model" => return SettingsMenuAction::OpenPrimaryModel,
            "fast-mode" if row.current != "Unavailable" => {
                return SettingsMenuAction::Change {
                    id: row.id,
                    value: if row.current == "on" { "default" } else { "priority" }.into(),
                };
            }
            "rlm-max-depth" | "rlm-max-depth-global" => {
                self.depth_edit = Some(DepthEdit {
                    global: row.id == "rlm-max-depth-global",
                    input: SearchInput::new(),
                    invalid: false,
                });
                return SettingsMenuAction::None;
            }
            _ => {}
        }
        if row.id == "child-models" && self.child_session_file.is_some() {
            return SettingsMenuAction::OpenChildModels;
        }
        if let Some(kind) = row.submenu.clone() {
            // The tier submenu preselects the row's current tier.
            let selected = match &kind {
                SettingsSubmenu::ServiceTier => SERVICE_TIER_CHOICES
                    .iter()
                    .position(|tier| *tier == row.current)
                    .unwrap_or(0),
                SettingsSubmenu::Thinking { levels } => levels
                    .iter()
                    .position(|level| *level == row.current)
                    .unwrap_or(0),
                SettingsSubmenu::SessionServiceTier { tiers } => tiers
                    .iter()
                    .position(|tier| *tier == row.current)
                    .unwrap_or(0),
                SettingsSubmenu::ChildEffort => CHILD_EFFORT_CHOICES
                    .iter()
                    .position(|&effort| child_effort_label(effort) == row.current)
                    .unwrap_or(0),
                SettingsSubmenu::ProviderBlockTarget { sessions } => sessions
                    .iter()
                    .position(|session| session.label == row.current)
                    .unwrap_or(0),
                _ => 0,
            };
            self.sub = Some(SubmenuState {
                row: row_index,
                kind,
                selected,
            });
            return SettingsMenuAction::None;
        }
        self.cycle_selected(1)
    }

    /// Cycle the selected row's value by `delta` steps, wrapping at the list's ends, and report the
    /// change. A submenu row (no inline values) and a missing selection no-op.
    fn cycle_selected(&mut self, delta: isize) -> SettingsMenuAction {
        let Some(tab) = self.tabs.get(self.tab) else {
            return SettingsMenuAction::None;
        };
        let Some(&row_index) = tab.filtered.get(tab.selected) else {
            return SettingsMenuAction::None;
        };
        let row = &mut self.rows[row_index];
        let Some(values) = &row.values else {
            return SettingsMenuAction::None;
        };
        let count = values.len();
        let index = values
            .iter()
            .position(|value| *value == row.current)
            .map_or(0, |index| {
                (index as isize + delta).rem_euclid(count as isize) as usize
            });
        let value = values[index].clone();
        row.current.clone_from(&value);
        SettingsMenuAction::Change { id: row.id, value }
    }

    /// One key inside an open submenu: Enter selects, Esc goes back.
    fn handle_submenu_key(
        &mut self,
        sub: &mut SubmenuState,
        key: &str,
        kb: &KeybindingsManager,
    ) -> SettingsMenuAction {
        let options = match &sub.kind {
            SettingsSubmenu::Thinking { levels } => levels.len(),
            SettingsSubmenu::Theme { themes } => themes.len(),
            SettingsSubmenu::Warnings => 1,
            SettingsSubmenu::ServiceTier => SERVICE_TIER_CHOICES.len(),
            SettingsSubmenu::SessionServiceTier { tiers } => tiers.len(),
            SettingsSubmenu::ChildEffort => CHILD_EFFORT_CHOICES.len(),
            SettingsSubmenu::ProviderBlockTarget { sessions } => sessions.len(),
        };
        if kb.matches(key, "tui.select.up") {
            if options > 0 {
                sub.selected = if sub.selected == 0 {
                    options - 1
                } else {
                    sub.selected - 1
                };
                return Self::submenu_selection_change(sub);
            }
            return SettingsMenuAction::None;
        }
        if kb.matches(key, "tui.select.down") {
            if options > 0 {
                sub.selected = (sub.selected + 1) % options;
                return Self::submenu_selection_change(sub);
            }
            return SettingsMenuAction::None;
        }
        if kb.matches(key, "tui.select.confirm") || key == "space" {
            let row_id = self.rows[sub.row].id;
            let value = match &sub.kind {
                SettingsSubmenu::Thinking { levels } => {
                    levels
                        .get(sub.selected)
                        .cloned()
                        .map(|level| SettingsMenuAction::Change {
                            id: row_id,
                            value: level,
                        })
                }
                SettingsSubmenu::Theme { themes } => {
                    themes
                        .get(sub.selected)
                        .cloned()
                        .map(|name| SettingsMenuAction::Change {
                            id: row_id,
                            value: name,
                        })
                }
                SettingsSubmenu::Warnings => Some(SettingsMenuAction::Change {
                    id: "warnings-anthropic-extra-usage",
                    value: if sub.selected == 0 {
                        "true".into()
                    } else {
                        "false".into()
                    },
                }),
                SettingsSubmenu::ServiceTier => SERVICE_TIER_CHOICES
                    .get(sub.selected)
                    .copied()
                    .map(|tier| SettingsMenuAction::Change {
                        id: row_id,
                        value: tier.to_string(),
                    }),
                SettingsSubmenu::SessionServiceTier { tiers } => tiers
                    .get(sub.selected)
                    .map(|tier| SettingsMenuAction::Change {
                        id: row_id,
                        value: (*tier).to_owned(),
                    }),
                SettingsSubmenu::ChildEffort => {
                    let effort = CHILD_EFFORT_CHOICES[sub.selected];
                    if child_effort_label(effort) == self.rows[sub.row].current {
                        Some(SettingsMenuAction::SubmenuClosed)
                    } else {
                        Some(SettingsMenuAction::SaveChildEffort(effort))
                    }
                }
                SettingsSubmenu::ProviderBlockTarget { sessions } => sessions
                    .get(sub.selected)
                    .map(|session| SettingsMenuAction::SaveProviderBlockTarget {
                        selector: session.selector.clone(),
                        label: session.label.clone(),
                    }),
            };
            // A value select closes the submenu and updates the row's displayed value.
            if let Some(action) = value {
                if !matches!(row_id, "thinking" | "session-service-tier") {
                    if let SettingsMenuAction::Change { value, .. } = &action {
                        self.rows[sub.row].current.clone_from(value);
                    }
                }
                return action;
            }
            return SettingsMenuAction::None;
        }
        if kb.matches(key, "tui.select.cancel") || key == "ctrl+c" {
            // Theme: Esc restores the row's theme; every submenu just goes back.
            return self.submenu_cancel(sub);
        }
        SettingsMenuAction::None
    }

    /// The selection-change side effect: only the theme submenu previews live.
    fn submenu_selection_change(sub: &SubmenuState) -> SettingsMenuAction {
        match &sub.kind {
            SettingsSubmenu::Theme { themes } => themes
                .get(sub.selected)
                .map_or(SettingsMenuAction::None, |name| {
                    SettingsMenuAction::PreviewTheme { name: name.clone() }
                }),
            _ => SettingsMenuAction::None,
        }
    }

    /// The submenu's Esc behavior: the theme submenu restores the theme the
    /// row opened with.
    fn submenu_cancel(&self, sub: &SubmenuState) -> SettingsMenuAction {
        match &sub.kind {
            SettingsSubmenu::Theme { .. } => SettingsMenuAction::RestoreTheme {
                name: self.rows[sub.row].current.clone(),
            },
            // Every other submenu just goes back; the menu itself stays open.
            _ => SettingsMenuAction::SubmenuClosed,
        }
    }

    /// Switch to a tab: a pure focus move, nothing resets.
    fn switch_tab(&mut self, tab: usize) {
        self.tab = tab;
    }

    /// Re-filter the active tab's rows (fuzzy over the label): a fresh query
    /// lands the tab's selection on its first match.
    fn apply_filter(&mut self) {
        let query = self.tabs[self.tab].search.value().to_string();
        let tab = &mut self.tabs[self.tab];
        tab.filtered = if query.is_empty() {
            tab.rows.clone()
        } else {
            let candidates: Vec<SettingsMenuRow> = tab
                .rows
                .iter()
                .map(|&index| self.rows[index].clone())
                .collect();
            crate::fuzzy::fuzzy_filter(&candidates, &query, |row| row.label.to_string())
                .iter()
                .map(|row| {
                    tab.rows
                        .iter()
                        .find(|&index| &self.rows[*index] == row)
                        .copied()
                        .expect("fuzzy keeps row values")
                })
                .collect()
        };
        tab.selected = 0;
    }

    /// Render the shared menu panel over the settings rows; a submenu replaces
    /// the whole list.
    #[must_use]
    pub fn render(&self, theme: &Theme, width: usize, kb: &KeybindingsManager) -> Vec<crate::Line> {
        if self.global_model_job.is_some() {
            let status = if self.model_policy.is_some() {
                "Saving global model settings…"
            } else {
                "Loading global model settings…"
            };
            return vec![hint_row(theme, width, status)];
        }
        if let Some(input) = &self.autonomous_edit {
            let mut lines = vec![hint_row(theme, width, "Autonomous mode · Enter to apply, Esc to cancel")];
            lines.extend(crate::menu_panel::search_field_lines(theme, width, input.value(), input.cursor(), true, "status, off, or on [options]"));
            for hint in ["--max-continuations, --max-turns, --max-tokens, --timeout-ms: number or unlimited",
                "--gate <command>, --gate-retries <n>, --gate-timeout-ms <n>, --subagent-keep-alive-ms <n>"] {
                lines.push(hint_row(theme, width, hint));
            }
            return lines;
        }
        if let Some(edit) = &self.depth_edit {
            let title = if edit.global {
                "RLM max depth (session and global default)"
            } else {
                "RLM max depth (session)"
            };
            let mut lines = vec![
                crate::menu_panel::rule_row(theme, width),
                hint_row(theme, width, title),
            ];
            lines.extend(crate::menu_panel::search_field_lines(
                theme,
                width,
                edit.input.value(),
                edit.input.cursor(),
                true,
                "Non-negative integer",
            ));
            if edit.invalid {
                lines.push(hint_row(theme, width, "Enter a non-negative integer."));
            }
            lines.push(hint_row(theme, width, &submenu_hint(kb)));
            return lines;
        }
        if let Some(picker) = &self.model_policy {
            return picker.render(theme, width, kb);
        }
        if let Some(sub) = &self.sub {
            return Self::render_submenu(sub, theme, width, kb);
        }
        let mut lines: Vec<crate::Line> = Vec::new();
        if self.rows.is_empty() {
            lines.push(crate::menu_panel::no_match_row(
                theme,
                width,
                "No settings available",
            ));
            lines.push(hint_row(theme, width, &hint(kb, 0)));
            return lines;
        }
        let tab = &self.tabs[self.tab];
        lines.extend(crate::menu_panel::search_field_lines(
            theme,
            width,
            tab.search.value(),
            tab.search.cursor(),
            true,
            "Search settings",
        ));
        // The tab strip sits under the search field with a blank row on either
        // side (the operator's 2026-09-28 spacing pass).
        lines.push(Vec::new());
        let names: Vec<&'static str> = self.tabs.iter().map(|tab| tab.name).collect();
        lines.push(tabs::strip_row(theme, width, &names, self.tab));
        lines.push(Vec::new());
        if tab.filtered.is_empty() {
            lines.push(crate::menu_panel::no_match_row(
                theme,
                width,
                "No matching settings",
            ));
            lines.push(hint_row(theme, width, &hint(kb, self.tabs.len())));
            return lines;
        }
        let start = tab
            .selected
            .saturating_sub(self.max_visible / 2)
            .min(tab.filtered.len().saturating_sub(self.max_visible));
        let end = (start + self.max_visible).min(tab.filtered.len());
        for (position, &row_index) in tab.filtered[start..end].iter().enumerate() {
            let position = start + position;
            let row = &self.rows[row_index];
            let selected = position == tab.selected;
            lines.push(crate::menu_panel::menu_row(
                theme,
                width,
                vec![crate::Span::raw(row.label)],
                &[crate::menu_panel::MenuSegment::muted(&row.current)],
                selected,
            ));
        }
        if start > 0 || end < tab.filtered.len() {
            lines.push(crate::menu_panel::scroll_row(
                theme,
                width,
                tab.selected + 1,
                tab.filtered.len(),
            ));
        }
        if let Some(&row_index) = tab.filtered.get(tab.selected) {
            let row = &self.rows[row_index];
            if !row.description.is_empty() {
                lines.push(Vec::new());
                for line in wrap_text(row.description, width.saturating_sub(4)) {
                    let plain: String = line.iter().map(|span| span.content.as_str()).collect();
                    lines.push(crate::width::truncate_line(
                        &vec![
                            crate::Span::raw("  ".to_string()),
                            crate::Span::styled(plain, theme.fg_style(ThemeColor::Dim)),
                        ],
                        width,
                        "",
                    ));
                }
                // The separator rule closes the detail block (the operator's 2026-09-28
                // directive).
                lines.push(crate::menu_panel::rule_row(theme, width));
            }
        }
        lines.push(hint_row(theme, width, &hint(kb, self.tabs.len())));
        lines
    }

    /// The submenu render: accent title, muted description, the shared menu
    /// rows, and the back hint.
    fn render_submenu(
        sub: &SubmenuState,
        theme: &Theme,
        width: usize,
        kb: &KeybindingsManager,
    ) -> Vec<crate::Line> {
        let (title, description, options): (&str, &str, Vec<(String, Option<String>)>) =
            match &sub.kind {
                SettingsSubmenu::Thinking { levels } => (
                    "Thinking Level",
                    "Select reasoning depth for thinking-capable models",
                    levels
                        .iter()
                        .map(|level| {
                            (level.clone(), Some(thinking_description(level).to_owned()))
                        })
                        .collect(),
                ),
                SettingsSubmenu::Theme { themes } => (
                    "Theme",
                    "Select color theme",
                    themes.iter().map(|name| (name.clone(), None)).collect(),
                ),
                SettingsSubmenu::Warnings => (
                    "Warnings",
                    "Enable or disable individual warnings",
                    vec![(String::from("Anthropic extra usage"), None)],
                ),
                SettingsSubmenu::ServiceTier => (
                    "Default Service Tier",
                    "Service tier for new sessions; applies to the current session when the model supports it",
                    SERVICE_TIER_CHOICES
                        .iter()
                        .map(|tier| {
                            (tier.to_string(), Some(service_tier_description(tier).to_owned()))
                        })
                        .collect(),
                ),
                SettingsSubmenu::SessionServiceTier { tiers } => (
                    "Session Service Tier",
                    "Choose a tier supported by the current model",
                    tiers.iter()
                        .map(|tier| {
                            (
                                (*tier).to_owned(),
                                Some(service_tier_description(tier).to_owned()),
                            )
                        })
                        .collect(),
                ),
                SettingsSubmenu::ChildEffort => (
                    "Child effort",
                    "Effort override for new subagents; inherit uses the spawn request or parent",
                    CHILD_EFFORT_CHOICES
                        .iter()
                        .map(|&effort| {
                            let label = child_effort_label(effort);
                            (
                                label.to_owned(),
                                effort.map(|_| thinking_description(label).to_owned()),
                            )
                        })
                        .collect(),
                ),
                SettingsSubmenu::ProviderBlockTarget { sessions } => (
                    "Provider Rejection Target",
                    "Route sanitized reports to this existing independent session's inbox",
                    sessions
                        .iter()
                        .map(|session| {
                            (session.label.clone(), Some(session.selector.clone()))
                        })
                        .collect(),
                ),
            };
        let mut lines: Vec<crate::Line> = Vec::new();
        // The list view's full-width top rule stays over the submenu too (the
        // operator's 2026-09-28 regression pin).
        lines.push(crate::menu_panel::rule_row(theme, width));
        // The setting's name and description carry the list rows' padding-x (the
        // operator's 2026-09-28 regression pin).
        lines.push(crate::width::truncate_line(
            &vec![
                crate::Span::raw("  ".to_string()),
                theme.fg_span(ThemeColor::Accent, title.to_string()),
            ],
            width,
            "",
        ));
        if !description.is_empty() {
            lines.push(Vec::new());
            lines.push(crate::width::truncate_line(
                &vec![
                    crate::Span::raw("  ".to_string()),
                    theme.fg_span(ThemeColor::Muted, description.to_string()),
                ],
                width,
                "",
            ));
        }
        lines.push(Vec::new());
        let max_visible = options.len().min(10);
        let start = sub
            .selected
            .saturating_sub(max_visible / 2)
            .min(options.len().saturating_sub(max_visible));
        let end = (start + max_visible).min(options.len());
        for (position, (value, description)) in options[start..end].iter().enumerate() {
            let position = start + position;
            let selected = position == sub.selected;
            let trailing: Vec<crate::menu_panel::MenuSegment> = description
                .as_ref()
                .map(|description| {
                    vec![crate::menu_panel::MenuSegment::muted(description.as_str())]
                })
                .unwrap_or_default();
            lines.push(crate::menu_panel::menu_row(
                theme,
                width,
                vec![crate::Span::raw(value.clone())],
                &trailing,
                selected,
            ));
        }
        lines.push(Vec::new());
        lines.push(hint_row(theme, width, &submenu_hint(kb)));
        lines
    }
}

/// The settings page's own key-hint row: this surface's keyboard-shortcuts row carries the
/// two-space inner column the detail block aligns on (the operator's 2026-09-28 padding pass).
fn hint_row(theme: &Theme, width: usize, hint: &str) -> crate::Line {
    let line = vec![
        crate::Span::raw("  ".to_string()),
        theme.fg_span(ThemeColor::Dim, hint.to_string()),
    ];
    crate::width::truncate_line(&line, width, "")
}

/// The menu's key hint: Space is a literal key the menu always handles; an
/// unbound Enter drops its label, an unbound Esc drops the close segment.
fn hint(kb: &KeybindingsManager, tabs: usize) -> String {
    let mut segments = vec!["Type to search".to_string()];
    if tabs > 0 {
        segments.push(format!("{}/1-{tabs} tabs", format_key_text("tab")));
    }
    let arrows = format!("{}/{}", format_key_text("left"), format_key_text("right"));
    segments.push(match kb.first_key("tui.select.confirm") {
        Some(key) => format!("{arrows}/{}/Space change", format_key_text(&key)),
        None => format!("{arrows}/Space change"),
    });
    if let Some(close) = crate::menu_panel::key_hint(kb, &["tui.select.cancel"], "close") {
        segments.push(close);
    }
    segments.join(" · ")
}

/// The submenu's key hint: an unbound action is omitted, never advertised
/// with a default key.
fn submenu_hint(kb: &KeybindingsManager) -> String {
    [
        crate::menu_panel::key_hint(kb, &["tui.select.confirm"], "select"),
        crate::menu_panel::key_hint(kb, &["tui.select.cancel"], "back"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<String>>()
    .join(" · ")
}

#[cfg(test)]
mod menu_tests;
