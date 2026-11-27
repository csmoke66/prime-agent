//! The `bh-web` host: credential flows, harness stores, and gist sharing, implemented over the same
//! `bh-core` seams the TUI uses (`ProviderAuth`, `TerminalMcpAuth`, `CliClientSettings`). The login
//! flows drive an inline auth panel handle; this module forwards the panel's requests to the page.

use std::path::{Path, PathBuf};

use bh_tui::auth_panel::{AuthPanelHandle, AuthPanelRequest, PasteStyle};
use bh_tui::client_auth::ClientAuthCommands;
use bh_tui::client_settings::ClientSettings;
use bh_tui::export_share::{gist_outcome, probe_gh_auth, spawn_gist_create, GhAuthStatus};
use bh_tui::provider_auth::{
    AuthStatusIndicator, AuthType as TuiAuthType, ProviderAuthCommands, ProviderAuthOutcome,
    ProviderRow as TuiProviderRow,
};
use bh_web::{
    AuthMode, AuthType, FlowOutcome, FlowRequest, FlowUi, PromptInput, ProviderRow, ShareLinks,
    WebHost,
};
use serde_json::Value;

use crate::client_settings::CliClientSettings;
use crate::mcp_login::TerminalMcpAuth;
use crate::provider_login::ProviderAuth;

pub(crate) struct CliWebHost {
    cwd: PathBuf,
    agent_dir: PathBuf,
}

impl CliWebHost {
    pub(crate) fn new(cwd: PathBuf, agent_dir: PathBuf) -> Self {
        CliWebHost { cwd, agent_dir }
    }

    fn providers(&self) -> ProviderAuth {
        ProviderAuth::new(&self.cwd, &self.agent_dir)
    }

    fn settings(&self) -> std::sync::Arc<CliClientSettings> {
        CliClientSettings::new(self.cwd.clone(), self.agent_dir.clone())
    }

    async fn tui_row(&self, mode: AuthMode, id: &str) -> Option<TuiProviderRow> {
        let rows = match mode {
            AuthMode::Login => self.providers().login_options().await,
            AuthMode::Logout => self.providers().logout_options().await,
        };
        rows.into_iter().find(|row| row.id == id)
    }

    async fn run_provider_flow(
        &self,
        mode: AuthMode,
        provider: &str,
        action: impl AsyncFnOnce(&ProviderAuth, &TuiProviderRow) -> ProviderAuthOutcome,
    ) -> FlowOutcome {
        match self.tui_row(mode, provider).await {
            Some(row) => outcome_of(action(&self.providers(), &row).await),
            None => FlowOutcome::Error(format!("Unknown provider {provider}.")),
        }
    }

    async fn run_request(&self, request: FlowRequest, panel: AuthPanelHandle) -> FlowOutcome {
        match request {
            FlowRequest::ProviderKey { provider, key } => {
                self.run_provider_flow(AuthMode::Login, &provider, async |auth, row| {
                    auth.login(row, Some(&key)).await
                })
                .await
            }
            FlowRequest::ProviderLogin { provider } => {
                self.run_provider_flow(AuthMode::Login, &provider, async |auth, row| {
                    auth.login_on_panel(row, panel).await
                })
                .await
            }
            FlowRequest::ProviderLogout { provider } => {
                self.run_provider_flow(AuthMode::Logout, &provider, async |auth, row| {
                    auth.logout(row).await
                })
                .await
            }
            FlowRequest::McpLogin { server } => mcp_outcome(self.mcp().login(&server, panel).await),
            FlowRequest::McpPaste { server } => {
                mcp_outcome(self.mcp().paste_token(&server, panel).await)
            }
            FlowRequest::McpKey { credential } => {
                mcp_outcome(self.mcp().api_key(&credential, panel).await)
            }
            FlowRequest::McpLogout { server } => mcp_outcome(self.mcp().logout(&server).await),
        }
    }

    fn mcp(&self) -> TerminalMcpAuth {
        TerminalMcpAuth::new(&self.cwd, &self.agent_dir)
    }

    /// Adoption telemetry for one finished flow; delivered by the background worker.
    fn track(&self, request: &FlowRequest, outcome: &FlowOutcome) {
        let flow = match request {
            FlowRequest::ProviderKey { .. } => "provider_key",
            FlowRequest::ProviderLogin { .. } => "provider_login",
            FlowRequest::ProviderLogout { .. } => "provider_logout",
            FlowRequest::McpLogin { .. } => "mcp_login",
            FlowRequest::McpPaste { .. } => "mcp_paste",
            FlowRequest::McpKey { .. } => "mcp_key",
            FlowRequest::McpLogout { .. } => "mcp_logout",
        };
        let outcome = match outcome {
            FlowOutcome::Status(_) => "status",
            FlowOutcome::Error(_) => "error",
            FlowOutcome::Cancelled => "cancelled",
        };
        let (cwd, agent_dir) = (self.cwd.clone(), self.agent_dir.clone());
        tokio::spawn(async move {
            let settings = bh_core::settings::SettingsManager::create(&cwd, &agent_dir);
            if crate::mode::telemetry_disabled(&settings) {
                return;
            }
            let client = bh_core::session_engine::telemetry::build_client(&settings, &agent_dir);
            let mut properties = bh_telemetry::base_properties("interactive");
            properties.set("flow", serde_json::json!(flow));
            properties.set("outcome", serde_json::json!(outcome));
            client.track("web credential flow finished", properties);
            let _ = client.shutdown().await;
        });
    }
}

