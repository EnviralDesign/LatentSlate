#![allow(dead_code)]
// The ComfyUI provider adapter is preserved while the egui refactor reconnects
// generation workflows. It is intentionally compiled but not driven by the
// current UI yet.

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

use crate::core::paths;
use crate::state::{
    input_value_as_bool, input_value_as_f64, input_value_as_i64, BindingTransform, ManifestInput,
    NodeSelector, ProviderInputType, ProviderManifest, ProviderOutputType,
};

const DEFAULT_WORKFLOW_PATH: &str = "workflows/sdxl_simple_example_API.json";
const OUTPUT_NODE_ID: &str = "53";
const DEFAULT_OUTPUT_KEY: &str = "images";

#[derive(Debug, Clone)]
pub struct ComfyUiOutput {
    pub bytes: Vec<u8>,
    pub extension: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ComfyUiProgress {
    pub overall: Option<f32>,
    pub node: Option<f32>,
}

impl ComfyUiProgress {
    fn overall(value: f32) -> Self {
        Self {
            overall: Some(value),
            node: None,
        }
    }

    fn node(value: f32) -> Self {
        Self {
            overall: None,
            node: Some(value),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum InputCoercion {
    String,
    Integer,
    Float,
}

struct WorkflowInputBinding {
    name: &'static str,
    node_id: &'static str,
    input_key: &'static str,
    coercion: InputCoercion,
}

const WORKFLOW_INPUTS: &[WorkflowInputBinding] = &[
    WorkflowInputBinding {
        name: "prompt",
        node_id: "6",
        input_key: "text",
        coercion: InputCoercion::String,
    },
    WorkflowInputBinding {
        name: "negative_prompt",
        node_id: "7",
        input_key: "text",
        coercion: InputCoercion::String,
    },
    WorkflowInputBinding {
        name: "seed",
        node_id: "10",
        input_key: "noise_seed",
        coercion: InputCoercion::Integer,
    },
    WorkflowInputBinding {
        name: "steps",
        node_id: "10",
        input_key: "steps",
        coercion: InputCoercion::Integer,
    },
    WorkflowInputBinding {
        name: "cfg",
        node_id: "10",
        input_key: "cfg",
        coercion: InputCoercion::Float,
    },
    WorkflowInputBinding {
        name: "width",
        node_id: "5",
        input_key: "width",
        coercion: InputCoercion::Integer,
    },
    WorkflowInputBinding {
        name: "height",
        node_id: "5",
        input_key: "height",
        coercion: InputCoercion::Integer,
    },
    WorkflowInputBinding {
        name: "checkpoint",
        node_id: "4",
        input_key: "ckpt_name",
        coercion: InputCoercion::String,
    },
    WorkflowInputBinding {
        name: "sampler",
        node_id: "10",
        input_key: "sampler_name",
        coercion: InputCoercion::String,
    },
    WorkflowInputBinding {
        name: "scheduler",
        node_id: "10",
        input_key: "scheduler",
        coercion: InputCoercion::String,
    },
    WorkflowInputBinding {
        name: "start_step",
        node_id: "68",
        input_key: "value",
        coercion: InputCoercion::Integer,
    },
];

/// Resolves a ComfyUI workflow path relative to the app root/exe as needed.
pub fn resolve_workflow_path(path: Option<&str>) -> PathBuf {
    let resolved = path.unwrap_or(DEFAULT_WORKFLOW_PATH);
    paths::resolve_resource_path(Path::new(resolved))
}

/// Resolves an optional manifest path relative to the app root/exe as needed.
pub fn resolve_manifest_path(path: Option<&str>) -> Option<PathBuf> {
    let path = path?;
    Some(paths::resolve_resource_path(Path::new(path)))
}

/// Lightweight health check for a ComfyUI instance.
pub async fn check_health(base_url: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|err| format!("Failed to build HTTP client: {}", err))?;
    let url = format!("{}/system_stats", base_url.trim_end_matches('/'));
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("Connection failed: {}", err))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("Health check failed ({})", response.status()))
    }
}

