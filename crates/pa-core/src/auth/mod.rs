//! Auth subsystem: credential storage, resolution priority, stale-marking.

pub(crate) mod manager;
pub(crate) mod provider_oauth;
pub(crate) mod resolve_config_value;
pub(crate) mod storage;
pub(crate) mod types;

pub use manager::{AuthApiKeyResult, AuthStorage, NoOAuth, OAuthIntegration, OAuthRefreshFailure};
pub use provider_oauth::{
    ProviderOAuth, ANTHROPIC_PROVIDER_ID, GITHUB_COPILOT_PROVIDER_ID, OPENAI_CODEX_PROVIDER_ID,
    XAI_PROVIDER_ID,
};
pub use storage::{
    parse_storage_data, AuthStorageBackend, FileAuthStorageBackend, InMemoryAuthStorageBackend,
    RefreshLock,
};
pub use types::{
    AuthCredential, AuthSource, AuthSourceToken, AuthStatus, AuthStorageData, ECO_CREDENTIAL_ID,
    ECO_CREDENTIAL_NAME, SERPER_CREDENTIAL_ID, SERPER_CREDENTIAL_NAME,
};
