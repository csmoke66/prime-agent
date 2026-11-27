//! The process-shared model catalog: the compiled-in catalog, parsed once per process.

use std::sync::OnceLock;

use pa_models::ModelCatalog;

/// The process-shared model catalog every registry sources its built-ins from.
pub fn catalog() -> &'static ModelCatalog {
    static CATALOG: OnceLock<ModelCatalog> = OnceLock::new();
    CATALOG.get_or_init(ModelCatalog::default)
}
