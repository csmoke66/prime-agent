//! Kernel bridges for subscription-backed web search.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::auth::{AuthStorage, ANTHROPIC_PROVIDER_ID, OPENAI_CODEX_PROVIDER_ID};
use crate::kernel::shared::{host_handler, HostRequestHandlers};

#[derive(Clone, Copy)]
enum SearchProvider {
    ChatGpt,
    Claude,
}

impl SearchProvider {
    fn auth(self) -> (&'static str, &'static str) {
        match self {
            Self::ChatGpt => (OPENAI_CODEX_PROVIDER_ID, "ChatGPT"),
            Self::Claude => (ANTHROPIC_PROVIDER_ID, "Claude"),
        }
    }

    async fn search(self, token: &str, query: &str, num_results: u64) -> anyhow::Result<String> {
        match self {
            Self::ChatGpt => bh_ai::search_chatgpt_web(token, query, num_results).await,
            Self::Claude => bh_ai::search_claude_web(token, query, num_results).await,
        }
    }
}

const SEARCH_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn register(handlers: &mut HostRequestHandlers, agent_dir: PathBuf) {
    let agent_dir = Arc::new(agent_dir);
    for (request_type, provider) in [
        ("chatgpt_web_search", SearchProvider::ChatGpt),
        ("claude_web_search", SearchProvider::Claude),
    ] {
        let agent_dir = Arc::clone(&agent_dir);
        handlers.register(
            request_type,
            host_handler(move |payload| {
                let agent_dir = Arc::clone(&agent_dir);
                let (_, name) = provider.auth();
                async move {
                    let (query, num_results) = parse_request(&payload.data, name)?;
                    tokio::time::timeout(SEARCH_TIMEOUT, async move {
                        let token = tokio::task::spawn_blocking(move || {
                            load_token(agent_dir.as_path(), provider)
                        })
                        .await
                        .map_err(|_| anyhow::anyhow!("Unable to read the {name} login"))??;
                        let text = provider.search(&token, query, num_results).await?;
                        Ok(json!({"text": text}))
                    })
                    .await
                    .map_err(|_| anyhow::anyhow!("{name} web search timed out"))?
                }
            }),
        );
    }
}

fn load_token(agent_dir: &Path, provider: SearchProvider) -> anyhow::Result<String> {
    let (provider_id, name) = provider.auth();
    let mut auth = AuthStorage::create(agent_dir);
    anyhow::ensure!(auth.load_error().is_none(), "Unable to read the {name} login");
    auth.get_stored_oauth_api_key(provider_id)
        .map_err(|_| anyhow::anyhow!("Unable to refresh the {name} login"))?
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("{name} web search requires a {name} login"))
}

fn parse_request<'a>(data: &'a Value, name: &str) -> anyhow::Result<(&'a str, u64)> {
    let query = data
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{name} web search requires a nonempty query"))?;
    let num_results = data
        .get("numResults")
        .and_then(Value::as_u64)
        .filter(|&count| count > 0)
        .ok_or_else(|| {
            anyhow::anyhow!("{name} web search numResults must be a positive integer")
        })?;
    Ok((query, num_results))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_host_request() {
        let payload = json!({"type": "chatgpt_web_search", "query": " docs ", "numResults": 2});
        assert_eq!(parse_request(&payload, "ChatGPT").unwrap(), ("docs", 2));
        for count in [json!(0), json!(-1), json!(1.5), json!("2"), Value::Null] {
            assert!(parse_request(&json!({"query": "docs", "numResults": count}), "Claude").is_err());
        }
        assert!(parse_request(&json!({"query": "  ", "numResults": 1}), "Claude").is_err());
    }

    #[tokio::test]
    async fn neither_handler_substitutes_a_stored_api_key_for_a_login() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("auth.json"),
            r#"{
                "openai-codex":{"type":"api_key","key":"not-an-oauth-token"},
                "anthropic":{"type":"api_key","key":"not-an-oauth-token"}
            }"#,
        )
        .unwrap();
        let mut handlers = HostRequestHandlers::new();
        register(&mut handlers, dir.path().to_path_buf());
        for (request_type, name) in [
            ("chatgpt_web_search", "ChatGPT"),
            ("claude_web_search", "Claude"),
        ] {
            let result = handlers.get(request_type).unwrap()(
                crate::kernel::shared::HostRequestPayload {
                    data: json!({"type":request_type, "query":"docs", "numResults":1}),
                },
            )
            .await;
            assert_eq!(
                result.unwrap_err().to_string(),
                format!("{name} web search requires a {name} login"),
            );
        }
    }

    #[test]
    fn loads_only_the_selected_providers_login_and_rejects_failed_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let auth = dir.path().join("auth.json");
        std::fs::write(
            &auth,
            r#"{"anthropic":{"type":"oauth","access":"claude-access","expires":9999999999999}}"#,
        )
        .unwrap();
        assert_eq!(
            load_token(dir.path(), SearchProvider::Claude).unwrap(),
            "claude-access"
        );
        assert_eq!(
            load_token(dir.path(), SearchProvider::ChatGpt)
                .unwrap_err()
                .to_string(),
            "ChatGPT web search requires a ChatGPT login",
        );
        std::fs::write(
            &auth,
            r#"{"anthropic":{"type":"oauth","access":"claude-access","expires":1}}"#,
        )
        .unwrap();
        assert_eq!(
            load_token(dir.path(), SearchProvider::Claude)
                .unwrap_err()
                .to_string(),
            "Unable to refresh the Claude login",
        );
    }
}
