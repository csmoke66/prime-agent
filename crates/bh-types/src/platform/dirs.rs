//! Home-directory resolution shared by every crate that expands `~`: Node `os.homedir()`
//! semantics -
//! `HOME` when set. Where
//! `os.homedir()` throws, this port returns `None` and each caller owns its fallback.

use std::path::PathBuf;

/// The user's home directory, Node `os.homedir()` semantics: `HOME` first (always wins, matching
/// the
/// TS order); `None` when `HOME` is unset or empty (the per-call-site fallback replaces the TS throw).
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

/// `BEEHIVE_CODING_AGENT_DIR`: the agent state directory override
/// (byte-compatible with the TS product).
pub const ENV_AGENT_DIR: &str = "BEEHIVE_CODING_AGENT_DIR";

/// The agent state directory name (TS `CONFIG_DIR_NAME`, `.beehive`).
pub const CONFIG_DIR_NAME: &str = ".beehive";

/// The agent state directory: the env override with a leading `~`/`~/` expanded, else
/// `<home>/.beehive`; `None` when the home directory does not resolve (each caller owns its
/// fallback).
#[must_use]
pub fn agent_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(ENV_AGENT_DIR).filter(|dir| !dir.is_empty()) {
        return Some(expand_tilde(&dir.to_string_lossy()));
    }
    home_dir().map(|home| home.join(CONFIG_DIR_NAME))
}

/// Expand a leading `~`/`~/` against [`home_dir`]; other values pass
/// through (`~foo` is not an expansion).
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().map_or_else(|| PathBuf::from(path), |home| home.join(rest));
    }
    if path == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Environment mutations are process-global: tests take this lock and
    /// restore previous values on exit.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Run `body` with the named variables set (`None` removes them),
    /// restoring the previous state afterwards.
    fn with_env(names: &[(&str, Option<&str>)], body: impl FnOnce()) {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous: Vec<(&str, Option<std::ffi::OsString>)> = names
            .iter()
            .map(|(name, _)| (*name, std::env::var_os(name)))
            .collect();
        for (name, value) in names {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        body();
        for (name, value) in previous {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }

    #[test]
    fn agent_dir_env_override_expands_tilde() {
        with_env(
            &[
                ("HOME", Some("/home/tester")),
                ("BEEHIVE_CODING_AGENT_DIR", Some("~/state")),
            ],
            || {
                assert_eq!(agent_dir(), Some(PathBuf::from("/home/tester/state")));
            },
        );
    }

    #[test]
    fn agent_dir_env_override_passthrough() {
        with_env(
            &[
                ("HOME", None),
                ("BEEHIVE_CODING_AGENT_DIR", Some("/opt/state")),
            ],
            || {
                assert_eq!(agent_dir(), Some(PathBuf::from("/opt/state")));
            },
        );
    }

    #[test]
    fn agent_dir_default_under_home() {
        with_env(
            &[
                ("HOME", Some("/home/tester")),
                ("BEEHIVE_CODING_AGENT_DIR", None),
            ],
            || {
                assert_eq!(agent_dir(), Some(PathBuf::from("/home/tester/.beehive")));
            },
        );
    }

    #[test]
    fn agent_dir_without_home_is_none() {
        with_env(
            &[("HOME", None), ("BEEHIVE_CODING_AGENT_DIR", None)],
            || {
                assert_eq!(agent_dir(), None);
            },
        );
    }

    #[test]
    fn home_dir_follows_home() {
        with_env(&[("HOME", Some("/home/tester"))], || {
            assert_eq!(home_dir(), Some(PathBuf::from("/home/tester")));
        });
    }

    #[test]
    fn home_dir_unset_is_none() {
        with_env(&[("HOME", None)], || {
            assert_eq!(home_dir(), None);
        });
    }

    #[test]
    fn home_dir_empty_is_none() {
        with_env(&[("HOME", Some(""))], || {
            assert_eq!(home_dir(), None);
        });
    }
}
