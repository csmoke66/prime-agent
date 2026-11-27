//! Session-model restore: a saved session model is restored only when it is
//! registered and its provider has configured auth.

use pa_types::ai::Model;

use super::registry::ModelRegistry;

/// Find a saved session model: registered, with configured provider auth
/// (TS `findRestorable`). A miss returns `None`, leaving the caller to fall
/// back (and to say so: the fallback must never be silent).
#[must_use]
pub fn find_session_model(
    registry: &ModelRegistry,
    provider: &str,
    model_id: &str,
) -> Option<Model> {
    registry
        .get_all()
        .iter()
        .find(|model| model.provider == provider && model.id == model_id)
        .filter(|model| registry.has_configured_auth(model))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_requires_a_registered_model_with_configured_auth() {
        let auth = crate::auth::AuthStorage::in_memory_without_env(
            &crate::auth::AuthStorageData::default(),
            std::sync::Arc::new(crate::auth::NoOAuth),
        );
        let registry = ModelRegistry::in_memory(auth);
        assert!(
            find_session_model(&registry, "anthropic", "claude-sonnet-5-5").is_none(),
            "no provider auth configured: not restorable"
        );
        assert!(
            find_session_model(&registry, "anthropic", "missing").is_none(),
            "not registered: not restorable"
        );
    }
}
