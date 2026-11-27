//! The model and MCP service catalogs compiled into the binary. Edit the
//! JSON files under `crates/pa-models/catalog/` to change them.

use crate::pinning::{pin_catalog_models, PinnedTemplates};
use crate::schema::{parse_model_catalog, InvalidEntries};
use crate::Model;

/// The model catalog document (`schemaVersion` 1).
pub const MODEL_CATALOG: &str = include_str!("../catalog/models.json");

/// The MCP service catalog document (parsed by pa-core's MCP catalog).
pub const MCP_SERVICE_CATALOG: &str = include_str!("../catalog/mcp-services.json");

/// Load + pin a catalog document. `None` (damaged document) falls back to
/// the compiled model definitions.
#[must_use]
pub fn load_catalog_models(asset: &str, templates: &PinnedTemplates) -> Option<Vec<Model>> {
    let payload: serde_json::Value = serde_json::from_str(asset).ok()?;
    let catalog = parse_model_catalog(&payload, InvalidEntries::Reject).ok()?;
    pin_catalog_models(catalog.models, templates).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transports;
    use serde_json::json;

    fn asset(models: impl AsRef<[serde_json::Value]>) -> String {
        serde_json::to_string_pretty(&json!({"schemaVersion": 1, "models": models.as_ref()}))
            .unwrap()
    }

    fn entry(id: &str, provider: &str) -> serde_json::Value {
        let compiled = transports::compiled_models();
        let template = compiled
            .iter()
            .find(|m| m.provider == provider)
            .expect("compiled provider");
        json!({
            "id": id,
            "name": id,
            "api": template.api,
            "provider": template.provider,
            "baseUrl": template.base_url,
            "reasoning": false,
            "input": ["text"],
            "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
            "contextWindow": 128_000,
            "maxTokens": 4_096,
        })
    }

    #[test]
    fn loads_and_pins_the_listed_entries() {
        let asset = asset(vec![entry("m1", "anthropic"), entry("m2", "openai")]);
        let models =
            load_catalog_models(&asset, &PinnedTemplates::from_compiled()).expect("loaded");
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["m1", "m2"]);
    }

    #[test]
    fn damaged_asset_returns_none_for_the_compiled_fallback() {
        assert!(load_catalog_models("{\"not json", &PinnedTemplates::from_compiled()).is_none());
        let damaged = asset(vec![json!({"id": "broken"})]);
        assert!(load_catalog_models(&damaged, &PinnedTemplates::from_compiled()).is_none());
    }
}
