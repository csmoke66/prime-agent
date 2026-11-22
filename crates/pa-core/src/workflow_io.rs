//! Definition-owned schemas for workflow input values and final JSON answers.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub type Fields = BTreeMap<String, Field>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowInput<'a> {
    pub data: Map<String, Value>,
    pub output_fields: &'a Fields,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_context: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    #[serde(rename = "type")]
    pub kind: FieldType,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
    Null,
}

impl<'a> WorkflowInput<'a> {
    pub fn parse(workflow: &'a crate::workflows::Workflow, input: Value) -> Result<Self> {
        let Value::Object(data) = input else {
            anyhow::bail!("workflow input must be a JSON object")
        };
        validate_fields("input", &workflow.input_fields, &data)?;
        Ok(Self {
            data,
            output_fields: &workflow.output_fields,
            parent_context: None,
        })
    }
}

pub fn parse_output(fields: &Fields, text: &str) -> Result<Value> {
    let output: Value = serde_json::from_str(text).context("workflow final answer is not JSON")?;
    validate_fields(
        "output",
        fields,
        output
            .as_object()
            .context("workflow output must be a JSON object")?,
    )?;
    Ok(output)
}

fn validate_fields(side: &str, fields: &Fields, obj: &Map<String, Value>) -> Result<()> {
    for (key, field) in fields {
        let Some(value) = obj.get(key) else {
            ensure!(
                !field.required,
                "workflow {side} is missing required field {key:?}"
            );
            continue;
        };
        let valid = match field.kind {
            FieldType::String => value.is_string(),
            FieldType::Number => value.is_number(),
            FieldType::Integer => value.is_i64() || value.is_u64(),
            FieldType::Boolean => value.is_boolean(),
            FieldType::Array => value.is_array(),
            FieldType::Object => value.is_object(),
            FieldType::Null => value.is_null(),
        };
        ensure!(
            valid,
            "workflow {side} field {key:?} has the wrong type (expected {:?})",
            field.kind
        );
    }
    if let Some(extra) = obj.keys().find(|k| !fields.contains_key(*k)) {
        anyhow::bail!("workflow {side} contains undeclared field {extra:?}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn optional_is_not_nullable_and_wrong_or_partial_answers_fail() {
        let workflow = serde_json::from_value(json!({"version":1,"model":"test/local","prompt":"Check","inputFields":{"items":{"type":"array","required":true}}, "outputFields":{
            "ok":{"type":"boolean","required":true},"why":{"type":"string","required":false}}})).unwrap();
        for bad in [
            json!({}),
            json!({"items":null}),
            json!({"items":[],"outputFields":{}}),
        ] {
            assert!(WorkflowInput::parse(&workflow, bad).is_err());
        }
        let input = WorkflowInput::parse(&workflow, json!({"items":[7]})).unwrap();
        assert_eq!(
            parse_output(input.output_fields, r#"{"ok":false}"#).unwrap(),
            json!({"ok": false})
        );
        for bad in [
            "{}",
            "true",
            "plain text",
            r#"{"ok":null}"#,
            r#"{"ok":true,"why":null}"#,
            r#"{"ok":true,"extra":1}"#,
        ] {
            assert!(parse_output(input.output_fields, bad).is_err(), "{bad}");
        }
    }
}
