//! Pattern matching and typed refusals for child-model selection. The daemon
//! resolves the effective pattern list for the session before checking it.

/// A model the daemon refused to resolve: the resolved selector is outside
/// the configured allowlist, typed so enforcement seams can downcast the
/// refusal (adoption telemetry) out of the TS-parity resolution errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelAllowlistRefusal {
    /// The refused model, full selector form `provider/model-id`.
    pub selector: String,
}

impl std::fmt::Display for ModelAllowlistRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Model \"{}\" is blocked by the daemon model allowlist for sub-agents. \
             Pick an allowed model or change this session's list in /settings → Agents.",
            self.selector
        )
    }
}

impl std::error::Error for ModelAllowlistRefusal {}

/// Whether a selector is allowed by the `allowedModels` patterns (the
/// `--models` grammar, case-insensitive, matched against the full selector
/// `provider/model-id` and the bare id, whose `test-provider` ids may
/// carry slashes): wildcards glob, plain patterns match exactly. An empty
/// pattern list allows nothing.
#[must_use]
pub fn model_allowed(selector: &str, allowlist: &[String]) -> bool {
    allowlist
        .iter()
        .any(|pattern| pattern_matches_selector(pattern, selector))
}

/// One pattern against one selector: case-insensitive exact or glob match
/// on the full selector or the bare id.
fn pattern_matches_selector(pattern: &str, selector: &str) -> bool {
    let pattern = pattern.trim().to_lowercase();
    if pattern.is_empty() {
        return false;
    }
    let selector = selector.to_lowercase();
    let bare_id = selector
        .split_once('/')
        .map_or(selector.as_str(), |(_, id)| id);
    if !pattern.contains(['*', '?', '[']) {
        return pattern == selector || pattern == bare_id;
    }
    let matcher = match globset::Glob::new(&pattern) {
        Ok(glob) => glob.compile_matcher(),
        Err(_) => return false,
    };
    matcher.is_match(&selector) || matcher.is_match(bare_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SELECTOR: &str = "test-provider/internal/glm-5.3-fast";

    #[test]
    fn exact_and_bare_id_patterns_match_case_insensitively() {
        assert!(model_allowed(
            SELECTOR,
            &["test-provider/internal/glm-5.3-fast".to_string()]
        ));
        assert!(model_allowed(
            SELECTOR,
            &["Test-Provider/Internal/GLM-5.3-Fast".to_string()]
        ));
        // The bare id (test-provider ids carry slashes) matches without
        // the provider.
        assert!(model_allowed(
            SELECTOR,
            &["internal/glm-5.3-fast".to_string()]
        ));
        assert!(!model_allowed(SELECTOR, &["glm-5.3-fast".to_string()]));
        assert!(!model_allowed(
            SELECTOR,
            &["test-provider/internal/glm-5.3-turbo".to_string()]
        ));
    }

    #[test]
    fn glob_patterns_match_the_full_selector_or_bare_id() {
        let allow = |pattern: &str| vec![pattern.to_string()];
        assert!(model_allowed(SELECTOR, &allow("test-provider/*")));
        assert!(model_allowed(SELECTOR, &allow("test-provider/internal/*")));
        assert!(model_allowed(SELECTOR, &allow("internal/*")));
        assert!(model_allowed(SELECTOR, &allow("Test-Provider/*")));
        // The provider wildcard does not swallow other providers.
        assert!(!model_allowed("zai/glm-5.3", &allow("test-provider/*")));
        // A question mark is a wildcard, not a plain character.
        assert!(model_allowed("zai/glm-5.3", &allow("zai/glm-5.?")));
        // An empty pattern list allows nothing (the settings getter never
        // produces one, but the matching must stay total).
        assert!(!model_allowed(SELECTOR, &[]));
        assert!(!model_allowed(SELECTOR, &["  ".to_string()]));
        assert!(!model_allowed(SELECTOR, &allow("[")));
    }
}