/// Submits a ComfyUI workflow and downloads the first output matching the output type.
pub async fn generate_output(
    base_url: &str,
    workflow_path: &Path,
    inputs: &HashMap<String, Value>,
    manifest_path: Option<&Path>,
    output_type: ProviderOutputType,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<ComfyUiProgress>>,
) -> Result<ComfyUiOutput, String> {
    let mut workflow = load_workflow(workflow_path)?;
    let total_nodes = workflow.as_object().map(|map| map.len()).unwrap_or(0);
    let (output_node_id, output_key, output_index) = if let Some(path) = manifest_path {
        let manifest = load_manifest(path)?;
        let (manifest_inputs, output_selector) = match manifest {
            ProviderManifest::ComfyUi { inputs, output, .. } => (inputs, output),
            _ => {
                return Err(
                    "Provider manifest adapter_type must be comfy_ui for ComfyUI providers."
                        .to_string(),
                )
            }
        };
        apply_manifest_inputs(&mut workflow, inputs, &manifest_inputs)?;
        let node_id = resolve_output_node_id(&workflow, &output_selector.selector)?;
        (
            Some(node_id),
            Some(output_selector.selector.input_key.clone()),
            output_selector.index,
        )
    } else {
        apply_inputs(&mut workflow, inputs)?;
        if output_type == ProviderOutputType::Image {
            (
                Some(OUTPUT_NODE_ID.to_string()),
                Some(DEFAULT_OUTPUT_KEY.to_string()),
                None,
            )
        } else {
            (None, None, None)
        }
    };

    let client = reqwest::Client::new();
    let prompt_id = submit_prompt(&client, base_url, &workflow).await?;
    let ws_task = progress_tx.map(|tx| {
        let base_url = base_url.to_string();
        let prompt_id = prompt_id.clone();
        let total_nodes = total_nodes;
        tokio::spawn(async move {
            let _ = listen_progress_ws(&base_url, &prompt_id, total_nodes, tx).await;
        })
    });
    let outputs = poll_history(&client, base_url, &prompt_id).await?;
    if let Some(task) = ws_task {
        task.abort();
    }
    let output_ref = find_output_ref(
        &outputs,
        output_node_id.as_deref(),
        output_key.as_deref(),
        output_index,
        output_type,
    )
    .ok_or_else(|| {
        format!(
            "ComfyUI history did not include {} outputs. This can happen when cached \
results are returned for identical inputs; try changing the seed or using batch seed offsets.",
            output_type_label(output_type)
        )
    })?;
    let bytes = download_output(&client, base_url, &output_ref).await?;

    let extension = Path::new(&output_ref.filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_else(|| default_extension_for_output(output_type))
        .to_string();

    Ok(ComfyUiOutput { bytes, extension })
}

fn load_workflow(path: &Path) -> Result<Value, String> {
    let json =
        std::fs::read_to_string(path).map_err(|err| format!("Failed to read workflow: {}", err))?;
    serde_json::from_str(&json).map_err(|err| format!("Invalid workflow JSON: {}", err))
}

fn load_manifest(path: &Path) -> Result<ProviderManifest, String> {
    let json =
        std::fs::read_to_string(path).map_err(|err| format!("Failed to read manifest: {}", err))?;
    serde_json::from_str(&json).map_err(|err| format!("Invalid manifest JSON: {}", err))
}

fn apply_inputs(workflow: &mut Value, inputs: &HashMap<String, Value>) -> Result<(), String> {
    for binding in WORKFLOW_INPUTS.iter() {
        let Some(value) = inputs.get(binding.name) else {
            continue;
        };
        let coerced = coerce_value(value, binding.coercion)
            .map_err(|err| format!("Input {}: {}", binding.name, err))?;
        set_workflow_input(workflow, binding.node_id, binding.input_key, coerced)?;
    }
    Ok(())
}

fn apply_manifest_inputs(
    workflow: &mut Value,
    inputs: &HashMap<String, Value>,
    manifest_inputs: &[ManifestInput],
) -> Result<(), String> {
    for manifest_input in manifest_inputs {
        let Some(value) = inputs.get(&manifest_input.name) else {
            continue;
        };
        let node_id = resolve_node_id(workflow, &manifest_input.bind.selector)?;
        let mut resolved = apply_binding_transform(value, manifest_input.bind.transform.as_ref())?;
        resolved = coerce_manifest_value(&resolved, &manifest_input.input_type)
            .map_err(|err| format!("Input {}: {}", manifest_input.name, err))?;
        set_workflow_input(
            workflow,
            &node_id,
            &manifest_input.bind.selector.input_key,
            resolved,
        )?;
    }
    Ok(())
}

fn resolve_node_id(workflow: &Value, selector: &NodeSelector) -> Result<String, String> {
    resolve_node_id_internal(workflow, selector, true)
}

fn resolve_output_node_id(workflow: &Value, selector: &NodeSelector) -> Result<String, String> {
    resolve_node_id_internal(workflow, selector, false)
}

fn resolve_node_id_internal(
    workflow: &Value,
    selector: &NodeSelector,
    require_input_key: bool,
) -> Result<String, String> {
    let Some(map) = workflow.as_object() else {
        return Err("Workflow JSON must be an object.".to_string());
    };

    let mut candidates = Vec::new();
    let mut preferred = Vec::new();
    for (node_id, node_value) in map.iter() {
        let Some(node_obj) = node_value.as_object() else {
            continue;
        };
        let class_type = node_obj
            .get("class_type")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if class_type != selector.class_type {
            continue;
        }

        if let Some(tag) = selector.tag.as_ref() {
            let node_tag = node_obj
                .get("_meta")
                .and_then(|meta| meta.get("nla_tag"))
                .and_then(|value| value.as_str());
            if node_tag != Some(tag.as_str()) {
                continue;
            }
        }

        let inputs = node_obj.get("inputs").and_then(|value| value.as_object());
        let has_input_key = inputs
            .map(|map| map.contains_key(&selector.input_key))
            .unwrap_or(false);

        if require_input_key && !has_input_key {
            continue;
        }

        let title = node_obj
            .get("_meta")
            .and_then(|meta| meta.get("title"))
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());

        if has_input_key {
            preferred.push((node_id.clone(), title.clone()));
        }
        candidates.push((node_id.clone(), title));
    }

    if candidates.is_empty() {
        return Err(format!(
            "No workflow node matched selector ({})",
            selector_label(selector)
        ));
    }

    if !preferred.is_empty() {
        candidates = preferred;
    }

    if let Some(title) = selector.title.as_ref() {
        let filtered: Vec<(String, Option<String>)> = candidates
            .iter()
            .filter(|(_, node_title)| node_title.as_ref() == Some(title))
            .cloned()
            .collect();
        if !filtered.is_empty() {
            candidates = filtered;
        }
    }

    if candidates.len() > 1 {
        let ids = candidates
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "Multiple workflow nodes matched selector ({}): {}",
            selector_label(selector),
            ids
        ));
    }

    Ok(candidates.pop().map(|(id, _)| id).unwrap_or_default())
}

