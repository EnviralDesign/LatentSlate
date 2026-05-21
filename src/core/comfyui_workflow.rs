use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ComfyWorkflowNode {
    pub id: String,
    pub class_type: String,
    pub title: Option<String>,
    pub inputs: Vec<String>,
}

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
        if let Some(input_map) = node_obj.get("inputs").and_then(|value| value.as_object()) {
            for key in input_map.keys() {
                inputs.push(key.clone());
            }
            inputs.sort();
        }

        nodes.push(ComfyWorkflowNode {
            id: node_id.clone(),
            class_type,
            title,
            inputs,
        });
    }

    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(nodes)
}
