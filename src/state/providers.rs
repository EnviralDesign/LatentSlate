#![allow(dead_code)]
//! Provider configuration data model.
//!
//! Providers describe external generation backends (ComfyUI, APIs, etc.).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The output media type produced by a provider entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderOutputType {
    Image,
    Video,
    Audio,
}

/// High-level generation workflow shape for UX filtering and creation menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderWorkflowKind {
    /// Infer from provider output type and exposed media inputs.
    Auto,
    TextToImage,
    ImageToImage,
    TextToVideo,
    ImageToVideo,
    FirstFrameLastFrameVideo,
    VideoToVideo,
    TextToAudio,
    AudioToAudio,
    Custom,
}

impl Default for ProviderWorkflowKind {
    fn default() -> Self {
        Self::Auto
    }
}

impl ProviderWorkflowKind {
    pub const ALL: [ProviderWorkflowKind; 10] = [
        ProviderWorkflowKind::Auto,
        ProviderWorkflowKind::TextToImage,
        ProviderWorkflowKind::ImageToImage,
        ProviderWorkflowKind::TextToVideo,
        ProviderWorkflowKind::ImageToVideo,
        ProviderWorkflowKind::FirstFrameLastFrameVideo,
        ProviderWorkflowKind::VideoToVideo,
        ProviderWorkflowKind::TextToAudio,
        ProviderWorkflowKind::AudioToAudio,
        ProviderWorkflowKind::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::TextToImage => "Text to Image",
            Self::ImageToImage => "Image to Image",
            Self::TextToVideo => "Text to Video",
            Self::ImageToVideo => "Image to Video",
            Self::FirstFrameLastFrameVideo => "First/Last Frame Video",
            Self::VideoToVideo => "Video to Video",
            Self::TextToAudio => "Text to Audio",
            Self::AudioToAudio => "Audio to Audio",
            Self::Custom => "Custom",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::TextToImage => "T2I",
            Self::ImageToImage => "I2I",
            Self::TextToVideo => "T2V",
            Self::ImageToVideo => "I2V",
            Self::FirstFrameLastFrameVideo => "FF2LF",
            Self::VideoToVideo => "V2V",
            Self::TextToAudio => "T2A",
            Self::AudioToAudio => "A2A",
            Self::Custom => "Custom",
        }
    }
}

/// Optional app-level behavior layered on top of a provider's workflow shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPurpose {
    Generic,
    TimelineBridge,
}

impl Default for ProviderPurpose {
    fn default() -> Self {
        Self::Generic
    }
}

impl ProviderPurpose {
    pub fn label(self) -> &'static str {
        match self {
            Self::Generic => "Normal",
            Self::TimelineBridge => "Timeline Bridge",
        }
    }

    pub fn is_generic(value: &Self) -> bool {
        *value == Self::Generic
    }
}

/// Timing constraints for purpose-built timeline bridge providers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineBridgeSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_visible_frames: Option<u32>,
}

impl Default for TimelineBridgeSettings {
    fn default() -> Self {
        Self {
            max_visible_frames: Some(DEFAULT_TIMELINE_BRIDGE_MAX_VISIBLE_FRAMES),
        }
    }
}

pub const DEFAULT_TIMELINE_BRIDGE_MAX_VISIBLE_FRAMES: u32 = 80;

/// Input types supported by provider schemas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderInputType {
    Image,
    Video,
    Audio,
    Text,
    Number,
    Integer,
    Boolean,
    Enum { options: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputRole {
    Width,
    Height,
    Seed,
    DurationSeconds,
    Fps,
    FrameCount,
    LeftVideo,
    RightVideo,
    LeftReplaceFrames,
    RightReplaceFrames,
    EdgeBlendFrames,
}

/// Schema field describing a single provider input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderInputField {
    pub name: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_type: ProviderInputType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<InputRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<InputUi>,
}

/// Connection configuration for a provider entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderConnection {
    ComfyUi {
        base_url: String,
        #[serde(default)]
        workflow_path: Option<String>,
        #[serde(default)]
        manifest: Option<ProviderManifest>,
    },
    OpenAiImage {
        #[serde(default)]
        api_key: Option<String>,
        model: String,
        #[serde(default)]
        base_url: Option<String>,
    },
    XaiImage {
        #[serde(default)]
        api_key: Option<String>,
        model: String,
        #[serde(default)]
        base_url: Option<String>,
    },
    XaiVideo {
        #[serde(default)]
        api_key: Option<String>,
        model: String,
        #[serde(default)]
        base_url: Option<String>,
    },
    CustomHttp {
        base_url: String,
        api_key: Option<String>,
    },
}

