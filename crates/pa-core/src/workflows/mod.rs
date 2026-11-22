//! Named v1 workflows. User-global definitions live under the configured agent
//! directory's `workflows` folder, project definitions live in `.prime/workflows`,
//! and operator definitions are read-only to this API. Precedence is operator,
//! project, then user-global: trust comes from OS ownership and permissions,
//! never from a field in an editable definition.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

use crate::models::resolver::is_valid_thinking_level;
use crate::workflow_io::{FieldType, Fields};

#[cfg(unix)]
#[path = "unix.rs"]
mod fs;
#[cfg(windows)]
#[path = "windows.rs"]
mod fs;

use fs::Directory;

const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// A complete workflow with its instructions already in memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workflow {
    pub version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagents: Option<WorkflowSubagents>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<WorkflowContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness_memory: Option<WorkflowHarnessMemory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interactive_apply_context_field: Option<String>,
    pub prompt: String,
    pub input_fields: Fields,
    pub output_fields: Fields,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowSubagents {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_models: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowContext {
    pub include_parent_session: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_characters: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowHarnessMemory {
    pub scope_input_field: String,
    pub interactive_apply_output_field: String,
}


impl Workflow {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported workflow version {}",
            self.version
        );
        validate_model(&self.model, "model")?;
        validate_effort(self.effort.as_deref(), "effort")?;
        if let Some(subagents) = &self.subagents {
            for model in subagents.allowed_models.as_deref().unwrap_or_default() {
                validate_model(model, "subagents.allowedModels")?;
            }
            validate_effort(subagents.effort.as_deref(), "subagents.effort")?;
        }
        if let Some(context) = &self.context {
            ensure!(
                context.max_characters.is_none_or(|limit| (4_096..=1_000_000).contains(&limit)),
                "context.maxCharacters must be between 4096 and 1000000"
            );
        }
        for (name, field) in self.input_fields.iter().chain(&self.output_fields) {
            if let Some(choices) = &field.choices {
                ensure!(!choices.is_empty(), "workflow field {name:?} choices cannot be empty");
                ensure!(
                    choices.iter().all(|value| crate::workflow_io::value_matches_type(field.kind, value)),
                    "workflow field {name:?} has a choice with the wrong type"
                );
                ensure!(
                    choices
                        .iter()
                        .enumerate()
                        .all(|(index, value)| !choices[..index].contains(value)),
                    "workflow field {name:?} choices must be unique"
                );
            }
        }
        if let Some(memory) = &self.harness_memory {
            let scope = self.input_fields.get(&memory.scope_input_field).with_context(|| {
                format!(
                    "harnessMemory.scopeInputField names unknown input field {:?}",
                    memory.scope_input_field
                )
            })?;
            ensure!(
                scope.required
                    && scope.kind == FieldType::String
                    && scope.choices.as_ref().is_some_and(|choices| {
                        choices == &[serde_json::Value::String("local".into()), serde_json::Value::String("global".into())]
                    }),
                "harnessMemory.scopeInputField must name a required string field with choices [\"local\", \"global\"]"
            );
            let output = self
                .output_fields
                .get(&memory.interactive_apply_output_field)
                .with_context(|| {
                    format!(
                        "harnessMemory.interactiveApplyOutputField names unknown output field {:?}",
                        memory.interactive_apply_output_field
                    )
                })?;
            ensure!(
                output.required && output.kind == FieldType::Array,
                "harnessMemory.interactiveApplyOutputField must name a required array output field"
            );
        }
        if let Some(field) = &self.interactive_apply_context_field {
            let output = self.output_fields.get(field).with_context(|| {
                format!("interactiveApplyContextField names unknown output field {field:?}")
            })?;
            ensure!(
                output.required && output.kind == FieldType::String,
                "interactiveApplyContextField must name a required string output field"
            );
        }
        ensure!(
            !self.prompt.trim().is_empty(),
            "workflow prompt cannot be blank"
        );
        ensure!(
            self.input_fields
                .keys()
                .chain(self.output_fields.keys())
                .all(|k| !k.trim().is_empty()),
            "workflow field names cannot be blank"
        );
        Ok(())
    }
}

fn validate_model(value: &str, field: &str) -> Result<()> {
    ensure!(
        value
            .split_once('/')
            .is_some_and(|(provider, id)| !provider.is_empty() && !id.is_empty())
            && !value.chars().any(|c| c.is_whitespace() || c.is_control())
            && !value.contains(['*', '?', '[', ']']),
        "{field} requires a qualified provider/model selector, got {value:?}"
    );
    Ok(())
}

fn validate_effort(effort: Option<&str>, field: &str) -> Result<()> {
    if let Some(effort) = effort {
        ensure!(
            is_valid_thinking_level(effort),
            "invalid {field}: {effort:?}"
        );
    }
    Ok(())
}

