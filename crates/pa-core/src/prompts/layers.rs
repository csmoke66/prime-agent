//! Embedded JSON system-prompt layers. Segment boundaries do not alter whitespace.

use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AssetSegment {
    pub name: String,
    pub body: String,
    pub after: String,
}

pub(super) static CORE: LazyLock<Vec<AssetSegment>> =
    LazyLock::new(|| parse(include_str!("layers/core.json")));
pub(super) static USAGE: LazyLock<Vec<AssetSegment>> =
    LazyLock::new(|| parse(include_str!("layers/usage.json")));
pub(super) static OPINIONATED: LazyLock<Vec<AssetSegment>> =
    LazyLock::new(|| parse(include_str!("layers/opinionated.json")));
pub(super) static BASH_OVER_SUBPROCESS: LazyLock<Vec<AssetSegment>> =
    LazyLock::new(|| parse(include_str!("layers/bash-over-subprocess.json")));

fn parse(json: &str) -> Vec<AssetSegment> {
    serde_json::from_str(json).expect("invalid embedded system-prompt JSON")
}

pub(super) fn text(segments: &[AssetSegment]) -> String {
    let mut text = String::new();
    for segment in segments {
        text.push_str(&segment.body);
        text.push_str(&segment.after);
    }
    text
}

pub static CORE_LAYER: LazyLock<String> = LazyLock::new(|| text(&CORE));
pub static USAGE_LAYER: LazyLock<String> = LazyLock::new(|| text(&USAGE));
pub static OPINIONATED_LAYER: LazyLock<String> = LazyLock::new(|| text(&OPINIONATED));

/// Source file of one layer (breakdown provenance).
#[must_use]
pub fn layer_source(name: &str) -> Option<&'static str> {
    match name {
        "core" => Some("prompts/layers/core.json"),
        "usage" => Some("prompts/layers/usage.json"),
        "opinionated" => Some("prompts/layers/opinionated.json"),
        "model-prompts" => Some("prompts/layers/model_prompts.json"),
        _ => None,
    }
}
