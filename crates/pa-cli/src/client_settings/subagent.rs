use std::path::Path;

use anyhow::{bail, Context, Result};
use pa_core::session::manager::read_session_header;
use pa_core::session::subagent_policy::SessionSubagentPolicy;
use pa_core::settings::ThinkingLevelSetting;
use pa_daemon::model_allowlist::{self, DaemonAllowlist};

const USAGE: &str = "/subagent-settings models <provider/model … | all | inherit>\n/subagent-settings effort <off | minimal | low | medium | high | xhigh | max | inherit>";

pub(super) fn configure(file: &Path, cwd: &Path, agent_dir: &Path, args: &str) -> Result<String> {
    let header = read_session_header(file).context("Cannot read the attached session header")?;
    let depth = u32::try_from(header.rlm_depth.unwrap_or(0))?;
    let owner = SessionSubagentPolicy::owner_log(file, depth)?;
    let mut words = args.split_whitespace();
    let policy = match words.next() {
        None => SessionSubagentPolicy::load(&owner)?,
        Some("models") => {
            let first = words.next().context(USAGE)?;
            let models = match first {
                "inherit" if words.next().is_none() => None,
                "all" if words.next().is_none() => Some(Vec::new()),
                "all" | "inherit" => bail!("Use all or inherit alone."),
                model => {
                    let mut patterns = vec![model.to_owned()];
                    for pattern in words {
                        if matches!(pattern, "all" | "inherit") {
                            bail!("Use all or inherit alone.");
                        }
                        patterns.push(pattern.to_string());
                    }
                    Some(patterns)
                }
            };
            SessionSubagentPolicy::set_models(&owner, models)?
        }
        Some("effort") => {
            let effort = match (words.next(), words.next()) {
                (Some("inherit"), None) => None,
                (Some(level), None) => Some(
                    serde_json::from_value::<ThinkingLevelSetting>(serde_json::Value::String(
                        level.into(),
                    ))
                    .with_context(|| format!("Unknown effort '{level}'.\n{USAGE}"))?,
                ),
                _ => bail!("{USAGE}"),
            };
            SessionSubagentPolicy::set_effort(&owner, effort)?
        }
        _ => bail!("{USAGE}"),
    };
    let models = match policy.allowed_models {
        Some(models) if models.is_empty() => "all (session override)".into(),
        Some(models) => format!("{} (session override)", models.join(", ")),
        None => match model_allowlist::load(cwd, agent_dir) {
            DaemonAllowlist::Allowed(models) => format!("{} (inherited)", models.join(", ")),
            DaemonAllowlist::Unrestricted => "all (inherited)".into(),
            DaemonAllowlist::Unreadable(reason) => format!("unavailable (inherited): {reason}"),
        },
    };
    let effort = policy
        .effort
        .map_or("inherit (spawn request or parent)", |level| {
            level.model_level().wire_name()
        });
    Ok(format!(
        "Sub-agents in this session family\nModels: {models}\nEffort: {effort}\n{USAGE}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_child_policy_replaces_inherits_and_never_edits_the_global_default() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let log = root.join("sessions/a.jsonl");
        std::fs::create_dir(log.parent().unwrap()).unwrap();
        std::fs::write(
            &log,
            r#"{"type":"session","id":"a","timestamp":"now","cwd":"/"}"#,
        )
        .unwrap();
        let global = r#"{"allowedModels":["original/*"]}"#;
        std::fs::write(root.join("settings.json"), global).unwrap();
        let run = |args| configure(&log, root, root, args);
        assert!(run("models other/*")
            .unwrap()
            .contains("other/* (session override)"));
        assert!(run("models all other/*").is_err());
        assert!(run("effort high")
            .unwrap()
            .contains("other/* (session override)"));
        assert!(run("models all")
            .unwrap()
            .contains("all (session override)"));
        let inherited = run("models inherit").unwrap();
        assert!(inherited.contains("original/* (inherited)"));
        assert!(inherited.contains("Effort: high"));
        assert!(run("effort inherit").unwrap().contains("Effort: inherit"));
        assert_eq!(
            std::fs::read_to_string(root.join("settings.json")).unwrap(),
            global
        );
    }
}