fn reserved_component(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or_default();
    let upper = stem.trim_end_matches(' ').to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"))
        .is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

fn valid_name(name: &str) -> bool {
    name.len() <= 64
        && name
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
        && !reserved_component(name)
}

fn definition_name(name: &str) -> Result<String> {
    ensure!(
        valid_name(name),
        "invalid workflow name {name:?}: expected 1–64 lowercase ASCII letters, digits, hyphens or underscores, starting with a letter or digit"
    );
    Ok(format!("{name}.json"))
}

fn read_text(file: File, what: &str) -> Result<String> {
    let mut text = String::new();
    file.take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .with_context(|| format!("read {what}"))?;
    ensure!(
        text.len() as u64 <= MAX_BYTES,
        "{what} exceeds {MAX_BYTES} bytes"
    );
    Ok(text)
}

fn read_workflow(dir: &Directory, filename: &OsStr) -> Result<Option<Workflow>> {
    let Some(file) = dir.open_file(filename)? else {
        return Ok(None);
    };
    let workflow: Workflow = serde_json::from_str(&read_text(file, "workflow definition")?)?;
    workflow.validate()?;
    Ok(Some(workflow))
}

fn project_dir(cwd: &Path, create: bool) -> io::Result<Option<Directory>> {
    let cwd = Directory::project_root(cwd)?;
    let Some(prime) = cwd.open_dir(OsStr::new(".prime"), create)? else {
        return Ok(None);
    };
    prime.open_dir(OsStr::new("workflows"), create)
}

fn user_dir(agent_dir: &Path) -> io::Result<Option<Directory>> {
    Directory::project_root(agent_dir)?.open_dir(OsStr::new("workflows"), false)
}

fn load_from(
    cwd: &Path,
    name: &str,
    filename: &OsStr,
    operator: Option<&Directory>,
    user: Option<&Directory>,
) -> Result<Workflow> {
    if let Some(dir) = operator {
        let definition = read_workflow(dir, filename)
            .with_context(|| format!("load operator workflow {name:?}"))?;
        if let Some(workflow) = definition {
            return Ok(workflow);
        }
    }
    if let Some(dir) = project_dir(cwd, false).context("open project workflow directory")? {
        let definition = read_workflow(&dir, filename)
            .with_context(|| format!("load project workflow {name:?}"))?;
        if let Some(workflow) = definition {
            return Ok(workflow);
        }
    }
    if let Some(dir) = user {
        let definition =
            read_workflow(dir, filename).with_context(|| format!("load user workflow {name:?}"))?;
        if let Some(workflow) = definition {
            return Ok(workflow);
        }
    }
    bail!("workflow {name:?} not found")
}

/// Load a named workflow. Operator definitions take precedence, followed by
/// project definitions and the user's `<agent dir>/workflows` registry.
///
/// # Errors
/// Returns an error for invalid names, missing or invalid definitions or
/// instructions, or an inaccessible or unsafe storage location.
pub fn load(cwd: &Path, agent_dir: &Path, name: &str) -> Result<Workflow> {
    let filename = definition_name(name)?;
    let operator = Directory::operator_root().context("open operator workflow directory")?;
    let user = user_dir(agent_dir).context("open user workflow directory")?;
    load_from(
        cwd,
        name,
        OsStr::new(&filename),
        operator.as_ref(),
        user.as_ref(),
    )
}

fn create_in(
    cwd: &Path,
    name: &str,
    filename: &OsStr,
    workflow: &Workflow,
    operator: Option<&Directory>,
) -> Result<()> {
    workflow.validate()?;
    if let Some(dir) = operator {
        ensure!(
            !dir.contains(filename)?,
            "workflow {name:?} is reserved by the operator"
        );
    }
    let mut contents = serde_json::to_vec_pretty(workflow)?;
    contents.push(b'\n');
    ensure!(
        contents.len() as u64 <= MAX_BYTES,
        "workflow definition exceeds {MAX_BYTES} bytes"
    );
    let dir = project_dir(cwd, true)
        .context("open project workflow directory")?
        .context("project workflow directory disappeared")?;
    dir.publish(filename, &contents).with_context(|| {
        format!("create project workflow {name:?} without replacing any existing entry")
    })
}

/// Create a new project workflow with inline instructions. Never overwrites
/// an existing entry or creates a shadow of an operator entry.
///
/// # Errors
/// Returns an error for an invalid name or definition, an occupied name, or
/// an inaccessible or unsafe storage location.
pub fn create(cwd: &Path, name: &str, workflow: &Workflow) -> Result<()> {
    let filename = definition_name(name)?;
    let operator = Directory::operator_root().context("open operator workflow directory")?;
    create_in(
        cwd,
        name,
        OsStr::new(&filename),
        workflow,
        operator.as_ref(),
    )
}

fn list_from(
    cwd: &Path,
    operator: Option<&Directory>,
    user: Option<&Directory>,
) -> Result<Vec<String>> {
    let mut names = BTreeSet::new();
    let project = project_dir(cwd, false).context("open project workflow directory")?;
    for dir in [operator, project.as_ref(), user].into_iter().flatten() {
        dir.for_each_file(|filename| {
            let Some(filename) = filename.to_str() else {
                return;
            };
            // Windows can resolve ASCII case variations of a file name.
            #[cfg(windows)]
            let filename = filename.to_ascii_lowercase();
            if let Some(name) = filename
                .strip_suffix(".json")
                .filter(|name| valid_name(name))
            {
                names.insert(name.to_owned());
            }
        })?;
    }
    Ok(names.into_iter().collect())
}

/// List sorted, unique names from the operator, project, and user registries
/// without reading their definitions. Use [`load`] to validate a definition.
///
/// # Errors
/// Returns an error if a registry cannot be safely opened or enumerated.
pub fn list(cwd: &Path, agent_dir: &Path) -> Result<Vec<String>> {
    let operator = Directory::operator_root().context("open operator workflow directory")?;
    let user = user_dir(agent_dir).context("open user workflow directory")?;
    list_from(cwd, operator.as_ref(), user.as_ref())
}

#[cfg(test)]
mod tests;
