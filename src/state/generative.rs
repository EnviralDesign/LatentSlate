#![allow(dead_code)]
//! Generative asset config model and persistence helpers.

use crate::state::{Asset, AssetKind, Project, ProviderEntry, ProviderOutputType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Optional frame extraction mode for media references that use a video clip as
/// an image provider input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFrameReference {
    First,
    Last,
}

impl SourceFrameReference {
    pub fn label(self) -> &'static str {
        match self {
            Self::First => "first frame",
            Self::Last => "last frame",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Last => "last",
        }
    }
}

/// Input value bound to a provider field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputValue {
    AssetRef {
        asset_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_clip_id: Option<Uuid>,
        #[serde(default = "default_asset_ref_pinned")]
        pinned: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame_reference: Option<SourceFrameReference>,
    },
    GenerationRef {
        asset_id: Uuid,
        version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame_reference: Option<SourceFrameReference>,
    },
    Literal {
        value: serde_json::Value,
    },
}

fn default_asset_ref_pinned() -> bool {
    true
}

/// Strategy for adjusting seeds across batch generations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedStrategy {
    Keep,
    Increment,
    Random,
}

impl Default for SeedStrategy {
    fn default() -> Self {
        SeedStrategy::Increment
    }
}

impl SeedStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            SeedStrategy::Keep => "keep",
            SeedStrategy::Increment => "increment",
            SeedStrategy::Random => "random",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "keep" => SeedStrategy::Keep,
            "random" => SeedStrategy::Random,
            _ => SeedStrategy::Increment,
        }
    }
}

/// Batch generation settings stored per generative asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchSettings {
    #[serde(default = "default_batch_count")]
    pub count: u32,
    #[serde(default)]
    pub seed_strategy: SeedStrategy,
    #[serde(default)]
    pub seed_field: Option<String>,
}

impl Default for BatchSettings {
    fn default() -> Self {
        Self {
            count: default_batch_count(),
            seed_strategy: SeedStrategy::default(),
            seed_field: None,
        }
    }
}

fn default_batch_count() -> u32 {
    1
}

/// A single generation record for a generative asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationRecord {
    pub version: String,
    pub timestamp: DateTime<Utc>,
    pub provider_id: Uuid,
    pub inputs_snapshot: HashMap<String, InputValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lab_node_id: Option<Uuid>,
}

/// A provider node inside a generative asset's Asset Lab graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetLabNode {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_node_id: Option<Uuid>,
    #[serde(default)]
    pub provider_id: Option<Uuid>,
    #[serde(default)]
    pub inputs: HashMap<String, InputValue>,
    #[serde(default)]
    pub output_version: Option<String>,
}

impl AssetLabNode {
    pub fn new(provider_id: Option<Uuid>) -> Self {
        Self {
            id: Uuid::new_v4(),
            parent_node_id: None,
            provider_id,
            inputs: HashMap::new(),
            output_version: None,
        }
    }

    pub fn new_with_parent(provider_id: Option<Uuid>, parent_node_id: Option<Uuid>) -> Self {
        Self {
            parent_node_id,
            ..Self::new(provider_id)
        }
    }
}

impl GenerativeConfig {
    pub fn normalize_lab_graph_lineage(&mut self) {
        let existing_ids: HashSet<Uuid> = self.lab_graph.nodes.iter().map(|node| node.id).collect();
        let outputs_by_version: HashMap<String, Uuid> = self
            .lab_graph
            .nodes
            .iter()
            .filter_map(|node| {
                node.output_version
                    .as_ref()
                    .map(|version| (version.clone(), node.id))
            })
            .collect();

        for node in self.lab_graph.nodes.iter_mut() {
            if node
                .parent_node_id
                .is_some_and(|parent_id| !existing_ids.contains(&parent_id))
            {
                node.parent_node_id = None;
            }
            if node.parent_node_id.is_some() {
                continue;
            }

            let mut ordered_inputs: Vec<(&String, &InputValue)> = node.inputs.iter().collect();
            ordered_inputs.sort_by(|(left, _), (right, _)| left.cmp(right));
            let inferred_parent = ordered_inputs
                .into_iter()
                .find_map(|(_, input)| match input {
                    InputValue::GenerationRef { version, .. } => {
                        outputs_by_version.get(version.as_str()).copied()
                    }
                    _ => None,
                });

            if inferred_parent.is_some() {
                node.parent_node_id = inferred_parent;
            }
        }
    }

