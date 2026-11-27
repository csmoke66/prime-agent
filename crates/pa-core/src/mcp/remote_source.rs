//! The MCP service catalog compiled into the binary, the remote-catalog source for resolution.

use super::catalog_schema::{parse_plugins_catalog, PluginsCatalog};

/// The compiled-in service catalog; `None` when the document does not parse.
pub fn compiled_plugins_catalog() -> Option<PluginsCatalog> {
    parse_plugins_catalog(pa_models::catalog::MCP_SERVICE_CATALOG.as_bytes()).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_compiled_in_service_catalog_parses() {
        assert!(super::compiled_plugins_catalog().is_some());
    }
}
