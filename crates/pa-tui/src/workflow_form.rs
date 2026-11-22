//! Definition-driven workflow selection and input for `/run`.
use crate::keybindings::KeybindingsManager;
use crate::menu_panel::{hint_row, menu_row, scrub_controls, search_field_lines};
use crate::search_input::SearchInput;
use crate::theme::Theme;
use crate::{Line, Span};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct WorkflowCatalog {
    names: Vec<String>,
    definitions: BTreeMap<String, Definition>,
    #[serde(default)]
    errors: BTreeMap<String, String>,
}

/// The part of a daemon workflow definition needed by the client form.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Definition {
    description: Option<String>,
    input_fields: BTreeMap<String, Field>,
}

#[derive(Debug, Deserialize)]
struct Field {
    #[serde(rename = "type")]
    kind: FieldType,
    required: bool,
    description: Option<String>,
    #[serde(default)]
    choices: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum FieldType {
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
    Null,
}

impl FieldType {
    fn label(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Integer => "integer",
            Self::Boolean => "boolean",
            Self::Array => "array",
            Self::Object => "object",
            Self::Null => "null",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Self::String => r#"Text or JSON string (use "" for empty text)"#,
            Self::Boolean => "true or false",
            Self::Null => "null",
            Self::Array => "JSON array, e.g. [1, 2]",
            Self::Object => r#"JSON object, e.g. {"key": 1}"#,
            Self::Number => "JSON number, e.g. 1.5",
            Self::Integer => "JSON integer, e.g. 42",
        }
    }

    fn parse(self, text: &str, name: &str) -> Result<Value, String> {
        if matches!(self, Self::String) {
            return if text.trim_start().starts_with('"') {
                serde_json::from_str::<String>(text)
                    .map(Value::String)
                    .map_err(|err| format!("{name:?}: {err}"))
            } else {
                Ok(Value::String(text.to_owned()))
            };
        }
        let value: Value = serde_json::from_str(text).map_err(|err| format!("{name:?}: {err}"))?;
        let valid = match self {
            Self::String => unreachable!(),
            Self::Number => value.is_number(),
            Self::Integer => value.is_i64() || value.is_u64(),
            Self::Boolean => value.is_boolean(),
            Self::Array => value.is_array(),
            Self::Object => value.is_object(),
            Self::Null => value.is_null(),
        };
        if valid {
            Ok(value)
        } else {
            Err(format!("{name:?}: expected {}", self.label()))
        }
    }
}

#[derive(Debug)]
struct Input {
    name: String,
    field: Field,
    editor: SearchInput,
}

#[derive(Debug)]
enum Stage {
    Select {
        choices: Vec<(String, Definition)>,
        invalid: usize,
    },
    Edit {
        name: String,
        description: Option<String>,
        inputs: Vec<Input>,
    },
}

#[derive(Debug)]
pub(crate) struct WorkflowForm {
    stage: Stage,
    focus: usize,
    error: Option<String>,
}

pub(crate) enum WorkflowFormAction {
    None,
    Cancel,
    Submit { name: String, input: Value },
}

impl WorkflowForm {
    pub(crate) fn open(name: &str, catalog: Value) -> Result<Self, String> {
        let WorkflowCatalog {
            names,
            mut definitions,
            errors,
        } = serde_json::from_value(catalog)
            .map_err(|err| format!("Cannot read workflow list: {err}"))?;
        if !name.is_empty() {
            return if let Some(definition) = definitions.remove(name) {
                Ok(Self::edit(name.to_owned(), definition))
            } else if let Some(error) = errors.get(name) {
                Err(format!("Cannot load workflow {name:?}: {error}"))
            } else if names.iter().any(|listed| listed == name) {
                Err(format!(
                    "Workflow list is missing the definition for {name:?}"
                ))
            } else {
                Err(format!("Unknown workflow {name:?}"))
            };
        }

        let mut choices = Vec::with_capacity(names.len());
        for name in names {
            if let Some(definition) = definitions.remove(&name) {
                choices.push((name, definition));
            } else if !errors.contains_key(&name) {
                return Err(format!(
                    "Workflow list is missing the definition for {name:?}"
                ));
            }
        }
        if choices.is_empty() {
            return match errors.first_key_value() {
                Some((name, error)) => Err(format!(
                    "No usable workflows ({} invalid): {name}: {error}",
                    errors.len()
                )),
                None => Err("No workflows available".into()),
            };
        }
        Ok(Self {
            stage: Stage::Select {
                choices,
                invalid: errors.len(),
            },
            focus: 0,
            error: None,
        })
    }

