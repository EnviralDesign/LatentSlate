#![allow(dead_code)]
//! Provider storage helpers for provider configs.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::core::credentials::{OPENAI_CREDENTIAL_ID, XAI_CREDENTIAL_ID};
use crate::state::{
    InputUi, ProviderConnection, ProviderEntry, ProviderInputField, ProviderInputType,
    ProviderOutputType, ProviderWorkflowKind,
};

pub fn load_provider_entries(project_root: &Path) -> io::Result<Vec<ProviderEntry>> {
    load_provider_entries_from(&providers_root(project_root))
}

pub fn load_global_provider_entries() -> io::Result<Vec<ProviderEntry>> {
    load_provider_entries_from(&global_providers_root())
}

pub fn load_global_provider_entries_or_empty() -> Vec<ProviderEntry> {
    match load_global_provider_entries() {
        Ok(entries) => entries,
        Err(err) => {
            println!("Failed to load provider entries: {}", err);
            Vec::new()
        }
    }
}

pub fn save_provider_entry(project_root: &Path, entry: &ProviderEntry) -> io::Result<PathBuf> {
    save_provider_entry_to(&providers_root(project_root), entry)
}

pub fn save_global_provider_entry(entry: &ProviderEntry) -> io::Result<PathBuf> {
    save_provider_entry_to(&global_providers_root(), entry)
}

pub fn global_providers_root() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("NLA-AI-VideoCreator").join("providers")
}

pub fn list_global_provider_files() -> Vec<PathBuf> {
    let root = global_providers_root();
    let mut files = Vec::new();
    let read_dir = match fs::read_dir(&root) {
        Ok(read_dir) => read_dir,
        Err(_) => return files,
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if is_json_file(&path) {
            files.push(path);
        }
    }
    files.sort();
    files
}