    pub fn root_nodes(&self) -> Vec<&AssetLabNode> {
        self.lab_graph
            .nodes
            .iter()
            .filter(|node| {
                node.parent_node_id.is_none_or(|parent_id| {
                    !self
                        .lab_graph
                        .nodes
                        .iter()
                        .any(|candidate| candidate.id == parent_id)
                })
            })
            .collect()
    }

    pub fn children_of(&self, node_id: Uuid) -> Vec<&AssetLabNode> {
        self.lab_graph
            .nodes
            .iter()
            .filter(|node| node.parent_node_id == Some(node_id))
            .collect()
    }

    pub fn lineage_depth(&self, node_id: Uuid) -> usize {
        let mut depth = 0usize;
        let mut current_id = Some(node_id);
        let mut visited = HashSet::new();
        while let Some(id) = current_id {
            if !visited.insert(id) {
                break;
            }
            let Some(node) = self.lab_graph.nodes.iter().find(|node| node.id == id) else {
                break;
            };
            current_id = node.parent_node_id.filter(|parent_id| {
                self.lab_graph
                    .nodes
                    .iter()
                    .any(|candidate| candidate.id == *parent_id)
            });
            if current_id.is_some() {
                depth += 1;
            }
        }
        depth
    }

    pub fn resolve_primary_media_version<'a>(&'a self, node: &'a AssetLabNode) -> Option<&'a str> {
        node.parent_node_id.and_then(|parent_id| {
            self.lab_graph
                .nodes
                .iter()
                .find(|candidate| candidate.id == parent_id)
                .and_then(|parent| parent.output_version.as_deref())
        })
    }
}

/// Persistent node graph state for internal asset iteration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AssetLabGraph {
    #[serde(default)]
    pub nodes: Vec<AssetLabNode>,
    #[serde(default)]
    pub selected_node_id: Option<Uuid>,
}

/// Persistent config stored in `generated/.../config.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerativeConfig {
    #[serde(default)]
    pub provider_id: Option<Uuid>,
    #[serde(default)]
    pub inputs: HashMap<String, InputValue>,
    #[serde(default)]
    pub reference_slots: HashMap<String, InputValue>,
    #[serde(default)]
    pub batch: BatchSettings,
    #[serde(default)]
    pub versions: Vec<GenerationRecord>,
    #[serde(default)]
    pub active_version: Option<String>,
    #[serde(default)]
    pub lab_graph: AssetLabGraph,
}

impl Default for GenerativeConfig {
    fn default() -> Self {
        Self {
            provider_id: None,
            inputs: HashMap::new(),
            reference_slots: HashMap::new(),
            batch: BatchSettings::default(),
            versions: Vec::new(),
            active_version: None,
            lab_graph: AssetLabGraph::default(),
        }
    }
}

impl GenerativeConfig {
    pub fn load(folder: &Path) -> io::Result<Self> {
        let path = config_path(folder);
        let tmp_path = temp_config_path(folder);
        let json = match fs::read_to_string(&path) {
            Ok(json) => json,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                if let Ok(json) = fs::read_to_string(&tmp_path) {
                    json
                } else {
                    return Ok(Self::default());
                }
            }
            Err(err) => return Err(err),
        };
        match serde_json::from_str::<Self>(&json) {
            Ok(mut config) => {
                config.normalize_lab_graph_lineage();
                Ok(config)
            }
            Err(err) => {
                if let Ok(tmp_json) = fs::read_to_string(&tmp_path) {
                    let mut config = serde_json::from_str::<Self>(&tmp_json)
                        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                    config.normalize_lab_graph_lineage();
                    Ok(config)
                } else {
                    Err(io::Error::new(io::ErrorKind::InvalidData, err))
                }
            }
        }
    }

    pub fn save(&self, folder: &Path) -> io::Result<()> {
        fs::create_dir_all(folder)?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        let path = config_path(folder);
        let tmp_path = temp_config_path(folder);
        fs::write(&tmp_path, json)?;
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }
}

