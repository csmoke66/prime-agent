//! The seam to the session engine: provider credentials, MCP logins, harness stores, and gist
//! sharing live above this crate (`bh-core`), so the composition root implements [`WebHost`].
//! Interactive login flows talk to the browser through [`FlowUi`].

use std::future::Future;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::flows::FlowShared;

/// How a provider authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthType {
    Oauth,
    ApiKey,
}

/// One row of the `/login` or `/logout` provider list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRow {
    pub id: String,
    pub name: String,
    pub auth_type: AuthType,
    /// Short status label (`configured`, `env: OPENAI_API_KEY`, ...); `None` hides it.
    pub status: Option<String>,
    pub configured: bool,
    /// Whether this build carries the provider's login flow.
    pub available: bool,
}

/// Which provider list to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMode {
    Login,
    Logout,
}

/// One credential action. Deliberately has no `Debug`: `ProviderKey` carries a secret.
#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FlowRequest {
    /// Store an API key for a provider.
    ProviderKey { provider: String, key: String },
    /// Run a provider's subscription (OAuth) login.
    ProviderLogin { provider: String },
    ProviderLogout { provider: String },
    /// Run an MCP service's OAuth login.
    McpLogin { server: String },
    /// Store a pasted static token for an MCP service.
    McpPaste { server: String },
    /// Store an API-key credential such as the web-search key.
    McpKey { credential: String },
    McpLogout { server: String },
}

/// How a flow ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum FlowOutcome {
    Status(String),
    Error(String),
    Cancelled,
}

/// A secret gist created for `/share`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareLinks {
    pub gist_url: String,
    pub preview_url: String,
}

/// One field of a partial update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldUpdate<T> {
    Keep,
    Clear,
    Set(T),
}

/// What the web server needs from the session engine; implemented by the composition root.
///
/// Implementations must never log or echo secrets (`FlowRequest::ProviderKey`, prompt answers).
pub trait WebHost: Send + Sync + 'static {
    /// The provider rows for `/login` (every loginable provider) or `/logout` (stored credentials).
    fn provider_rows(&self, mode: AuthMode) -> impl Future<Output = Vec<ProviderRow>> + Send;

    /// Run one credential action to completion, reporting progress and prompts through `ui` and
    /// returning promptly once `ui.cancelled()` turns true.
    fn run_flow(
        &self,
        request: FlowRequest,
        ui: FlowUi,
    ) -> impl Future<Output = FlowOutcome> + Send;

    /// Raw harness entries from the global store and the store of `session_file`, if given.
    ///
    /// # Errors
    ///
    /// Returns the store's error text.
    fn harness_entries(&self, session_file: Option<&str>) -> Result<Vec<Value>, String>;

    /// Delete `entry` from the store it came from if it is unchanged.
    ///
    /// # Errors
    ///
    /// Returns the store's error text (for example a stale selection).
    fn delete_harness_entry(&self, session_file: Option<&str>, entry: &Value)
        -> Result<(), String>;

    /// The subagent policy of `session_file`'s root family: `{ modelRules, inherited, effort, models }`
    /// where each model row is `{ selector, name, allowed, inherited }`.
    ///
    /// # Errors
    ///
    /// Returns the store's error text.
    fn child_policy(&self, session_file: &str) -> Result<Value, String>;

    /// Save the family's child models (cleared: inherit the global list; an empty list allows all)
    /// and/or child effort (cleared: no override).
    ///
    /// # Errors
    ///
    /// Returns the store's error text.
    fn set_child_policy(
        &self,
        session_file: &str,
        models: FieldUpdate<Vec<String>>,
        effort: FieldUpdate<String>,
    ) -> Result<(), String>;

    /// Publish `file` as a secret gist.
    ///
    /// # Errors
    ///
    /// Returns the failure text (for example `gh` missing or not logged in).
    fn share_gist(&self, file: &Path) -> impl Future<Output = Result<ShareLinks, String>> + Send;
}

/// The flow side of a browser login: progress lines, the sign-in link, and prompts the user answers
/// in the page.
#[derive(Clone)]
pub struct FlowUi {
    pub(crate) shared: Arc<FlowShared>,
}

impl FlowUi {
    /// A muted progress line.
    pub fn progress(&self, message: impl Into<String>) {
        self.shared.progress(message.into());
    }

    /// The polling flow's waiting line.
    pub fn waiting(&self, message: impl Into<String>) {
        self.shared.waiting(message.into());
    }

    /// The provider sign-in URL the user opens in the browser.
    pub fn auth_url(&self, url: &str, instructions: Option<&str>) {
        self.shared.auth_url(url, instructions);
    }

    /// Ask for one pasted value. `None` means the user cancelled.
    pub async fn prompt(&self, prompt: &str, input: PromptInput) -> Option<String> {
        self.shared.prompt(prompt, input).await
    }

    #[must_use]
    pub fn cancelled(&self) -> bool {
        self.shared.cancelled()
    }

    /// Resolves once the user cancels the flow.
    pub async fn cancelled_wait(&self) {
        self.shared.cancelled_wait().await;
    }
}

/// How a prompt's input field behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptInput {
    /// Mask the typed value (tokens and keys).
    pub masked: bool,
    /// Accept an empty submit.
    pub allow_empty: bool,
}