pub fn read_provider_file(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

pub fn write_provider_file(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    Ok(())
}

pub fn provider_path_for_entry(entry: &ProviderEntry) -> PathBuf {
    global_providers_root().join(format!("{}.json", entry.id))
}

pub fn default_provider_entry() -> ProviderEntry {
    let mut entry = ProviderEntry::new(
        "New Provider",
        ProviderOutputType::Image,
        ProviderConnection::ComfyUi {
            base_url: "http://127.0.0.1:8188".to_string(),
            workflow_path: Some("workflows/sdxl_simple_example_API.json".to_string()),
            manifest_path: None,
        },
    );
    entry.inputs = Vec::new();
    entry
}

pub fn default_openai_image_provider_entry() -> ProviderEntry {
    let mut entry = ProviderEntry::new(
        "OpenAI Image",
        ProviderOutputType::Image,
        ProviderConnection::OpenAiImage {
            credential_id: OPENAI_CREDENTIAL_ID.to_string(),
            model: "gpt-image-2".to_string(),
            base_url: None,
        },
    );
    entry.workflow_kind = ProviderWorkflowKind::TextToImage;
    entry.inputs = vec![
        text_input(
            "prompt",
            "Prompt",
            Some("Describe the image to generate.".to_string()),
            None,
            true,
        ),
        enum_input(
            "size",
            "Size",
            &["1024x1024", "1536x1024", "1024x1536", "auto"],
            Some("1024x1024"),
        ),
        enum_input(
            "quality",
            "Quality",
            &["auto", "low", "medium", "high"],
            Some("auto"),
        ),
        enum_input(
            "output_format",
            "Output Format",
            &["png", "jpeg", "webp"],
            Some("png"),
        ),
    ];
    entry
}

pub fn default_xai_image_provider_entry() -> ProviderEntry {
    let mut entry = ProviderEntry::new(
        "xAI Imagine Image",
        ProviderOutputType::Image,
        ProviderConnection::XaiImage {
            credential_id: XAI_CREDENTIAL_ID.to_string(),
            model: "grok-imagine-image-quality".to_string(),
            base_url: None,
        },
    );
    entry.workflow_kind = ProviderWorkflowKind::TextToImage;
    entry.inputs = vec![
        text_input(
            "prompt",
            "Prompt",
            Some("Describe the image to generate.".to_string()),
            None,
            true,
        ),
        enum_input(
            "aspect_ratio",
            "Aspect Ratio",
            &["1:1", "16:9", "9:16", "4:3", "3:4"],
            Some("1:1"),
        ),
        enum_input("resolution", "Resolution", &["1k", "2k"], Some("1k")),
    ];
    entry
}

pub fn default_xai_video_provider_entry() -> ProviderEntry {
    let mut entry = ProviderEntry::new(
        "xAI Grok Video",
        ProviderOutputType::Video,
        ProviderConnection::XaiVideo {
            credential_id: XAI_CREDENTIAL_ID.to_string(),
            model: "grok-imagine-video".to_string(),
            base_url: None,
        },
    );
    entry.workflow_kind = ProviderWorkflowKind::TextToVideo;
    entry.inputs = vec![
        text_input(
            "prompt",
            "Prompt",
            Some("Describe the video to generate.".to_string()),
            None,
            true,
        ),
        integer_input("duration", "Duration Seconds", 6, Some(1.0), Some(15.0)),
        enum_input(
            "aspect_ratio",
            "Aspect Ratio",
            &["16:9", "9:16", "1:1", "4:3", "3:4", "3:2", "2:3"],
            Some("16:9"),
        ),
        enum_input("resolution", "Resolution", &["480p", "720p"], Some("480p")),
    ];
    entry
}

fn text_input(
    name: &str,
    label: &str,
    placeholder: Option<String>,
    default: Option<String>,
    required: bool,
) -> ProviderInputField {
    ProviderInputField {
        name: name.to_string(),
        label: label.to_string(),
        input_type: ProviderInputType::Text,
        required,
        default: default.map(serde_json::Value::String),
        ui: Some(InputUi {
            placeholder,
            multiline: true,
            ..InputUi::default()
        }),
    }
}

fn enum_input(
    name: &str,
    label: &str,
    options: &[&str],
    default: Option<&str>,
) -> ProviderInputField {
    ProviderInputField {
        name: name.to_string(),
        label: label.to_string(),
        input_type: ProviderInputType::Enum {
            options: options.iter().map(|value| value.to_string()).collect(),
        },
        required: true,
        default: default.map(|value| serde_json::Value::String(value.to_string())),
        ui: None,
    }
}

fn integer_input(
    name: &str,
    label: &str,
    default: i64,
    min: Option<f64>,
    max: Option<f64>,
) -> ProviderInputField {
    ProviderInputField {
        name: name.to_string(),
        label: label.to_string(),
        input_type: ProviderInputType::Integer,
        required: true,
        default: Some(serde_json::Value::Number(default.into())),
        ui: Some(InputUi {
            min,
            max,
            step: Some(1.0),
            ..InputUi::default()
        }),
    }
}

fn providers_root(project_root: &Path) -> PathBuf {
    project_root.join(".providers")
}

fn load_provider_entries_from(root: &Path) -> io::Result<Vec<ProviderEntry>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                println!("Failed to read provider entry: {}", err);
                continue;
            }
        };
        let path = entry.path();
        if !is_json_file(&path) {
            continue;
        }
        let json = match fs::read_to_string(&path) {
            Ok(json) => json,
            Err(err) => {
                println!("Failed to read provider config {:?}: {}", path, err);
                continue;
            }
        };
        let provider: ProviderEntry = match serde_json::from_str(&json) {
            Ok(provider) => provider,
            Err(err) => {
                println!("Failed to parse provider config {:?}: {}", path, err);
                continue;
            }
        };
        entries.push(provider);
    }

    Ok(entries)
}

fn save_provider_entry_to(root: &Path, entry: &ProviderEntry) -> io::Result<PathBuf> {
    fs::create_dir_all(root)?;
    let path = root.join(format!("{}.json", entry.id));
    let json = serde_json::to_string_pretty(entry)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    fs::write(&path, json)?;
    Ok(path)
}

fn is_json_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("json"))
        .unwrap_or(false)
}
