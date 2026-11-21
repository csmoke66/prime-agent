//! Isolated live search through Anthropic Messages with a Claude login.

use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{anyhow, bail, ensure, Result};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{json, Value};

use super::{is_oauth_token, ANTHROPIC_VERSION, OAUTH_BETA};

const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const SEARCH_MODEL: &str = "claude-haiku-4-5-20251001";
const MAX_QUERY_BYTES: usize = 8 * 1024;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_RESULTS: usize = 20;
const MAX_URL_BYTES: usize = 2048;
const MAX_TITLE_CHARS: usize = 256;

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    crate::utils_inner::tls::http_client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(55))
        .build()
        .expect("Anthropic web search HTTP client")
});

/// Search the live web with a Claude subscription OAuth token. The request
/// contains only the query and the hosted search tool, not session history.
///
/// # Errors
/// Returns a sanitized error when the request fails or produces no grounded sources.
pub async fn search_claude_web(token: &str, query: &str, num_results: u64) -> Result<String> {
    ensure!(
        is_oauth_token(token),
        "Claude web search requires a Claude login"
    );
    let query = query.trim();
    ensure!(
        !query.is_empty(),
        "Claude web search requires a nonempty query"
    );
    ensure!(
        query.len() <= MAX_QUERY_BYTES,
        "Claude web search query was too long"
    );
    let limit = num_results.min(MAX_RESULTS as u64) as usize;
    ensure!(
        limit > 0,
        "Claude web search numResults must be a positive integer"
    );

    let mut response = search_request(token, query, limit)
        .send()
        .await
        .map_err(|error| transport_error(&error, "Could not connect to Claude web search"))?;
    check_status(response.status())?;

    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| transport_error(&error, "Claude web search response was interrupted"))?
    {
        ensure!(
            chunk.len() <= MAX_RESPONSE_BYTES - body.len(),
            "Claude web search response was too large"
        );
        body.extend_from_slice(&chunk);
    }
    let message: Value = serde_json::from_slice(&body)
        .map_err(|_| anyhow!("Claude web search returned an invalid response"))?;
    render(&message, token, limit)
}

fn transport_error(error: &reqwest::Error, otherwise: &'static str) -> anyhow::Error {
    if error.is_timeout() {
        anyhow!("Claude web search timed out")
    } else {
        anyhow!(otherwise)
    }
}

fn check_status(status: StatusCode) -> Result<()> {
    if status == StatusCode::UNAUTHORIZED {
        bail!("Claude web search requires a Claude login");
    }
    ensure!(
        status.is_success(),
        "Claude web search request failed (HTTP {})",
        status.as_u16()
    );
    Ok(())
}

fn search_request(token: &str, query: &str, limit: usize) -> RequestBuilder {
    CLIENT
        .post(MESSAGES_URL)
        .bearer_auth(token)
        .header("anthropic-beta", OAUTH_BETA)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&json!({
            "model": SEARCH_MODEL,
            "max_tokens": 1024,
            "system": format!("Use live web search for the user's query. Cite up to {limit} relevant sources."),
            "messages": [{"role": "user", "content": query}],
            "tools": [{"type": "web_search_20250305", "name": "web_search", "max_uses": 1}],
        }))
}

fn render(message: &Value, token: &str, limit: usize) -> Result<String> {
    ensure!(
        message["type"] == "message"
            && message["role"] == "assistant"
            && message["stop_reason"] == "end_turn",
        "Claude web search did not complete"
    );
    let blocks = message["content"]
        .as_array()
        .ok_or_else(|| anyhow!("Claude web search returned an invalid response"))?;
    let mut results = Vec::new();
    for block in blocks
        .iter()
        .filter(|block| block["type"] == "web_search_tool_result")
    {
        // A failed hosted search is encoded as an object here even with HTTP 200.
        let hits = block["content"]
            .as_array()
            .ok_or_else(|| anyhow!("Claude's hosted web search failed"))?;
        for hit in hits.iter().filter(|hit| hit["type"] == "web_search_result") {
            let Some(url) = hit["url"].as_str().filter(|url| valid_url(url, token)) else {
                continue;
            };
            results.push((url, hit["title"].as_str()));
        }
    }
    ensure!(
        !results.is_empty(),
        "Claude web search returned no source links"
    );

    // Prefer the results Anthropic natively cited, then fill in provider result
    // order. Do not render the model's freeform text or URLs from that text.
    let cited = blocks
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["citations"].as_array())
        .flatten()
        .filter(|citation| citation["type"] == "web_search_result_location")
        .filter_map(|citation| citation["url"].as_str())
        .filter_map(|url| results.iter().find(|(known, _)| *known == url));
    let mut text = String::from("Sources:");
    let mut seen = Vec::new();
    for &(url, title) in cited.chain(&results) {
        if seen.contains(&url) {
            continue;
        }
        seen.push(url);
        text.push_str("\n- ");
        if let Some(title) = title
            .map(str::trim)
            .filter(|title| !title.is_empty() && !title.contains(token))
        {
            text.extend(title.chars().take(MAX_TITLE_CHARS).map(|c| {
                if c.is_control() || c.is_whitespace() {
                    ' '
                } else {
                    c
                }
            }));
            text.push_str(": ");
        }
        text.push_str(url);
        if seen.len() == limit {
            break;
        }
    }
    Ok(text)
}

