//! The update-channel setting persists to the source used by `/update`.
#![cfg(unix)]
#![warn(clippy::allow_attributes_without_reason)]

mod support;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use pa_tui::interactive::{HeadlessPlan, HeadlessStep, InteractiveOptions};

use support::{
    attach_response, create_response, detach_response, options, run_headless, serve_connection,
    session_stats_response, write_json, MockSupervisor, Request,
};

impl MockSupervisor {
    fn serve(self) {
        let (stream, _) = self.listener.accept().expect("accept");
        serve_connection(stream, &[], |writer, Request { id, kind, .. }| {
            match kind {
                "create" => {
                    write_json(writer, &create_response(id));
                }
                "attach" => {
                    write_json(writer, &attach_response(id, "nightly session"));
                }
                "get_session_stats" => {
                    write_json(writer, &session_stats_response(id));
                }
                "detach" => {
                    write_json(writer, &detach_response(id));
                }
                _ => return false,
            }
            true
        });
    }
}

/// A minimal settings seam for the harness: every getter returns its TS default, writes succeed
/// without persistence, and the channel pair resolves like the composition root.
#[derive(Default)]
struct StubSettings {
    update_channel: std::sync::Mutex<Option<String>>,
}

impl pa_tui::client_settings::ClientSettings for StubSettings {
    fn compaction_threshold_percent(&self) -> f64 {
        95.0
    }
    fn set_compaction_threshold_percent(&self, _percent: f64) -> Result<()> {
        Ok(())
    }

    fn theme(&self) -> Option<String> {
        None
    }
    fn set_theme(&self, _theme: &str) -> Result<()> {
        Ok(())
    }
    fn show_images(&self) -> bool {
        true
    }
    fn set_show_images(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn clear_on_shrink(&self) -> bool {
        false
    }
    fn set_clear_on_shrink(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn show_terminal_progress(&self) -> bool {
        false
    }
    fn set_show_terminal_progress(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn image_auto_resize(&self) -> bool {
        true
    }
    fn set_image_auto_resize(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn block_images(&self) -> bool {
        false
    }
    fn set_block_images(&self, _blocked: bool) -> Result<()> {
        Ok(())
    }
    fn image_model(&self) -> Option<String> {
        None
    }
    fn enable_skill_commands(&self) -> bool {
        true
    }
    fn set_enable_skill_commands(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn enable_builtin_skills(&self) -> bool {
        true
    }
    fn set_enable_builtin_skills(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn show_hardware_cursor(&self) -> bool {
        false
    }
    fn set_show_hardware_cursor(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn editor_padding_x(&self) -> u64 {
        0
    }
    fn set_editor_padding_x(&self, _padding: u64) -> Result<()> {
        Ok(())
    }
    fn autocomplete_max_visible(&self) -> u64 {
        5
    }
    fn set_autocomplete_max_visible(&self, _max_visible: u64) -> Result<()> {
        Ok(())
    }
    fn quiet_startup(&self) -> bool {
        false
    }
    fn set_quiet_startup(&self, _quiet: bool) -> Result<()> {
        Ok(())
    }
    fn idle_eviction_minutes(&self) -> String {
        "90".to_string()
    }
    fn set_idle_eviction_minutes(&self, _value: &str) -> Result<()> {
        Ok(())
    }
    fn mermaid_rendering_mode(&self) -> String {
        "streaming".to_string()
    }
    fn set_mermaid_rendering_mode(&self, _mode: &str) -> Result<()> {
        Ok(())
    }
    fn tree_filter_mode(&self) -> String {
        "user-only".to_string()
    }
    fn set_tree_filter_mode(&self, _mode: &str) -> Result<()> {
        Ok(())
    }
    fn default_service_tier(&self) -> String {
        "default".to_string()
    }
    fn set_default_service_tier(&self, _tier: &str) -> Result<()> {
        Ok(())
    }
    fn chat_detail(&self) -> String {
        "details".to_string()
    }
    fn set_chat_detail(&self, _detail: &str) -> Result<()> {
        Ok(())
    }
    fn factory_enabled(&self) -> bool {
        false
    }
    fn set_factory_enabled(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn warnings_anthropic_extra_usage(&self) -> bool {
        true
    }
    fn set_warnings_anthropic_extra_usage(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
    fn update_channel(&self) -> Option<String> {
        self.update_channel.lock().expect("channel lock").clone()
    }
    fn set_update_channel(&self, channel: &str) -> Result<()> {
        *self.update_channel.lock().expect("channel lock") = Some(channel.to_string());
        Ok(())
    }
    fn telemetry_status(&self) -> String {
        String::new()
    }
    fn set_telemetry_enabled(&self, _enabled: bool) -> Result<String> {
        Ok(String::new())
    }
    fn effective_update_channel(&self, version: &str) -> String {
        if let Some(channel) = self.update_channel() {
            return channel;
        }
        // The inference TS resolveUpdateChannel applies: a -beta* prerelease reads nightly,
        // anything else stable.
        if version.contains("-beta") {
            "nightly".to_string()
        } else {
            "stable".to_string()
        }
    }
}

fn toggle_channel(settings: std::sync::Arc<StubSettings>) {
    run_headless(
        HeadlessPlan {
            steps: vec![
                HeadlessStep::Submit("/settings".into()),
                HeadlessStep::Paste("channel".into()),
                HeadlessStep::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            ],
            width: 100,
            height: 30,
        },
        |socket| InteractiveOptions {
            client_settings: Some(settings),
            ..options(socket)
        },
        |socket| {
            let supervisor = MockSupervisor::bind(socket);
            move || supervisor.serve()
        },
    );
}

#[test]
fn channel_cycles_from_inferred_stable_to_nightly_and_back() {
    let settings = std::sync::Arc::new(StubSettings::default());
    for expected in ["nightly", "stable"] {
        toggle_channel(settings.clone());
        assert_eq!(settings.update_channel.lock().unwrap().as_deref(), Some(expected));
    }
}