pub fn generation_record_source_inputs(
    config: &GenerativeConfig,
    record: &GenerationRecord,
) -> HashMap<String, InputValue> {
    if let Some(node_id) = record.lab_node_id {
        if let Some(node) = config
            .lab_graph
            .nodes
            .iter()
            .find(|node| node.id == node_id)
        {
            if !node.inputs.is_empty() {
                return node.inputs.clone();
            }
        }
    }
    record.inputs_snapshot.clone()
}

fn config_path(folder: &Path) -> PathBuf {
    folder.join("config.json")
}

fn temp_config_path(folder: &Path) -> PathBuf {
    folder.join("config.json.tmp")
}

pub fn generative_info_for_clip(
    project: &Project,
    clip_id: uuid::Uuid,
) -> Option<(PathBuf, ProviderOutputType)> {
    let clip = project.clips.iter().find(|clip| clip.id == clip_id)?;
    let asset = project.find_asset(clip.asset_id)?;
    let (folder, output) = match &asset.kind {
        AssetKind::GenerativeVideo { folder, .. } => (folder.clone(), ProviderOutputType::Video),
        AssetKind::GenerativeImage { folder, .. } => (folder.clone(), ProviderOutputType::Image),
        AssetKind::GenerativeAudio { folder, .. } => (folder.clone(), ProviderOutputType::Audio),
        _ => return None,
    };
    Some((folder, output))
}

pub fn parse_version_index(version: &str) -> Option<u32> {
    let trimmed = version.trim();
    let numeric = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))?;
    numeric.parse::<u32>().ok()
}

pub fn delete_generative_version_files(folder: &Path, version: &str) -> Result<(), String> {
    let entries = fs::read_dir(folder).map_err(|err| err.to_string())?;
    let mut deleted_any = false;
    for entry in entries {
        let path = entry.map_err(|err| err.to_string())?.path();
        if !path.is_file() {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if stem == version {
            fs::remove_file(&path).map_err(|err| err.to_string())?;
            deleted_any = true;
        }
    }
    if deleted_any {
        Ok(())
    } else {
        Err(format!("No files found for version {version}."))
    }
}

/// Delete files for all provided generation versions in the folder.
pub fn delete_all_generative_version_files(
    folder: &Path,
    versions: &[String],
) -> Result<(), String> {
    if versions.is_empty() {
        return Ok(());
    }
    let targets: HashSet<&str> = versions.iter().map(|version| version.as_str()).collect();
    let entries = fs::read_dir(folder).map_err(|err| err.to_string())?;
    for entry in entries {
        let path = entry.map_err(|err| err.to_string())?.path();
        if !path.is_file() {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if targets.contains(stem) {
            fs::remove_file(&path).map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

pub fn next_generative_index(
    assets: &[Asset],
    prefix: &str,
    kind_filter: fn(&AssetKind) -> bool,
) -> u32 {
    let mut max_index = 0u32;
    for asset in assets.iter() {
        if !kind_filter(&asset.kind) {
            continue;
        }
        if let Some(suffix) = asset.name.strip_prefix(prefix) {
            let trimmed = suffix.trim();
            if let Ok(index) = trimmed.parse::<u32>() {
                max_index = max_index.max(index);
            }
        }
    }
    max_index + 1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum GenerationJobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GenerationSeedAdvance {
    pub field: String,
    pub next_seed: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GenerationJob {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub status: GenerationJobStatus,
    pub progress_overall: Option<f32>,
    pub progress_node: Option<f32>,
    pub attempts: u8,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub provider: ProviderEntry,
    pub output_type: ProviderOutputType,
    pub asset_id: Uuid,
    pub clip_id: Option<Uuid>,
    pub asset_label: String,
    pub folder_path: PathBuf,
    pub inputs: HashMap<String, serde_json::Value>,
    pub inputs_snapshot: HashMap<String, InputValue>,
    pub seed_advance: Option<GenerationSeedAdvance>,
    pub version: Option<String>,
    pub lab_node_id: Option<Uuid>,
    pub activate_on_success: bool,
    pub error: Option<String>,
}