fn valid_url(url: &str, token: &str) -> bool {
    url.len() <= MAX_URL_BYTES
        && !url.contains(token)
        && !url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\')
        && (url.starts_with("https://") || url.starts_with("http://"))
        && url::Url::parse(url).is_ok_and(|url| {
            url.has_host() && url.username().is_empty() && url.password().is_none()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "sk-ant-oat01-test-token";

    fn message(content: Value) -> Value {
        json!({"type": "message", "role": "assistant", "stop_reason": "end_turn", "content": content})
    }

    #[test]
    fn request_uses_only_oauth_and_the_hosted_tool() {
        let request = search_request(TOKEN, "example query", 3).build().unwrap();
        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.url().as_str(), MESSAGES_URL);
        assert_eq!(
            request.headers()["authorization"],
            format!("Bearer {TOKEN}").as_str()
        );
        assert_eq!(request.headers()["anthropic-beta"], "oauth-2025-04-20");
        assert_eq!(request.headers()["anthropic-version"], "2023-06-01");
        assert!(!request.headers().contains_key("x-api-key"));
        let body: Value = serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["model"], "claude-haiku-4-5-20251001");
        assert_eq!(
            body["messages"],
            json!([{"role": "user", "content": "example query"}])
        );
        assert_eq!(
            body["tools"],
            json!([{"type": "web_search_20250305", "name": "web_search", "max_uses": 1}])
        );
    }

    #[tokio::test]
    async fn rejects_api_keys_before_network() {
        let error = search_claude_web("sk-ant-api03-test-key", "docs", 1)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "Claude web search requires a Claude login");
    }

    #[test]
    fn renders_only_native_results_prioritizing_citations() {
        let response = message(json!([
            {"type": "server_tool_use", "name": "web_search", "id": "srvtoolu_1"},
            {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_1", "content": [
                {"type": "web_search_result", "title": "First", "url": "https://example.org/first"},
                {"type": "web_search_result", "title": "Second\nsource", "url": "https://example.org/second"},
                {"type": "web_search_result", "title": "duplicate", "url": "https://example.org/second"},
                {"type": "web_search_result", "url": "javascript:unsafe"},
                {"type": "web_search_result", "url": "https://login:secret@example.org"},
                {"type": "web_search_result", "url": format!("https://example.org/{TOKEN}")},
                {"type": "web_search_result", "url": format!("https://example.org/{}", "a".repeat(MAX_URL_BYTES))},
                {"type": "web_search_result", "title": TOKEN, "url": "https://example.org/third"}
            ]},
            {"type": "text", "text": "Do not render this. https://not-a-result.org", "citations": [
                {"type": "web_search_result_location", "url": "https://not-a-tool-result.org"},
                {"type": "web_search_result_location", "url": "https://example.org/second"}
            ]}
        ]));
        assert_eq!(
            render(&response, TOKEN, 1).unwrap(),
            "Sources:\n- Second source: https://example.org/second"
        );
        assert_eq!(
            render(&response, TOKEN, MAX_RESULTS).unwrap(),
            "Sources:\n- Second source: https://example.org/second\n- First: https://example.org/first\n- https://example.org/third"
        );
    }

    #[test]
    fn rejects_http_errors_and_empty_or_failed_searches() {
        assert_eq!(
            check_status(StatusCode::UNAUTHORIZED).unwrap_err().to_string(),
            "Claude web search requires a Claude login"
        );
        assert_eq!(
            check_status(StatusCode::TOO_MANY_REQUESTS).unwrap_err().to_string(),
            "Claude web search request failed (HTTP 429)"
        );
        assert_eq!(
            render(&message(json!([{"type": "web_search_tool_result", "content": {
                "type": "web_search_tool_result_error", "error_code": TOKEN
            }}])), TOKEN, 2).unwrap_err().to_string(),
            "Claude's hosted web search failed"
        );
        for blocks in [
            json!([{"type": "text", "citations": [{"type": "web_search_result_location", "url": "https://example.org"}]}]),
            json!([{"type": "web_search_tool_result", "content": []}]),
        ] {
            assert_eq!(
                render(&message(blocks), TOKEN, 2).unwrap_err().to_string(),
                "Claude web search returned no source links"
            );
        }
        let mut incomplete = message(json!([]));
        incomplete["stop_reason"] = json!("max_tokens");
        assert!(render(&incomplete, TOKEN, 2).is_err());
        assert!(render(&json!({"type": "error", "error": {"message": TOKEN}}), TOKEN, 2).is_err());
    }
}