fn selector_label(selector: &NodeSelector) -> String {
    let mut parts = vec![
        format!("class_type={}", selector.class_type),
        format!("input_key={}", selector.input_key),
    ];
    if let Some(tag) = selector.tag.as_ref() {
        parts.push(format!("tag={}", tag));
    }
    if let Some(title) = selector.title.as_ref() {
        parts.push(format!("title={}", title));
    }
    parts.join(", ")
}

fn set_workflow_input(
    workflow: &mut Value,
    node_id: &str,
    input_key: &str,
    value: Value,
) -> Result<(), String> {
    let Some(node) = workflow.get_mut(node_id) else {
        return Err(format!("Workflow missing node {}", node_id));
    };
    let Some(inputs) = node.get_mut("inputs") else {
        return Err(format!("Workflow node {} missing inputs", node_id));
    };
    let Some(inputs) = inputs.as_object_mut() else {
        return Err(format!("Workflow node {} inputs not an object", node_id));
    };
    inputs.insert(input_key.to_string(), value);
    Ok(())
}

fn apply_binding_transform(
    value: &Value,
    transform: Option<&BindingTransform>,
) -> Result<Value, String> {
    let Some(transform) = transform else {
        return Ok(value.clone());
    };
    let number = input_value_as_f64(value)
        .ok_or_else(|| "Expected numeric input for transform.".to_string())?;
    let adjusted = match transform {
        BindingTransform::Clamp { min, max } => number.clamp(*min, *max),
        BindingTransform::Scale { factor } => number * factor,
    };
    serde_json::Number::from_f64(adjusted)
        .map(Value::Number)
        .ok_or_else(|| "Transformed value is not a valid number.".to_string())
}