    fn edit(name: String, definition: Definition) -> Self {
        Self {
            stage: Stage::Edit {
                name,
                description: definition.description,
                inputs: definition
                    .input_fields
                    .into_iter()
                    .map(|(name, field)| {
                        let mut editor = SearchInput::new();
                        if let Some(first) = field.choices.as_ref().and_then(|choices| choices.first()) {
                            editor.prefill(&choice_text(first));
                        }
                        Input {
                            name,
                            field,
                            editor,
                        }
                    })
                    .collect(),
            },
            focus: 0,
            error: None,
        }
    }

    fn count(&self) -> usize {
        match &self.stage {
            Stage::Select { choices, .. } => choices.len(),
            Stage::Edit { inputs, .. } => inputs.len() + 1,
        }
    }

    pub(crate) fn paste(&mut self, text: &str) {
        if let Stage::Edit { inputs, .. } = &mut self.stage {
            if let Some(input) = inputs.get_mut(self.focus) {
                if input.field.choices.is_some() {
                    return;
                }
                if text.contains(['\n', '\r']) {
                    if matches!(input.field.kind, FieldType::String)
                        && input.editor.value().is_empty()
                        && serde_json::from_str::<String>(text).is_err()
                    {
                        // Keep line breaks in a pasted plain-text paragraph.
                        input.editor.paste(
                            &serde_json::to_string(text).expect("a string serializes to JSON"),
                        );
                    } else {
                        // JSON whitespace cannot be stripped between tokens.
                        input.editor.paste(&text.replace(['\n', '\r'], " "));
                    }
                } else {
                    input.editor.paste(text);
                }
                self.error = None;
            }
        }
    }

    fn request(inputs: &[Input]) -> Result<Value, (usize, String)> {
        let mut values = Map::new();
        for (index, Input { name, field, editor }) in inputs.iter().enumerate() {
            let text = editor.value();
            let blank = if matches!(field.kind, FieldType::String) {
                text.is_empty()
            } else {
                text.trim().is_empty()
            };
            if blank {
                if field.required {
                    return Err((index, format!("{name:?} is required")));
                }
                continue;
            }
            let value = field.kind.parse(text, name).map_err(|err| (index, err))?;
            if field
                .choices
                .as_ref()
                .is_some_and(|choices| !choices.contains(&value))
            {
                return Err((index, format!("{name:?}: choose one of the listed values")));
            }
            values.insert(name.clone(), value);
        }
        Ok(Value::Object(values))
    }

    pub(crate) fn handle_key(&mut self, key: &str, kb: &KeybindingsManager) -> WorkflowFormAction {
        let count = self.count();
        self.error = None;
        match key {
            "escape" | "esc" | "ctrl+c" => return WorkflowFormAction::Cancel,
            "up" | "shift+tab" => self.focus = (self.focus + count - 1) % count,
            "down" | "tab" => self.focus = (self.focus + 1) % count,
            _ => match &mut self.stage {
                Stage::Select { choices, .. } => match key {
                    "ctrl+p" => self.focus = (self.focus + count - 1) % count,
                    "ctrl+n" => self.focus = (self.focus + 1) % count,
                    "enter" | "space" => {
                        let (name, definition) = choices.swap_remove(self.focus);
                        *self = Self::edit(name, definition);
                    }
                    _ => {}
                },
                Stage::Edit { name, inputs, .. } => {
                    if let Some(input) = inputs.get_mut(self.focus) {
                        if key == "enter" {
                            self.focus += 1;
                        } else if let Some(choices) = &input.field.choices {
                            if matches!(key, "left" | "right" | "space") {
                                let current = choices
                                    .iter()
                                    .position(|value| choice_text(value) == input.editor.value())
                                    .unwrap_or(0);
                                let next = if key == "left" {
                                    (current + choices.len() - 1) % choices.len()
                                } else {
                                    (current + 1) % choices.len()
                                };
                                input.editor.prefill(&choice_text(&choices[next]));
                            }
                        } else {
                            input.editor.handle_key(key, kb);
                        }
                    } else if matches!(key, "enter" | "space") {
                        match Self::request(inputs) {
                            Ok(input) => {
                                return WorkflowFormAction::Submit {
                                    name: name.clone(),
                                    input,
                                };
                            }
                            Err((index, error)) => {
                                self.focus = index;
                                self.error = Some(error);
                            }
                        }
                    }
                }
            },
        }
        WorkflowFormAction::None
    }

