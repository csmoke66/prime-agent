//! Isolated live search through the ChatGPT Codex subscription backend.

use std::fmt::Write as _;

use anyhow::{anyhow, bail, ensure, Result};
use serde_json::{json, Value};

use super::request::{
    build_sse_headers, extract_account_id, resolve_codex_url, DEFAULT_CODEX_BASE_URL,
};
use crate::utils_inner::http::{send, HttpResponse, RequestOptions};
use crate::utils_inner::sse::{ServerSentEvent, SseDecoder};

const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// Search the live web using a ChatGPT subscription access token. No session
/// state, caller tools, custom endpoint, or API-key fallback is used. The caller
/// must impose a deadline covering the catalog lookup and the response stream.
///
/// # Errors
/// Returns a sanitized error if the backend request fails or produces no grounded results.
pub async fn search_chatgpt_web(token: &str, query: &str, num_results: u64) -> Result<String> {
    let account = extract_account_id(token)
        .map_err(|_| anyhow!("ChatGPT web search requires a ChatGPT login"))?;
    let catalog = super::catalog::models(&account, token)
        .await
        .map_err(anyhow::Error::msg)?;
    let model = catalog
        .iter()
        .filter(|model| model.searchable)
        .min_by_key(|model| (model.slug != "gpt-6-luna", model.priority))
        .ok_or_else(|| anyhow!("No ChatGPT web search model is available for this login"))?;

    let mut options = RequestOptions::new(
        reqwest::Method::POST,
        resolve_codex_url(DEFAULT_CODEX_BASE_URL),
    );
    options.headers = build_sse_headers(None, None, &account, token, None);
    let mut body = search_body(&model.slug, query, num_results);
    super::catalog::apply_daybreak(&mut body, model.daybreak_blue);
    options.body = Some(body.to_string());
    let mut response = send_request(options).await?;
    let mut decoder = SseDecoder::new();
    let mut search = SearchResponse::default();
    let mut received = 0;
    loop {
        let (events, done) = if let Some(chunk) = next_chunk(&mut response).await? {
            received += chunk.len();
            ensure_size(received)?;
            (decoder.push_text(&chunk), false)
        } else {
            (decoder.finish(), true)
        };
        for event in events {
            if let Some(text) = search.handle(event, num_results)? {
                // Do not expose credentials even if unexpectedly reflected in output.
                return Ok(if text.contains(token) {
                    text.replace(token, "[redacted]")
                } else {
                    text
                });
            }
        }
        if done {
            bail!("ChatGPT web search ended before completing");
        }
    }
}

async fn send_request(options: RequestOptions) -> Result<HttpResponse> {
    // Never pass through an upstream body, URL, or transport error containing credentials.
    let response = send(options)
        .await
        .map_err(|_| anyhow!("Could not connect to ChatGPT web search"))?;
    if response.status == 401 {
        bail!("ChatGPT web search requires a ChatGPT login");
    }
    ensure!(
        (200..300).contains(&response.status),
        "ChatGPT web search request failed (HTTP {})",
        response.status
    );
    Ok(response)
}

async fn next_chunk(response: &mut HttpResponse) -> Result<Option<String>> {
    response
        .next_text()
        .await
        .map_err(|_| anyhow!("ChatGPT web search response was interrupted"))
}

fn ensure_size(len: usize) -> Result<()> {
    ensure!(
        len <= MAX_RESPONSE_BYTES,
        "ChatGPT web search response was too large"
    );
    Ok(())
}

fn search_body(model: &str, query: &str, num_results: u64) -> Value {
    json!({
        "model": model,
        "store": false,
        "stream": true,
        "instructions": format!(
            "Use live web search for the user's query. Return up to {num_results} relevant results. \
             Give a short description for each and cite it with a native URL citation. \
             Do not invent sources or write bare URLs."
        ),
        "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": query}]}],
        "tools": [{"type": "web_search", "external_web_access": true}],
        "tool_choice": "required",
        "parallel_tool_calls": false,
        "include": ["web_search_call.action.sources"],
    })
}

#[derive(Default)]
struct SearchResponse {
    done_items: Vec<Value>,
}