fn coerce_manifest_value(value: &Value, input_type: &ProviderInputType) -> Result<Value, String> {
    match input_type {
        ProviderInputType::Text => Ok(Value::String(match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })),
        ProviderInputType::Enum { .. } => Ok(Value::String(match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })),
        ProviderInputType::Number => {
            let number = input_value_as_f64(value).ok_or_else(|| "Expected number.".to_string())?;
            serde_json::Number::from_f64(number)
                .map(Value::Number)
                .ok_or_else(|| "Number is not valid.".to_string())
        }
        ProviderInputType::Integer => {
            let number =
                input_value_as_i64(value).ok_or_else(|| "Expected integer.".to_string())?;
            Ok(Value::Number(number.into()))
        }
        ProviderInputType::Boolean => {
            let flag = input_value_as_bool(value).ok_or_else(|| "Expected boolean.".to_string())?;
            Ok(Value::Bool(flag))
        }
        ProviderInputType::Image | ProviderInputType::Video | ProviderInputType::Audio => {
            Ok(Value::String(match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            }))
        }
    }
}

fn coerce_value(value: &Value, kind: InputCoercion) -> Result<Value, String> {
    match kind {
        InputCoercion::String => Ok(Value::String(match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })),
        InputCoercion::Integer => {
            let number = if let Some(num) = value.as_i64() {
                serde_json::Number::from(num)
            } else if let Some(num) = value.as_u64() {
                serde_json::Number::from(num)
            } else if let Some(num) = value.as_f64() {
                serde_json::Number::from(num.round() as i64)
            } else if let Some(text) = value.as_str() {
                let parsed = text
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| format!("Expected integer, got {}", text))?;
                serde_json::Number::from(parsed)
            } else {
                return Err("Expected integer value".to_string());
            };
            Ok(Value::Number(number))
        }
        InputCoercion::Float => {
            let number = if let Some(num) = value.as_f64() {
                serde_json::Number::from_f64(num)
            } else if let Some(text) = value.as_str() {
                let parsed = text
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| format!("Expected float, got {}", text))?;
                serde_json::Number::from_f64(parsed)
            } else {
                None
            }
            .ok_or_else(|| "Expected float value".to_string())?;
            Ok(Value::Number(number))
        }
    }
}

async fn submit_prompt(
    client: &reqwest::Client,
    base_url: &str,
    workflow: &Value,
) -> Result<String, String> {
    let url = format!("{}/prompt", base_url.trim_end_matches('/'));
    let response = client
        .post(url)
        .json(&serde_json::json!({ "prompt": workflow }))
        .send()
        .await
        .map_err(|err| format!("Failed to submit prompt: {}", err))?;
    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|err| format!("Failed to parse prompt response: {}", err))?;
    if !status.is_success() {
        return Err(format!("ComfyUI rejected prompt ({}): {}", status, payload));
    }
    payload
        .get("prompt_id")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| "ComfyUI response missing prompt_id".to_string())
}

