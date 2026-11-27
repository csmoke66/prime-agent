//! Export palette resolution: the bundled Beehive palette
//! ([`bh_types::themes`]) to the CSS custom properties the export template
//! consumes. Pure functions only; terminal-side rendering is bh-tui's.

use std::collections::{BTreeMap, HashSet};

use anyhow::{anyhow, Result};
use serde_json::{Map, Value};

/// The CSS variables plus the three background colors the export template
/// substitutes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExportTheme {
    /// The `:root` custom-property block (`--key: value;` per line).
    pub(crate) theme_vars: String,
    pub(crate) body_bg: String,
    pub(crate) container_bg: String,
    pub(crate) info_bg: String,
}

const DEFAULT_DARK_TEXT: &str = "#e5e5e7";

struct ExportThemeJson {
    vars: Map<String, Value>,
    colors: Map<String, Value>,
    export_page_bg: Value,
    export_card_bg: Value,
    export_info_bg: Value,
}

/// The refinement row colors the palette carries implicitly (TS `refinementColors`).
const REFINEMENT_COLORS: [(&str, &str); 2] = [
    ("refinementHeader", "#9575cd"),
    ("refinementSummary", "#b7a1d6"),
];

/// A color value after variable-reference resolution: a CSS color string
/// (possibly empty, meaning the terminal default) or an ANSI-256 index.
#[derive(Debug, Clone, PartialEq)]
enum ResolvedColor {
    Ansi256(u64),
    Css(String),
}

/// Resolve one color value against the theme variables: numbers and hex colors pass through;
/// anything else is a variable reference, resolved transitively with cycle detection.
fn resolve_var_refs(value: &Value, vars: &Map<String, Value>) -> Result<ResolvedColor> {
    match value {
        Value::Number(index) => Ok(ResolvedColor::Ansi256(as_index(index)?)),
        Value::String(color) => {
            if color.is_empty() || color.starts_with('#') {
                Ok(ResolvedColor::Css(color.clone()))
            } else {
                let referenced = vars
                    .get(color)
                    .ok_or_else(|| anyhow!("Variable reference not found: {color}"))?;
                let mut visited = HashSet::from([color.as_str()]);
                resolve_var_ref_chain(referenced, vars, &mut visited)
            }
        }
        other => Err(anyhow!("Invalid color value: {other}")),
    }
}

fn resolve_var_ref_chain<'a>(
    value: &'a Value,
    vars: &'a Map<String, Value>,
    visited: &mut HashSet<&'a str>,
) -> Result<ResolvedColor> {
    match value {
        Value::Number(index) => Ok(ResolvedColor::Ansi256(as_index(index)?)),
        Value::String(color) => {
            if color.is_empty() || color.starts_with('#') {
                return Ok(ResolvedColor::Css(color.clone()));
            }
            if visited.contains(color.as_str()) {
                return Err(anyhow!("Circular variable reference detected: {color}"));
            }
            let referenced = vars
                .get(color)
                .ok_or_else(|| anyhow!("Variable reference not found: {color}"))?;
            visited.insert(color.as_str());
            resolve_var_ref_chain(referenced, vars, visited)
        }
        other => Err(anyhow!("Invalid color value: {other}")),
    }
}

fn as_index(value: &serde_json::Number) -> Result<u64> {
    value
        .as_u64()
        .ok_or_else(|| anyhow!("Invalid color value: {value}"))
}

/// Convert an ANSI-256 index to a hex color: the basic 16 (approximate
/// terminal values), the 6x6x6 cube, and the grayscale ramp.
fn ansi256_to_hex(index: u64) -> String {
    const BASIC_COLORS: [&str; 16] = [
        "#000000", "#800000", "#008000", "#808000", "#000080", "#800080", "#008080", "#c0c0c0",
        "#808080", "#ff0000", "#00ff00", "#ffff00", "#0000ff", "#ff00ff", "#00ffff", "#ffffff",
    ];
    if index < 16 {
        return BASIC_COLORS[index as usize].to_string();
    }
    if index < 232 {
        let cube_index = index - 16;
        let channel = |n: u64| if n == 0 { 0 } else { 55 + n * 40 };
        let (r, g, b) = (
            channel(cube_index / 36),
            channel((cube_index % 36) / 6),
            channel(cube_index % 6),
        );
        return format!("#{r:02x}{g:02x}{b:02x}");
    }
    let gray = (8 + (index - 232) * 10).min(255) as u8;
    format!("#{gray:02x}{gray:02x}{gray:02x}")
}