/// A configured provider entry stored on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub output_type: ProviderOutputType,
    #[serde(default)]
    pub workflow_kind: ProviderWorkflowKind,
    #[serde(default, skip_serializing_if = "ProviderPurpose::is_generic")]
    pub purpose: ProviderPurpose,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_bridge: Option<TimelineBridgeSettings>,
    #[serde(default)]
    pub inputs: Vec<ProviderInputField>,
    pub connection: ProviderConnection,
}

impl ProviderEntry {
    pub fn new(
        name: impl Into<String>,
        output_type: ProviderOutputType,
        connection: ProviderConnection,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            output_type,
            workflow_kind: ProviderWorkflowKind::Auto,
            purpose: ProviderPurpose::Generic,
            timeline_bridge: None,
            inputs: Vec::new(),
            connection,
        }
    }

    pub fn resolved_workflow_kind(&self) -> ProviderWorkflowKind {
        match self.workflow_kind {
            ProviderWorkflowKind::Auto => infer_workflow_kind(self.output_type, &self.inputs),
            explicit => explicit,
        }
    }
}

fn infer_workflow_kind(
    output_type: ProviderOutputType,
    inputs: &[ProviderInputField],
) -> ProviderWorkflowKind {
    let image_inputs: Vec<&ProviderInputField> = inputs
        .iter()
        .filter(|input| matches!(input.input_type, ProviderInputType::Image))
        .collect();
    let video_input_count = inputs
        .iter()
        .filter(|input| matches!(input.input_type, ProviderInputType::Video))
        .count();
    let audio_input_count = inputs
        .iter()
        .filter(|input| matches!(input.input_type, ProviderInputType::Audio))
        .count();

    match output_type {
        ProviderOutputType::Image => {
            if image_inputs.is_empty() {
                ProviderWorkflowKind::TextToImage
            } else {
                ProviderWorkflowKind::ImageToImage
            }
        }
        ProviderOutputType::Video => {
            let has_start = image_inputs
                .iter()
                .any(|input| provider_input_reference_slot(input).starts_with("start"));
            let has_end = image_inputs
                .iter()
                .any(|input| provider_input_reference_slot(input).starts_with("end"));
            if has_start && has_end {
                ProviderWorkflowKind::FirstFrameLastFrameVideo
            } else if !image_inputs.is_empty() {
                ProviderWorkflowKind::ImageToVideo
            } else if video_input_count > 0 {
                ProviderWorkflowKind::VideoToVideo
            } else {
                ProviderWorkflowKind::TextToVideo
            }
        }
        ProviderOutputType::Audio => {
            if audio_input_count > 0 {
                ProviderWorkflowKind::AudioToAudio
            } else {
                ProviderWorkflowKind::TextToAudio
            }
        }
    }
}

fn provider_input_reference_slot(input: &ProviderInputField) -> &'static str {
    let key = format!("{} {}", input.name, input.label).to_ascii_lowercase();
    if contains_any(&key, &["end", "last", "final"]) {
        "end_image"
    } else if contains_any(&key, &["start", "first", "initial", "init"]) {
        "start_image"
    } else {
        "image"
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

pub fn input_value_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

pub fn input_value_as_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().map(|v| v as i64))
        .or_else(|| value.as_f64().map(|v| v.round() as i64))
}

pub fn input_value_as_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|v| v as f64))
        .or_else(|| value.as_u64().map(|v| v as f64))
}

pub fn input_value_as_bool(value: &serde_json::Value) -> Option<bool> {
    match value {
        serde_json::Value::Bool(flag) => Some(*flag),
        serde_json::Value::String(text) => text.parse::<bool>().ok(),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "adapter_type", rename_all = "snake_case")]
