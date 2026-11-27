//! The bundled Beehive palette (pure data, the product theme file verbatim), shared by the TUI's
//! palette loader (bh-tui) and the session HTML exporter (bh-core); its `export` section
//! carries the export page's background colors.

/// The Beehive palette file: the only palette.
pub const BEEHIVE_THEME_JSON: &str = include_str!("../themes/beehive.json");