fn outcome_of(outcome: ProviderAuthOutcome) -> FlowOutcome {
    match outcome {
        ProviderAuthOutcome::Status(message) => FlowOutcome::Status(message),
        ProviderAuthOutcome::Error(message) => FlowOutcome::Error(message),
        ProviderAuthOutcome::Cancelled => FlowOutcome::Cancelled,
    }
}

fn mcp_outcome(result: anyhow::Result<String>) -> FlowOutcome {
    match result {
        Ok(status) => FlowOutcome::Status(status),
        Err(error) => FlowOutcome::Error(format!("{error:#}")),
    }
}

fn web_row(row: TuiProviderRow) -> ProviderRow {
    ProviderRow {
        id: row.id,
        name: row.name,
        auth_type: match row.auth_type {
            TuiAuthType::Oauth => AuthType::Oauth,
            TuiAuthType::ApiKey => AuthType::ApiKey,
        },
        status: row.status.map(|AuthStatusIndicator { label, .. }| label),
        configured: row.configured,
        available: row.available,
    }
}

/// Forward the panel's requests to the page until the flow drops its handle.
async fn forward_panel(mut requests: tokio::sync::mpsc::UnboundedReceiver<AuthPanelRequest>, ui: FlowUi) {
    while let Some(request) = requests.recv().await {
        match request {
            AuthPanelRequest::Progress { message, .. } => ui.progress(message),
            AuthPanelRequest::Waiting { message } => ui.waiting(message),
            AuthPanelRequest::AuthUrl { url, instructions } => {
                ui.auth_url(&url, instructions.as_deref());
            }
            AuthPanelRequest::PastePrompt {
                prompt,
                style,
                allow_empty,
                reply,
                ..
            } => {
                let ui = ui.clone();
                tokio::spawn(async move {
                    let input = PromptInput {
                        masked: style == PasteStyle::Masked,
                        allow_empty,
                    };
                    let _ = reply.send(ui.prompt(&prompt, input).await);
                });
            }
            AuthPanelRequest::ProviderSettled { .. } | AuthPanelRequest::McpSettled { .. } => {}
        }
    }
}

impl WebHost for CliWebHost {
    async fn provider_rows(&self, mode: AuthMode) -> Vec<ProviderRow> {
        let rows = match mode {
            AuthMode::Login => self.providers().login_options().await,
            AuthMode::Logout => self.providers().logout_options().await,
        };
        rows.into_iter().map(web_row).collect()
    }

    async fn run_flow(&self, request: FlowRequest, ui: FlowUi) -> FlowOutcome {
        let (tx, requests) = tokio::sync::mpsc::unbounded_channel();
        let panel = AuthPanelHandle::new(tx);
        let forwarder = tokio::spawn(forward_panel(requests, ui.clone()));
        let canceller = {
            let panel = panel.clone();
            tokio::spawn(async move {
                ui.cancelled_wait().await;
                panel.cancel();
            })
        };
        let outcome = self.run_request(request.clone(), panel).await;
        forwarder.abort();
        canceller.abort();
        self.track(&request, &outcome);
        outcome
    }

    fn harness_entries(&self, session_file: Option<&str>) -> Result<Vec<Value>, String> {
        self.settings()
            .harness_entries(session_file)
            .map_err(|error| format!("{error:#}"))
    }

    fn delete_harness_entry(&self, session_file: Option<&str>, entry: &Value) -> Result<(), String> {
        self.settings()
            .delete_harness_entry(session_file, entry)
            .map_err(|error| format!("{error:#}"))
    }

    async fn share_gist(&self, file: &Path) -> Result<ShareLinks, String> {
        match probe_gh_auth() {
            GhAuthStatus::NotLoggedIn => {
                return Err("GitHub CLI is not logged in. Run 'gh auth login' first.".to_string());
            }
            GhAuthStatus::NotInstalled => {
                return Err(
                    "GitHub CLI (gh) is not installed. Install it from https://cli.github.com/"
                        .to_string(),
                );
            }
            GhAuthStatus::Ok => {}
        }
        let child = spawn_gist_create(file).map_err(|error| error.to_string())?;
        let outcome = gist_outcome(child).await?;
        Ok(ShareLinks {
            gist_url: outcome.gist_url,
            preview_url: outcome.preview_url,
        })
    }
}