    pub(crate) fn render(&self, theme: &Theme, width: usize) -> Vec<Line> {
        let count = self.count();
        let start = self.focus.saturating_sub(4).min(count.saturating_sub(8));
        let visible = start..(start + 8).min(count);
        let mut lines = Vec::new();
        match &self.stage {
            Stage::Select { choices, invalid } => {
                lines.push(hint_row(theme, width, "Choose a workflow"));
                for (n, (name, _)) in choices.iter().enumerate().skip(start).take(8) {
                    lines.push(menu_row(
                        theme,
                        width,
                        vec![Span::raw(name.clone())],
                        &[],
                        n == self.focus,
                    ));
                }
                push_description(
                    &mut lines,
                    theme,
                    width,
                    choices[self.focus].1.description.as_deref(),
                );
                if *invalid > 0 {
                    lines.push(hint_row(
                        theme,
                        width,
                        &format!("{invalid} invalid workflow definition(s) not shown"),
                    ));
                }
                lines.push(hint_row(
                    theme,
                    width,
                    "↑/↓ or Tab: move · Enter: choose · Esc: cancel",
                ));
            }
            Stage::Edit {
                name,
                description,
                inputs,
            } => {
                lines.push(hint_row(theme, width, &format!("Run workflow · {name}")));
                push_description(&mut lines, theme, width, description.as_deref());
                for n in visible {
                    let focused = n == self.focus;
                    let input = inputs.get(n);
                    let label = input.map_or_else(
                        || "Run workflow".into(),
                        |input| {
                            let presence = if input.field.required {
                                "required"
                            } else {
                                "optional"
                            };
                            let name = input.name.escape_debug();
                            let kind = input.field.kind.label();
                            let value = input.editor.value();
                            if focused || value.is_empty() {
                                format!("{name} ({kind}, {presence})")
                            } else {
                                format!("{name} ({kind}, {presence})  {value}")
                            }
                        },
                    );
                    lines.push(menu_row(theme, width, vec![Span::raw(label)], &[], focused));
                    if focused {
                        if let Some(input) = input {
                            push_description(
                                &mut lines,
                                theme,
                                width,
                                input.field.description.as_deref(),
                            );
                            if input.field.choices.is_some() {
                                lines.push(hint_row(theme, width, "←/→ or Space: choose"));
                            } else {
                                lines.extend(search_field_lines(
                                    theme,
                                    width,
                                    input.editor.value(),
                                    input.editor.cursor(),
                                    true,
                                    input.field.kind.placeholder(),
                                ));
                            }
                        }
                    }
                }
                if let Some(error) = &self.error {
                    lines.push(hint_row(theme, width, error));
                }
                lines.push(hint_row(
                    theme,
                    width,
                    "Tab: move · Enter: next/run · Blank omits optional · Esc: cancel",
                ));
            }
        }
        lines
    }
}