pub enum ProviderManifest {
    ComfyUi {
        schema_version: u32,
        #[serde(default)]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        output_type: ProviderOutputType,
        #[serde(default, skip_serializing_if = "ProviderPurpose::is_generic")]
        purpose: ProviderPurpose,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeline_bridge: Option<TimelineBridgeSettings>,
        workflow: ComfyWorkflowRef,
        #[serde(default)]
        inputs: Vec<ManifestInput>,
        output: ComfyOutputSelector,
    },
    CustomHttp {
        schema_version: u32,
        #[serde(default)]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        output_type: ProviderOutputType,
        #[serde(default, skip_serializing_if = "ProviderPurpose::is_generic")]
        purpose: ProviderPurpose,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeline_bridge: Option<TimelineBridgeSettings>,
        workflow: CustomHttpWorkflow,
        #[serde(default)]
        inputs: Vec<CustomHttpInput>,
        output: CustomHttpOutput,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComfyWorkflowRef {
    pub workflow_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestInput {
    pub name: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_type: ProviderInputType,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<InputRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<InputUi>,
    pub bind: InputBinding,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputUi {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub multiline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default)]
    pub advanced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

impl Default for InputUi {
    fn default() -> Self {
        Self {
            min: None,
            max: None,
            step: None,
            placeholder: None,
            multiline: false,
            group: None,
            advanced: false,
            unit: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputBinding {
    pub selector: NodeSelector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<BindingTransform>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeSelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub class_type: String,
    pub input_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComfyOutputSelector {
    pub selector: NodeSelector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BindingTransform {
    Clamp { min: f64, max: f64 },
    Scale { factor: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomHttpWorkflow {
    pub base_url: String,
    pub path: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomHttpInput {
    pub name: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_type: ProviderInputType,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<InputRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<InputUi>,
    pub bind: CustomHttpBinding,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomHttpBinding {
    pub json_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomHttpOutput {
    #[serde(default)]
    pub download: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_descriptions_are_optional_for_legacy_json() {
        let json = r#"{
            "id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "name": "Legacy Provider",
            "output_type": "image",
            "workflow_kind": "text_to_image",
            "inputs": [
                {
                    "name": "prompt",
                    "label": "Prompt",
                    "input_type": { "type": "text" },
                    "required": true
                }
            ],
            "connection": {
                "type": "comfy_ui",
                "base_url": "http://127.0.0.1:8188"
            }
        }"#;

        let provider: ProviderEntry = serde_json::from_str(json).expect("legacy provider");
        assert_eq!(provider.description, None);
        assert_eq!(provider.inputs[0].description, None);
    }

    #[test]
    fn provider_descriptions_round_trip_through_json() {
        let mut provider = ProviderEntry::new(
            "Described Provider",
            ProviderOutputType::Image,
            ProviderConnection::ComfyUi {
                base_url: "http://127.0.0.1:8188".to_string(),
                workflow_path: None,
                manifest: None,
            },
        );
        provider.description = Some("Use this for still keyframes.".to_string());
        provider.inputs.push(ProviderInputField {
            name: "prompt".to_string(),
            label: "Prompt".to_string(),
            description: Some("Describe the image content.".to_string()),
            input_type: ProviderInputType::Text,
            required: true,
            default: None,
            role: None,
            ui: None,
        });

        let json = serde_json::to_string(&provider).expect("serialize provider");
        assert!(json.contains("Use this for still keyframes."));
        assert!(json.contains("Describe the image content."));

        let parsed: ProviderEntry = serde_json::from_str(&json).expect("parse provider");
        assert_eq!(parsed.description, provider.description);
        assert_eq!(parsed.inputs[0].description, provider.inputs[0].description);
    }

    #[test]
    fn manifest_input_descriptions_round_trip_through_json() {
        let manifest = ProviderManifest::ComfyUi {
            schema_version: 1,
            name: Some("Manifest Provider".to_string()),
            description: Some("Manifest-level guidance.".to_string()),
            output_type: ProviderOutputType::Image,
            purpose: ProviderPurpose::Generic,
            timeline_bridge: None,
            workflow: ComfyWorkflowRef {
                workflow_path: "workflow.json".to_string(),
                workflow_hash: None,
            },
            inputs: vec![ManifestInput {
                name: "prompt".to_string(),
                label: "Prompt".to_string(),
                description: Some("Positive prompt text.".to_string()),
                input_type: ProviderInputType::Text,
                required: true,
                default: None,
                role: None,
                ui: None,
                bind: InputBinding {
                    selector: NodeSelector {
                        node_id: Some("6".to_string()),
                        tag: None,
                        class_type: "CLIPTextEncode".to_string(),
                        input_key: "text".to_string(),
                        title: None,
                    },
                    transform: None,
                },
            }],
            output: ComfyOutputSelector {
                selector: NodeSelector {
                    node_id: Some("9".to_string()),
                    tag: None,
                    class_type: "SaveImage".to_string(),
                    input_key: "images".to_string(),
                    title: None,
                },
                index: None,
            },
        };

        let json = serde_json::to_string(&manifest).expect("serialize manifest");
        assert!(json.contains("Manifest-level guidance."));
        assert!(json.contains("Positive prompt text."));

        let parsed: ProviderManifest = serde_json::from_str(&json).expect("parse manifest");
        match parsed {
            ProviderManifest::ComfyUi {
                description,
                inputs,
                ..
            } => {
                assert_eq!(description.as_deref(), Some("Manifest-level guidance."));
                assert_eq!(
                    inputs[0].description.as_deref(),
                    Some("Positive prompt text.")
                );
            }
            ProviderManifest::CustomHttp { .. } => panic!("expected comfy manifest"),
        }
    }
}
