//! The compiled transport table.
//! `bh_ai::models_generated` is the hand-maintained table of transports
//! the client implements; it only changes with a client release — the
//! catalog can select among these but never introduce one.

use std::collections::HashSet;
use std::sync::LazyLock;

use crate::Model;

/// The compiled catalog, shared as one immutable list (TS `installedModels`).
#[must_use]
pub fn compiled_models() -> &'static [Model] {
    static COMPILED: LazyLock<Vec<Model>> = LazyLock::new(|| {
        let mut models: Vec<Model> = bh_ai::models_generated::get_providers()
            .into_iter()
            .flat_map(bh_ai::models_generated::get_models)
            .cloned()
            .collect();
        models.shrink_to_fit();
        models
    });
    &COMPILED
}

/// Distinct compiled `(provider, api, baseUrl)` tuples.
#[must_use]
pub fn compiled_transport_tuples() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut tuples: Vec<(&'static str, &'static str, &'static str)> = Vec::new();
    let mut seen: HashSet<(&str, &str, &str)> = HashSet::new();
    for model in compiled_models() {
        if seen.insert((
            model.provider.as_str(),
            model.api.as_str(),
            model.base_url.as_str(),
        )) {
            tuples.push((
                model.provider.as_str(),
                model.api.as_str(),
                model.base_url.as_str(),
            ));
        }
    }
    tuples
}