fn choice_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn push_description(lines: &mut Vec<Line>, theme: &Theme, width: usize, description: Option<&str>) {
    if let Some(description) = description.and_then(|text| text.lines().next()) {
        lines.push(hint_row(theme, width, &scrub_controls(description)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalog(fields: Value) -> Value {
        json!({
            "names": ["review"],
            "definitions": {
                "review": {
                    "version": 1,
                    "model": "provider/model",
                    "prompt": "Review the changes.",
                    "inputFields": fields,
                    "outputFields": {"summary": {"type": "string", "required": true}}
                }
            }
        })
    }

    #[test]
    fn form_sends_only_declared_input_values_and_preserves_types() {
        let mut form = WorkflowForm::open(
            "review",
            catalog(json!({
                "array": {"type": "array", "required": true},
                "boolean": {"type": "boolean", "required": true},
                "empty": {"type": "string", "required": false},
                "integer": {"type": "integer", "required": true},
                "null": {"type": "null", "required": false},
                "number": {"type": "number", "required": true},
                "object": {"type": "object", "required": true},
                "omitted": {"type": "string", "required": false},
                "text": {"type": "string", "required": true}
            })),
        )
        .unwrap();
        let kb = KeybindingsManager::new();
        for text in ["[1,\nfalse]", "false", r#""""#, "9", "null", "1.5", "{}", "", "123"] {
            form.paste(text);
            form.handle_key("enter", &kb);
        }
        let WorkflowFormAction::Submit { name, input } = form.handle_key("enter", &kb) else {
            panic!("form did not submit: {form:?}")
        };
        assert_eq!(name, "review");
        assert_eq!(
            input,
            json!({
                "array": [1, false], "boolean": false, "empty": "", "integer": 9,
                "null": null, "number": 1.5, "object": {}, "text": "123"
            })
        );
    }

    #[test]
    fn form_finds_missing_and_wrong_type_values_before_submitting() {
        let mut form = WorkflowForm::open(
            "review",
            catalog(json!({
                "count": {"type": "integer", "required": true}
            })),
        )
        .unwrap();
        let kb = KeybindingsManager::new();
        form.handle_key("tab", &kb);
        assert!(matches!(
            form.handle_key("enter", &kb),
            WorkflowFormAction::None
        ));
        assert_eq!(form.error.as_deref(), Some(r#""count" is required"#));
        assert_eq!(form.focus, 0);
        form.paste("1.5");
        form.handle_key("tab", &kb);
        assert!(matches!(
            form.handle_key("enter", &kb),
            WorkflowFormAction::None
        ));
        assert_eq!(form.error.as_deref(), Some(r#""count": expected integer"#));
        assert_eq!(form.focus, 0);
    }

    #[test]
    fn picker_opens_the_selected_definition_and_skips_invalid_ones() {
        let mut response = catalog(json!({}));
        response["names"] = json!(["review", "broken", "report"]);
        response["errors"] = json!({"broken": "missing prompt"});
        response["definitions"]["report"] = json!({
            "inputFields": {"message": {"type": "string", "required": true}}
        });
        let mut form = WorkflowForm::open("", response.clone()).unwrap();
        let kb = KeybindingsManager::new();
        form.handle_key("down", &kb);
        form.handle_key("enter", &kb);
        form.paste("line one\nline two");
        form.handle_key("tab", &kb);
        let WorkflowFormAction::Submit { name, input } = form.handle_key("enter", &kb) else {
            panic!("form did not submit: {form:?}")
        };
        assert_eq!(name, "report");
        assert_eq!(input, json!({"message": "line one\nline two"}));
        assert_eq!(
            WorkflowForm::open("broken", response).unwrap_err(),
            r#"Cannot load workflow "broken": missing prompt"#
        );
    }

    #[test]
    fn choice_fields_are_prefilled_and_cycle_without_free_text() {
        let mut form = WorkflowForm::open(
            "review",
            catalog(json!({
                "scope": {
                    "type": "string",
                    "required": true,
                    "choices": ["local", "global"]
                }
            })),
        )
        .unwrap();
        let kb = KeybindingsManager::new();
        form.paste("invalid");
        form.handle_key("right", &kb);
        form.handle_key("enter", &kb);
        let WorkflowFormAction::Submit { input, .. } = form.handle_key("enter", &kb) else {
            panic!("form did not submit: {form:?}")
        };
        assert_eq!(input, json!({"scope": "global"}));
    }

    #[test]
    fn empty_and_malformed_catalogs_do_not_open_a_form() {
        assert_eq!(
            WorkflowForm::open("", json!({"names": [], "definitions": {}})).unwrap_err(),
            "No workflows available"
        );
        assert!(WorkflowForm::open("", json!({"names": []}))
            .unwrap_err()
            .starts_with("Cannot read workflow list:"));
        assert_eq!(
            WorkflowForm::open(
                "",
                json!({
                    "names": ["broken"],
                    "definitions": {},
                    "errors": {"broken": "missing prompt"}
                })
            )
            .unwrap_err(),
            "No usable workflows (1 invalid): broken: missing prompt"
        );
    }
}