async fn poll_history(
    client: &reqwest::Client,
    base_url: &str,
    prompt_id: &str,
) -> Result<Value, String> {
    let url = format!("{}/history/{}", base_url.trim_end_matches('/'), prompt_id);
    for _ in 0..240 {
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|err| format!("Failed to query history: {}", err))?;
        let payload: Value = response
            .json()
            .await
            .map_err(|err| format!("Failed to parse history: {}", err))?;

        if let Some(outputs) = extract_outputs(&payload, prompt_id) {
            return Ok(outputs.clone());
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Err("Timed out waiting for ComfyUI output.".to_string())
}

fn build_ws_url(base_url: &str, client_id: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let (scheme, rest) = if trimmed.starts_with("https://") {
        ("wss://", &trimmed["https://".len()..])
    } else if trimmed.starts_with("http://") {
        ("ws://", &trimmed["http://".len()..])
    } else if trimmed.starts_with("wss://") || trimmed.starts_with("ws://") {
        ("", trimmed)
    } else {
        ("ws://", trimmed)
    };
    let base = format!("{}{}", scheme, rest);
    format!("{}/ws?clientId={}", base, urlencoding::encode(client_id))
}

async fn listen_progress_ws(
    base_url: &str,
    prompt_id: &str,
    total_nodes: usize,
    progress_tx: tokio::sync::mpsc::UnboundedSender<ComfyUiProgress>,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;

    let client_id = Uuid::new_v4().to_string();
    let ws_url = build_ws_url(base_url, &client_id);
    let (stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|err| format!("WS connect failed: {}", err))?;
    let (_write, mut read) = stream.split();
    let mut last_node = None::<f32>;
    let mut last_overall = None::<f32>;

    while let Some(message) = read.next().await {
        let message = message.map_err(|err| format!("WS read failed: {}", err))?;
        match message {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let message_type = value
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                let Some(data) = value.get("data") else {
                    continue;
                };
                let Some(message_prompt_id) =
                    data.get("prompt_id").and_then(|value| value.as_str())
                else {
                    continue;
                };
                if message_prompt_id != prompt_id {
                    continue;
                }
                if message_type == "progress" {
                    let Some(max) = data.get("max").and_then(json_number_as_f64) else {
                        continue;
                    };
                    if max <= 0.0 {
                        continue;
                    }
                    let Some(value) = data.get("value").and_then(json_number_as_f64) else {
                        continue;
                    };
                    let ratio = (value / max).clamp(0.0, 1.0) as f32;
                    if let Some(last) = last_node {
                        if (ratio - last).abs() < 0.001 {
                            continue;
                        }
                    }
                    if progress_tx.send(ComfyUiProgress::node(ratio)).is_err() {
                        break;
                    }
                    last_node = Some(ratio);
                } else if message_type == "progress_state" {
                    let Some(ratio) = overall_ratio_from_state(data, total_nodes) else {
                        continue;
                    };
                    if let Some(last) = last_overall {
                        if (ratio - last).abs() < 0.001 {
                            continue;
                        }
                    }
                    if progress_tx.send(ComfyUiProgress::overall(ratio)).is_err() {
                        break;
                    }
                    last_overall = Some(ratio);
                }
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

fn json_number_as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
        .or_else(|| value.as_u64().map(|value| value as f64))
}

fn overall_ratio_from_state(data: &Value, total_nodes: usize) -> Option<f32> {
    if total_nodes == 0 {
        return None;
    }
    let nodes = data.get("nodes")?.as_object()?;
    let mut total = 0.0f64;
    for node in nodes.values() {
        let state = node
            .get("state")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match state {
            "finished" => {
                total += 1.0;
            }
            "running" => {
                let max = node.get("max").and_then(json_number_as_f64).unwrap_or(0.0);
                if max > 0.0 {
                    let value = node
                        .get("value")
                        .and_then(json_number_as_f64)
                        .unwrap_or(0.0);
                    total += (value / max).clamp(0.0, 1.0);
                }
            }
            _ => {}
        }
    }
    Some((total / total_nodes as f64).clamp(0.0, 1.0) as f32)
}

fn extract_outputs<'a>(payload: &'a Value, prompt_id: &str) -> Option<&'a Value> {
    if let Some(outputs) = payload.get("outputs") {
        return Some(outputs);
    }
    payload.get(prompt_id)?.get("outputs")
}

struct OutputRef {
    filename: String,
    subfolder: String,
    kind: String,
}

fn find_output_ref(
    outputs: &Value,
    output_node_id: Option<&str>,
    output_key: Option<&str>,
    index: Option<u32>,
    output_type: ProviderOutputType,
) -> Option<OutputRef> {
    if let Some(node_id) = output_node_id {
        if let Some(output) = outputs.get(node_id) {
            if let Some(key) = output_key {
                if let Some(item) = extract_output_ref(output, key, index, Some(output_type)) {
                    return Some(item);
                }
            }
            if let Some(item) = extract_output_ref_any(output, output_type, index) {
                return Some(item);
            }
        }
    } else if let Some(output) = outputs.get(OUTPUT_NODE_ID) {
        if let Some(key) = output_key {
            if let Some(item) = extract_output_ref(output, key, index, Some(output_type)) {
                return Some(item);
            }
        }
        if let Some(item) = extract_output_ref_any(output, output_type, index) {
            return Some(item);
        }
    }

    if let Some(key) = output_key {
        if let Some(item) = find_output_by_key(outputs, key, index, output_type) {
            return Some(item);
        }
    }

    find_output_any(outputs, output_type, index)
}

fn find_output_by_key(
    outputs: &Value,
    output_key: &str,
    index: Option<u32>,
    output_type: ProviderOutputType,
) -> Option<OutputRef> {
    outputs.as_object().and_then(|map| {
        map.values()
            .find_map(|value| extract_output_ref(value, output_key, index, Some(output_type)))
    })
}

fn find_output_any(
    outputs: &Value,
    output_type: ProviderOutputType,
    index: Option<u32>,
) -> Option<OutputRef> {
    outputs.as_object().and_then(|map| {
        map.values()
            .find_map(|value| extract_output_ref_any(value, output_type, index))
    })
}

fn extract_output_ref(
    output: &Value,
    output_key: &str,
    index: Option<u32>,
    output_type: Option<ProviderOutputType>,
) -> Option<OutputRef> {
    let items = output.get(output_key)?.as_array()?;
    let item = extract_output_item(items, index)?;
    if let Some(output_type) = output_type {
        if !output_matches_type(&item.filename, output_type) {
            return None;
        }
    }
    Some(item)
}

fn extract_output_ref_any(
    output: &Value,
    output_type: ProviderOutputType,
    index: Option<u32>,
) -> Option<OutputRef> {
    let output_obj = output.as_object()?;
    for value in output_obj.values() {
        let Some(items) = value.as_array() else {
            continue;
        };
        let Some(item) = extract_output_item(items, index) else {
            continue;
        };
        if output_matches_type(&item.filename, output_type) {
            return Some(item);
        }
    }
    None
}

fn extract_output_item(items: &[Value], index: Option<u32>) -> Option<OutputRef> {
    let idx = index.unwrap_or(0) as usize;
    let item = items.get(idx)?;
    let filename = item.get("filename")?.as_str()?.to_string();
    let subfolder = item
        .get("subfolder")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let kind = item
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("output")
        .to_string();
    Some(OutputRef {
        filename,
        subfolder,
        kind,
    })
}

fn output_matches_type(filename: &str, output_type: ProviderOutputType) -> bool {
    let Some(ext) = output_extension(filename) else {
        return false;
    };
    output_extensions(output_type)
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&ext))
}

