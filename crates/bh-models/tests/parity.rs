//! The compiled-in model catalog (`crates/bh-models/catalog/models.json`)
//! parses whole against the strict schema, and every entry pins to a
//! compiled transport.

use bh_models::catalog::MODEL_CATALOG;
use bh_models::pinning::{parse_provider_model_catalog, PinnedTemplates};
use bh_models::schema::{parse_model_catalog, InvalidEntries};
use serde_json::Value;

fn catalog() -> Value {
    serde_json::from_str(MODEL_CATALOG).expect("the catalog is JSON")
}

#[test]
fn every_entry_parses_and_pins_to_a_compiled_transport() {
    let parsed = parse_model_catalog(&catalog(), InvalidEntries::Reject).expect("strict parse");
    assert!(parsed.models.iter().all(|model| model.headers.is_none()));
    let pinned = parse_provider_model_catalog(&catalog(), &PinnedTemplates::from_compiled())
        .expect("pinned");
    assert_eq!(pinned.len(), parsed.models.len(), "no entry is dropped");
}
