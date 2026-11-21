//! The kernel bridge for ChatGPT subscription-backed web search.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::auth::AuthStorage;
use crate::kernel::shared::{host_handler, HostRequestHandlers};

const PROVIDER: &str = "openai-codex";
const SEARCH_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn register(handlers: &mut HostRequestHandlers, agent_dir: PathBuf) {
    let agent_dir = Arc::new(agent_dir);
    handlers.register(
        "chatgpt_web_search",
        host_handler(move |payload| {
            let agent_dir = Arc::clone(&agent_dir);
            async move {
                let (query, num_results) = parse_request(&payload.data)?;
                tokio::time::timeout(SEARCH_TIMEOUT, async move {
                    let token = tokio::task::spawn_blocking(move || {
                        let mut auth = AuthStorage::create(agent_dir.as_path());
                        anyhow::ensure!(
                            auth.load_error().is_none(),
                            "Unable to read the ChatGPT login"
                        );
                        auth.get_stored_oauth_api_key(PROVIDER)
                            .map_err(|_| anyhow::anyhow!("Unable to refresh the ChatGPT login"))?
                            .filter(|token| !token.trim().is_empty())
                            .ok_or_else(|| {
                                anyhow::anyhow!("ChatGPT web search requires a ChatGPT login")
                            })
                    })
                    .await
                    .map_err(|_| anyhow::anyhow!("Unable to read the ChatGPT login"))??;
                    let text = pa_ai::search_chatgpt_web(&token, query, num_results).await?;
                    Ok(json!({"text": text}))
                })
                .await
                .map_err(|_| anyhow::anyhow!("ChatGPT web search timed out"))?
            }
        }),
    );
}

fn parse_request(data: &Value) -> anyhow::Result<(&str, u64)> {
    let query = data
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .ok_or_else(|| anyhow::anyhow!("ChatGPT web search requires a nonempty query"))?;
    let num_results = data
        .get("numResults")
        .and_then(Value::as_u64)
        .filter(|&count| count > 0)
        .ok_or_else(|| {
            anyhow::anyhow!("ChatGPT web search numResults must be a positive integer")
        })?;
    Ok((query, num_results))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_host_request() {
        let payload = json!({"type": "chatgpt_web_search", "query": " docs ", "numResults": 2});
        assert_eq!(parse_request(&payload).unwrap(), ("docs", 2));
        for count in [json!(0), json!(-1), json!(1.5), json!("2"), Value::Null] {
            assert!(parse_request(&json!({"query": "docs", "numResults": count})).is_err());
        }
        assert!(parse_request(&json!({"query": "  ", "numResults": 1})).is_err());
    }

    #[tokio::test]
    async fn does_not_use_a_stored_api_key_in_place_of_chatgpt_login() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("auth.json"),
            r#"{"openai-codex":{"type":"api_key","key":"not-an-oauth-token"}}"#,
        )
        .unwrap();
        let mut handlers = HostRequestHandlers::new();
        register(&mut handlers, dir.path().to_path_buf());
        let result = handlers.get("chatgpt_web_search").unwrap()(
            crate::kernel::shared::HostRequestPayload {
                data: json!({"type":"chatgpt_web_search", "query":"docs", "numResults":1}),
            },
        )
        .await;
        assert_eq!(
            result.unwrap_err().to_string(),
            "ChatGPT web search requires a ChatGPT login"
        );
    }
}