/// A resolved color as a CSS color string: ANSI-256 indices become hex, and empty
/// values (the terminal default) become the default text color.
fn css_color(color: ResolvedColor) -> String {
    match color {
        ResolvedColor::Ansi256(index) => ansi256_to_hex(index),
        ResolvedColor::Css(value) if value.is_empty() => DEFAULT_DARK_TEXT.to_string(),
        ResolvedColor::Css(value) => value,
    }
}

fn parse_theme_json(raw: &str) -> Result<ExportThemeJson> {
    let value: Value = serde_json::from_str(raw)?;
    let object = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| anyhow!("missing \"{key}\""))
    };
    let export = object("export")?;
    let export_color = |key: &str| {
        export
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow!("missing export \"{key}\""))
    };
    Ok(ExportThemeJson {
        vars: object("vars")?,
        colors: object("colors")?,
        export_page_bg: export_color("pageBg")?,
        export_card_bg: export_color("cardBg")?,
        export_info_bg: export_color("infoBg")?,
    })
}

/// Resolve the export palette: every palette color plus the refinement colors as `--key: value;`
/// lines, then the export backgrounds.
pub(crate) fn resolve_export_theme() -> Result<ExportTheme> {
    let theme = parse_theme_json(bh_types::themes::BEEHIVE_THEME_JSON)?;

    // Refinement colors are the implicit base the file's colors override.
    let mut css_colors: BTreeMap<String, String> = REFINEMENT_COLORS
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    for (key, value) in &theme.colors {
        css_colors.insert(
            key.clone(),
            css_color(resolve_var_refs(value, &theme.vars)?),
        );
    }

    let export_bg =
        |value: &Value| -> Result<String> { Ok(css_color(resolve_var_refs(value, &theme.vars)?)) };
    let body_bg = export_bg(&theme.export_page_bg)?;
    let container_bg = export_bg(&theme.export_card_bg)?;
    let info_bg = export_bg(&theme.export_info_bg)?;

    let mut lines: Vec<String> = css_colors
        .iter()
        .map(|(key, value)| format!("--{key}: {value};"))
        .collect();
    lines.push(format!("--exportPageBg: {body_bg};"));
    lines.push(format!("--exportCardBg: {container_bg};"));
    lines.push(format!("--exportInfoBg: {info_bg};"));

    Ok(ExportTheme {
        theme_vars: lines.join("\n      "),
        body_bg,
        container_bg,
        info_bg,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beehive_palette_resolves_like_the_ts_exporter() {
        let theme = resolve_export_theme().expect("resolve");
        assert!(theme.theme_vars.contains("--accent: #7c6faf;"));
        assert!(theme.theme_vars.contains("--exportPageBg: #050506;"));
        assert_eq!(theme.body_bg, "#050506");
        assert_eq!(theme.container_bg, "#0d0d10");
        assert_eq!(theme.info_bg, "#151518");
    }

    #[test]
    fn ansi256_colors() {
        assert_eq!(ansi256_to_hex(196), "#ff0000");
        assert_eq!(ansi256_to_hex(203), "#ff5f5f");
        assert_eq!(ansi256_to_hex(244), "#808080");
        assert_eq!(ansi256_to_hex(9), "#ff0000");
    }

    #[test]
    fn var_reference_errors() {
        let circular: Map<String, Value> =
            serde_json::from_str(r#"{ "a": "b", "b": "a" }"#).expect("vars");
        let error = resolve_var_refs(&Value::String("a".into()), &circular)
            .expect_err("circular reference must fail");
        assert!(
            error.to_string().contains("Circular variable reference"),
            "unexpected: {error}"
        );
        let error = resolve_var_refs(&Value::String("nope".into()), &Map::new())
            .expect_err("missing reference must fail");
        assert!(
            error.to_string().contains("Variable reference not found"),
            "unexpected: {error}"
        );
    }
}
