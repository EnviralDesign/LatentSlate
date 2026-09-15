//! Provider storage helpers for provider configs.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::state::{
    InputRole, InputUi, ProviderConnection, ProviderEntry, ProviderInputField, ProviderInputType,
    ProviderOutputType, ProviderWorkflowKind,
};

pub fn load_local_provider_entries_with_reports() -> io::Result<(
    Vec<ProviderEntry>,
    Vec<crate::providers::latentslate_engine::EngineCatalogLoadReport>,
)> {
    let connections = crate::providers::latentslate_engine::load_connections();
    load_local_provider_entries_for_connections_with_reports(&connections)
}

pub fn load_local_provider_entries_for_connections_with_reports(
    connections: &[crate::providers::latentslate_engine::EngineConnectionSettings],
) -> io::Result<(
    Vec<ProviderEntry>,
    Vec<crate::providers::latentslate_engine::EngineCatalogLoadReport>,
)> {
    let mut entries = load_provider_entries_from(&local_providers_root())?;
    let (engine_entries, reports) =
        crate::providers::latentslate_engine::load_provider_entries_for_connections_with_reports(
            connections,
        );
    merge_provider_entries(&mut entries, engine_entries);
    Ok((entries, reports))
}

fn merge_provider_entries(entries: &mut Vec<ProviderEntry>, incoming: Vec<ProviderEntry>) {
    for provider in incoming {
        if let Some(index) = entries.iter().position(|entry| entry.id == provider.id) {
            entries[index] = provider;
        } else {
            entries.push(provider);
        }
    }
}

pub fn save_local_provider_entry(entry: &ProviderEntry) -> io::Result<PathBuf> {
    save_provider_entry_to(&local_providers_root(), entry)
}

pub fn local_providers_root() -> PathBuf {
    crate::core::paths::app_runtime_root().join("providers")
}

pub fn list_local_provider_files() -> Vec<PathBuf> {
    let root = local_providers_root();
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
    local_providers_root().join(format!("{}.json", entry.id))
}

pub fn default_provider_entry() -> ProviderEntry {
    let mut entry = ProviderEntry::new(
        "New Provider",
        ProviderOutputType::Image,
        ProviderConnection::ComfyUi {
            base_url: "http://127.0.0.1:8188".to_string(),
            workflow_path: None,
            manifest: None,
        },
    );
    entry.inputs = Vec::new();
    entry
}