fn output_extension(filename: &str) -> Option<String> {
    let ext = Path::new(filename).extension()?.to_str()?.to_string();
    if ext.is_empty() {
        None
    } else {
        Some(ext)
    }
}

fn output_type_label(output_type: ProviderOutputType) -> &'static str {
    match output_type {
        ProviderOutputType::Image => "image",
        ProviderOutputType::Video => "video",
        ProviderOutputType::Audio => "audio",
    }
}

fn default_extension_for_output(output_type: ProviderOutputType) -> &'static str {
    match output_type {
        ProviderOutputType::Image => "png",
        ProviderOutputType::Video => "mp4",
        ProviderOutputType::Audio => "wav",
    }
}

fn output_extensions(output_type: ProviderOutputType) -> &'static [&'static str] {
    match output_type {
        ProviderOutputType::Image => &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"],
        ProviderOutputType::Video => &["mp4", "mov", "mkv", "webm", "avi", "m4v", "gif"],
        ProviderOutputType::Audio => &["wav", "mp3", "flac", "ogg", "aac", "m4a"],
    }
}

async fn download_output(
    client: &reqwest::Client,
    base_url: &str,
    output: &OutputRef,
) -> Result<Vec<u8>, String> {
    let url = format!(
        "{}/view?filename={}&subfolder={}&type={}",
        base_url.trim_end_matches('/'),
        urlencoding::encode(&output.filename),
        urlencoding::encode(&output.subfolder),
        urlencoding::encode(&output.kind),
    );
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("Failed to download output: {}", err))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("ComfyUI output download failed: {}", status));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|err| format!("Failed to read output bytes: {}", err))
}
