//! Capabilities advertised for the signed-in ChatGPT identity, not the bundled model list.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, ensure, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use super::request::build_sse_headers;
use crate::utils_inner::http::{send, RequestOptions};

const MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models?client_version=0.170.0";

pub(super) struct CatalogModel {
    pub slug: String,
    pub searchable: bool,
    pub priority: u64,
    pub daybreak_blue: bool,
}

fn parse_models(body: &[u8]) -> Result<Vec<CatalogModel>> {
    let mut catalog: Value = serde_json::from_slice(body)?;
    let rows = catalog
        .get_mut("models")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("invalid Codex model catalog"))?;
    Ok(rows
        .iter_mut()
        .filter_map(|row| {
            let Value::String(slug) = row.get_mut("slug")?.take() else {
                return None;
            };
            if slug.is_empty() {
                return None;
            }
            Some(CatalogModel {
                slug,
                searchable: row["visibility"] == "list" && row["supports_search_tool"] == true,
                priority: row["priority"].as_u64().unwrap_or(u64::MAX),
                daybreak_blue: row["available_access_programs"]["cyber"]
                    .as_array()
                    .is_some_and(|programs| programs.iter().any(|p| p == "daybreak_blue")),
            })
        })
        .collect())
}

async fn fetch(account: &str, token: &str) -> Result<Vec<CatalogModel>> {
    let mut req = RequestOptions::new(reqwest::Method::GET, MODELS_URL.to_owned());
    req.headers = build_sse_headers(None, None, account, token, None);
    for (name, value) in &mut req.headers {
        if name == "accept" {
            "application/json".clone_into(value);
        }
    }
    let mut res = send(req)
        .await
        .map_err(|_| anyhow!("Could not reach the ChatGPT model catalog"))?;
    ensure!(
        (200..300).contains(&res.status),
        "ChatGPT model catalog returned HTTP {}",
        res.status
    );
    let mut body = Vec::new();
    while let Some(chunk) = res
        .next_bytes()
        .await
        .map_err(|_| anyhow!("Interrupted ChatGPT model catalog"))?
    {
        ensure!(
            body.len() + chunk.len() <= 2 * 1024 * 1024,
            "ChatGPT model catalog too large"
        );
        body.extend_from_slice(&chunk);
    }
    parse_models(&body)
}

type Lookup = Result<Arc<[CatalogModel]>, String>;
struct Cached {
    identity: [u8; 32],
    expires: Instant,
    models: Lookup,
}

pub(super) async fn models(account: &str, token: &str) -> Lookup {
    // Workers usually serve one login. Keep one bounded entry, never the bearer
    // token; even two members of the same workspace can have different access.
    static CACHE: Mutex<Option<Cached>> = Mutex::const_new(None);
    let identity: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let mut cache = CACHE.lock().await;
    if let Some(hit) = cache
        .as_ref()
        .filter(|c| c.identity == identity && c.expires > Instant::now())
    {
        return hit.models.clone();
    }
    let models: Lookup =
        match tokio::time::timeout(Duration::from_secs(4), fetch(account, token)).await {
            Ok(result) => result.map(Arc::from).map_err(|e| e.to_string()),
            Err(_) => Err("ChatGPT model catalog timed out".into()),
        };
    let ttl = if models.is_ok() { 300 } else { 30 };
    *cache = Some(Cached {
        identity,
        expires: Instant::now() + Duration::from_secs(ttl),
        models: models.clone(),
    });
    models
}

pub(super) fn apply_daybreak(body: &mut Value, enabled: bool) {
    if enabled {
        body["access_programs"] = serde_json::json!({"cyber": "daybreak_blue"});
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daybreak_is_selected_only_for_an_explicit_model_entitlement() {
        let catalog = parse_models(br#"{"models":[{"slug":"one","available_access_programs":{"cyber":["standard","daybreak_blue"]}},{"slug":"two","available_access_programs":{"cyber":["standard"]}},{"slug":"three"}]}"#).unwrap();
        for m in catalog {
            let mut body = serde_json::json!({"model": m.slug});
            apply_daybreak(&mut body, m.daybreak_blue);
            if m.slug == "one" {
                assert_eq!(body["access_programs"]["cyber"], "daybreak_blue");
            } else {
                assert!(body.get("access_programs").is_none());
            }
        }
    }
}
