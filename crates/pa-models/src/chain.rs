//! The process model catalog: the compiled-in catalog document, pinned to
//! compiled transports, with the compiled transport list as the fallback
//! when the document cannot be read.

use crate::catalog::{load_catalog_models, MODEL_CATALOG};
use crate::pinning::PinnedTemplates;
use crate::transports;
use crate::Model;

/// The resolved model catalog.
pub struct ModelCatalog {
    models: Vec<Model>,
}

impl Default for ModelCatalog {
    fn default() -> Self {
        Self::from_document(MODEL_CATALOG)
    }
}

impl ModelCatalog {
    /// A catalog over `document` (a `schemaVersion` 1 catalog). A document
    /// that does not parse falls back to the compiled transport list.
    #[must_use]
    pub fn from_document(document: &str) -> Self {
        Self {
            models: load_catalog_models(document, &PinnedTemplates::from_compiled())
                .unwrap_or_else(|| transports::compiled_models().to_vec()),
        }
    }

    /// The catalog's models.
    #[must_use]
    pub fn models(&self) -> &[Model] {
        &self.models
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unreadable_document_falls_back_to_the_compiled_models() {
        assert_eq!(
            ModelCatalog::from_document("{broken").models(),
            transports::compiled_models()
        );
    }

    #[test]
    fn the_compiled_in_catalog_lists_anthropic_models() {
        assert!(ModelCatalog::default()
            .models()
            .iter()
            .any(|model| model.provider == "anthropic"));
    }
}