pub fn default_openai_image_provider_entry() -> ProviderEntry {
    let mut entry = ProviderEntry::new(
        "OpenAI Image 2.5 Text to Image",
        ProviderOutputType::Image,
        ProviderConnection::OpenAiImage {
            api_key: None,
            model: "gpt-image-2.5-flare".to_string(),
            base_url: None,
        },
    );
    entry.description =
        Some("Cloud text-to-image provider for generating still assets from prompts.".to_string());
    entry.workflow_kind = ProviderWorkflowKind::TextToImage;
    entry.inputs = vec![
        enum_input(
            "model",
            "Model",
            &["gpt-image-2.5-flare", "gpt-image-2.5-sunburst"],
            Some("gpt-image-2.5-flare"),
        ),
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
            &["auto", "low", "medium", "high", "xhigh", "max"],
            Some("auto"),
        ),
        enum_input(
            "background",
            "Background",
            &["auto", "opaque", "transparent"],
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

pub fn default_openai_image_edit_provider_entry() -> ProviderEntry {
    let mut entry = default_openai_image_provider_entry();
    entry.name = "OpenAI Image 2.5 Image to Image".to_string();
    entry.description =
        Some("Edit a reference image with GPT Image 2.5 Sunburst or Flare.".to_string());
    entry.workflow_kind = ProviderWorkflowKind::ImageToImage;
    if let ProviderConnection::OpenAiImage { model, .. } = &mut entry.connection {
        *model = "gpt-image-2.5-sunburst".to_string();
    }
    for field in &mut entry.inputs {
        if field.name == "model" {
            field.default = Some(serde_json::json!("gpt-image-2.5-sunburst"));
        }
    }
    entry.inputs.insert(
        1,
        ProviderInputField {
            name: "image".to_string(),
            label: "Reference image".to_string(),
            description: Some("Image to edit or use as a reference.".to_string()),
            input_type: ProviderInputType::Image,
            required: true,
            default: None,
            role: None,
            ui: None,
            ordered_collection: false,
            image_dimensions: None,
        },
    );
    entry
}

pub fn default_xai_image_provider_entry() -> ProviderEntry {
    let mut entry = ProviderEntry::new(
        "xAI Imagine Image",
        ProviderOutputType::Image,
        ProviderConnection::XaiImage {
            api_key: None,
            model: "grok-imagine-image-quality".to_string(),
            base_url: None,
        },
    );
    entry.description =
        Some("Cloud text-to-image provider for generating still assets from prompts.".to_string());
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
            api_key: None,
            model: "grok-imagine-video".to_string(),
            base_url: None,
        },
    );
    entry.description = Some(
        "Cloud text-to-video provider for generating short video assets from prompts.".to_string(),
    );
    entry.workflow_kind = ProviderWorkflowKind::TextToVideo;
    let mut duration = integer_input("duration", "Duration Seconds", 6, Some(1.0), Some(15.0));
    duration.role = Some(InputRole::DurationSeconds);
    entry.inputs = vec![
        text_input(
            "prompt",
            "Prompt",
            Some("Describe the video to generate.".to_string()),
            None,
            true,
        ),
        duration,
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
        ordered_collection: false,
        image_dimensions: None,
        name: name.to_string(),
        label: label.to_string(),
        description: placeholder.clone(),
        input_type: ProviderInputType::Text,
        required,
        default: default.map(serde_json::Value::String),
        role: None,
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
        ordered_collection: false,
        image_dimensions: None,
        name: name.to_string(),
        label: label.to_string(),
        description: None,
        input_type: ProviderInputType::Enum {
            options: options.iter().map(|value| value.to_string()).collect(),
        },
        required: true,
        default: default.map(|value| serde_json::Value::String(value.to_string())),
        role: None,
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
        ordered_collection: false,
        image_dimensions: None,
        name: name.to_string(),
        label: label.to_string(),
        description: None,
        input_type: ProviderInputType::Integer,
        required: true,
        default: Some(serde_json::Value::Number(default.into())),
        role: None,
        ui: Some(InputUi {
            min,
            max,
            step: Some(1.0),
            ..InputUi::default()
        }),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_subdirectory_does_not_change_generation_loading() {
        let generation = default_openai_image_provider_entry();
        let root = std::env::temp_dir().join(format!("latentslate-isolation-{}", generation.id));
        let path = save_provider_entry_to(&root, &generation).unwrap();
        let before = fs::read(&path).unwrap();
        let agent = crate::state::AgentProviderEntry::default();
        fs::create_dir(root.join("agents")).unwrap();
        fs::write(
            root.join("agents").join(format!("{}.json", agent.id)),
            serde_json::to_vec(&agent).unwrap(),
        )
        .unwrap();
        let loaded = load_provider_entries_from(&root).unwrap();
        assert_eq!(loaded, vec![generation]);
        assert_eq!(fs::read(path).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cloud_templates_mint_a_fresh_id_on_every_add() {
        let first = default_openai_image_provider_entry();
        let second = default_openai_image_provider_entry();
        assert_ne!(first.id, second.id);
        assert_eq!(first.name, second.name);

        let xai_image = default_xai_image_provider_entry();
        let xai_video = default_xai_video_provider_entry();
        assert_ne!(xai_image.id, default_xai_image_provider_entry().id);
        assert_ne!(xai_video.id, default_xai_video_provider_entry().id);
    }

    #[test]
    fn merge_keeps_same_display_name_when_ids_differ() {
        let first = default_openai_image_provider_entry();
        let second = default_openai_image_provider_entry();
        let mut entries = vec![first.clone()];
        merge_provider_entries(&mut entries, vec![second.clone()]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, first.id);
        assert_eq!(entries[1].id, second.id);
        assert_eq!(entries[0].name, entries[1].name);
    }

    #[test]
    fn save_writes_one_file_per_id_not_per_name() {
        let first = default_openai_image_provider_entry();
        let second = default_openai_image_provider_entry();
        let root = std::env::temp_dir().join(format!("latentslate-provider-id-{}", first.id));
        let first_path = save_provider_entry_to(&root, &first).expect("save first");
        let second_path = save_provider_entry_to(&root, &second).expect("save second");
        assert_ne!(first_path, second_path);
        let expected_name = format!("{}.json", first.id);
        assert_eq!(
            first_path.file_name().and_then(|name| name.to_str()),
            Some(expected_name.as_str())
        );
        let loaded = load_provider_entries_from(&root).expect("load");
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|entry| entry.id == first.id));
        assert!(loaded.iter().any(|entry| entry.id == second.id));
    }
}