impl SearchResponse {
    fn handle(&mut self, event: ServerSentEvent, num_results: u64) -> Result<Option<String>> {
        let data = event.data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(None);
        }
        let mut event: Value = serde_json::from_str(data)
            .map_err(|_| anyhow!("ChatGPT web search returned an invalid stream event"))?;
        match event["type"].as_str() {
            Some("response.output_item.done") => self.done_items.push(event["item"].take()),
            Some("response.completed" | "response.done") => {
                let response = &event["response"];
                ensure!(
                    response["status"].is_null() || response["status"] == "completed",
                    "ChatGPT web search did not complete"
                );
                // The terminal event ordinarily contains the entire output. Some
                // Codex streams only include it in completed item events.
                let items = response["output"]
                    .as_array()
                    .filter(|items| !items.is_empty())
                    .unwrap_or(&self.done_items);
                return render(items, num_results).map(Some);
            }
            Some("error" | "response.failed" | "response.incomplete" | "response.cancelled") => {
                bail!("ChatGPT web search did not complete");
            }
            _ => {}
        }
        Ok(None)
    }
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn render(items: &[Value], num_results: u64) -> Result<String> {
    let searches = items.iter().filter(|item| {
        item["type"] == "web_search_call"
            && item["status"] == "completed"
            && (item["action"].is_null() || item["action"]["type"] == "search")
    });
    ensure!(
        searches.clone().next().is_some(),
        "ChatGPT did not perform a live web search"
    );

    let mut text = String::new();
    let mut sources = Vec::new();
    let limit = usize::try_from(num_results).unwrap_or(usize::MAX);
    for item in items
        .iter()
        .filter(|item| item["type"] == "message" && item["role"] == "assistant")
    {
        for part in array(&item["content"]) {
            if part["type"] != "output_text" {
                continue;
            }
            let mut cited = false;
            for annotation in array(&part["annotations"]) {
                if annotation["type"] == "url_citation" {
                    cited |= add_source(&mut sources, annotation, limit);
                }
            }
            // Ignore free-form model text which did not come with actual citations.
            if cited {
                if let Some(body) = part["text"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    if !text.is_empty() {
                        text.push_str("\n\n");
                    }
                    text.push_str(body);
                }
            }
        }
    }
    // When the answer cites fewer sources than requested, fill from the tool's
    // actual search results, not additional URLs invented by the model.
    for call in searches {
        for source in array(&call["action"]["sources"]) {
            add_source(&mut sources, source, limit);
        }
    }
    ensure!(
        !sources.is_empty(),
        "ChatGPT web search returned no source links"
    );
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str("Sources:");
    for (url, title) in sources {
        if let Some(title) = title {
            let _ = write!(text, "\n- {}: {url}", title.replace(['\r', '\n'], " "));
        } else {
            let _ = write!(text, "\n- {url}");
        }
    }
    Ok(text)
}

fn add_source<'a>(
    sources: &mut Vec<(&'a str, Option<&'a str>)>,
    value: &'a Value,
    limit: usize,
) -> bool {
    let Some(url) = value["url"].as_str() else {
        return false;
    };
    if sources.iter().any(|&(known, _)| known == url) {
        return true;
    }
    if sources.len() == limit || url.chars().any(char::is_whitespace) {
        return false;
    }
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return false;
    }
    sources.push((
        url,
        value["title"]
            .as_str()
            .filter(|title| !title.trim().is_empty()),
    ));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_requires_live_hosted_tool_and_only_sends_the_query() {
        let body = search_body("gpt-6-luna", "example query", 3);
        assert_eq!(
            body["tools"],
            json!([{"type":"web_search","external_web_access":true}])
        );
        assert_eq!(body["tool_choice"], "required");
        assert_eq!(body["store"], false);
        assert_eq!(body["input"].as_array().unwrap().len(), 1);
        assert_eq!(body["input"][0]["content"][0]["text"], "example query");
        assert!(body.get("previous_response_id").is_none());
    }

    #[test]
    fn renders_only_completed_search_with_real_source_links() {
        let search = json!({"type":"web_search_call","status":"completed","action":{"type":"search","sources":[
            {"type":"url","url":"https://example.org/second"}
        ]}});
        let mut message = json!({"type":"message","role":"assistant","content":[
            {"type":"output_text","text":"Sourced result.","annotations":[
                {"type":"url_citation","title":"Example","url":"https://example.org/first"}
            ]},
            {"type":"output_text","text":"Uncited result.","annotations":[]}
        ]});
        assert!(render(std::slice::from_ref(&message), 2).is_err());
        let terminal = json!({"type":"response.completed","response":{"status":"completed","output":[search.clone(),message.clone()]}});
        let mut decoder = SseDecoder::new();
        let wire = format!("data: {terminal}\n\n");
        let (head, tail) = wire.split_at(42);
        assert!(decoder.push_text(head).is_empty());
        let result = SearchResponse::default()
            .handle(decoder.push_text(tail).pop().unwrap(), 2)
            .unwrap()
            .unwrap();
        assert_eq!(result, "Sourced result.\n\nSources:\n- Example: https://example.org/first\n- https://example.org/second");
        message["content"][0]["annotations"] = json!([]);
        assert_eq!(
            render(&[search, message], 1).unwrap(),
            "Sources:\n- https://example.org/second"
        );
    }
}
