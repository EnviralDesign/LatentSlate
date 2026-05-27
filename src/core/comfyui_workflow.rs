#![allow(dead_code)]
// Kept for the provider/workflow builder path. The egui shell is not currently
// invoking workflow introspection, but the parser is still part of the planned
// provider configuration surface.

use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ComfyWorkflowNode {
    pub id: String,
    pub class_type: String,
    pub title: Option<String>,
    pub inputs: Vec<String>,
    pub input_values: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default)]
pub struct ComfyInputSchema {
    pub required: bool,
    pub type_name: Option<String>,
    pub enum_options: Vec<String>,
    pub default: Option<Value>,
    pub multiline: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
}

pub type ComfyObjectInfoMap = HashMap<String, HashMap<String, ComfyInputSchema>>;

pub fn load_workflow_nodes(path: &Path) -> Result<Vec<ComfyWorkflowNode>, String> {
    let json =
        std::fs::read_to_string(path).map_err(|err| format!("Failed to read workflow: {}", err))?;
    let value: Value =
        serde_json::from_str(&json).map_err(|err| format!("Invalid workflow JSON: {}", err))?;
    parse_workflow_nodes(&value)
}

pub fn parse_workflow_nodes(value: &Value) -> Result<Vec<ComfyWorkflowNode>, String> {
    let Some(map) = value.as_object() else {
        return Err("Workflow JSON must be an object.".to_string());
    };

    let mut nodes = Vec::new();
    for (node_id, node_value) in map.iter() {
        let Some(node_obj) = node_value.as_object() else {
            continue;
        };
        let class_type = node_obj
            .get("class_type")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string();
        let title = node_obj
            .get("_meta")
            .and_then(|meta| meta.get("title"))
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        let mut inputs = Vec::new();
        let mut input_values = HashMap::new();
        if let Some(input_map) = node_obj.get("inputs").and_then(|value| value.as_object()) {
            for (key, value) in input_map {
                inputs.push(key.clone());
                input_values.insert(key.clone(), value.clone());
            }
            inputs.sort();
        }

        nodes.push(ComfyWorkflowNode {
            id: node_id.clone(),
            class_type,
            title,
            inputs,
            input_values,
        });
    }

    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(nodes)
}

pub fn parse_object_info_schema(value: &Value) -> ComfyObjectInfoMap {
    let Some(classes) = value.as_object() else {
        return ComfyObjectInfoMap::new();
    };

    let mut parsed = ComfyObjectInfoMap::new();
    for (class_type, class_value) in classes {
        let Some(input_sections) = class_value.get("input").and_then(Value::as_object) else {
            continue;
        };

        let mut inputs = HashMap::new();
        for (section_name, required) in [("required", true), ("optional", false)] {
            let Some(input_map) = input_sections.get(section_name).and_then(Value::as_object)
            else {
                continue;
            };

            for (input_key, input_schema) in input_map {
                if let Some(schema) = parse_object_info_input_schema(input_schema, required) {
                    inputs.insert(input_key.clone(), schema);
                }
            }
        }

        if !inputs.is_empty() {
            parsed.insert(class_type.clone(), inputs);
        }
    }

    parsed
}

fn parse_object_info_input_schema(value: &Value, required: bool) -> Option<ComfyInputSchema> {
    let Some(values) = value.as_array() else {
        return None;
    };
    let type_value = values.first()?;
    let metadata = values.get(1).and_then(Value::as_object);

    let (type_name, enum_options) = match type_value {
        Value::Array(options) => (
            Some("ENUM".to_string()),
            options
                .iter()
                .filter_map(schema_option_to_string)
                .collect::<Vec<_>>(),
        ),
        Value::String(type_name) => (Some(type_name.clone()), Vec::new()),
        other => (schema_option_to_string(other), Vec::new()),
    };

    let default = metadata.and_then(|map| map.get("default").cloned());
    let multiline = metadata
        .and_then(|map| map.get("multiline"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    Some(ComfyInputSchema {
        required,
        type_name,
        enum_options,
        default,
        multiline,
        min: metadata
            .and_then(|map| map.get("min"))
            .and_then(Value::as_f64),
        max: metadata
            .and_then(|map| map.get("max"))
            .and_then(Value::as_f64),
        step: metadata
            .and_then(|map| map.get("step"))
            .and_then(Value::as_f64),
    })
}

fn schema_option_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}
