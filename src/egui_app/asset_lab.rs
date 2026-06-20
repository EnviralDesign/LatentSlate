use std::path::{Path, PathBuf};

use super::*;

use eframe::egui::{
    self, Color32, ColorImage, FontId, Pos2, Rect, Response, Sense, Stroke, TextureHandle,
    TextureId, Ui, Vec2,
};
use uuid::Uuid;

use crate::state::{
    parse_version_index, Asset, AssetKind, AssetLabNode, GenerationRecord, GenerativeConfig,
    InputValue, ProviderEntry, ProviderInputField, ProviderInputType, ProviderOutputType,
};
use crate::ui_kit as kit;

use super::asset_panel::{
    asset_accent, asset_icon, natural_case_insensitive_cmp, paint_truncated_row_text_bottom,
    paint_truncated_row_text_top,
};
use super::preview_transform::preview_scroll_delta;
use super::{
    inspector_meta_row, path_label, ASSET_LAB_PREVIEW_H, ASSET_LAB_VERSION_ROW_H, AUDIO_EXTENSIONS,
    IMAGE_EXTENSIONS, PREVIEW_SCROLL_ZOOM_SENSITIVITY, PREVIEW_ZOOM_MAX, PREVIEW_ZOOM_MIN,
    VIDEO_EXTENSIONS,
};

#[derive(Clone, Debug)]
pub(super) struct AssetLabState {
    pub(super) asset_id: Option<Uuid>,
    pub(super) selected_version: Option<String>,
    pub(super) pending_delete_version: Option<String>,
    pub(super) local_time_seconds: f64,
    pub(super) preview_auto_fit: bool,
    pub(super) preview_zoom: f32,
    pub(super) preview_pan: Vec2,
    pub(super) preview_pan_drag: Option<(Vec2, Pos2)>,
    pub(super) graph_pan: Vec2,
    pub(super) graph_zoom: f32,
    pub(super) graph_pan_drag: Option<(Vec2, Pos2)>,
    pub(super) draft_source_node_id: Option<Uuid>,
    pub(super) draft_base_version: Option<String>,
    pub(super) draft_provider_id: Option<Uuid>,
    pub(super) draft_inputs: HashMap<String, InputValue>,
}

impl Default for AssetLabState {
    fn default() -> Self {
        Self {
            asset_id: None,
            selected_version: None,
            pending_delete_version: None,
            local_time_seconds: 0.0,
            preview_auto_fit: true,
            preview_zoom: 1.0,
            preview_pan: Vec2::ZERO,
            preview_pan_drag: None,
            graph_pan: Vec2::ZERO,
            graph_zoom: 1.0,
            graph_pan_drag: None,
            draft_source_node_id: None,
            draft_base_version: None,
            draft_provider_id: None,
            draft_inputs: HashMap::new(),
        }
    }
}

impl AssetLabState {
    pub(super) fn clear_draft(&mut self) {
        self.draft_source_node_id = None;
        self.draft_base_version = None;
        self.draft_provider_id = None;
        self.draft_inputs.clear();
    }

    fn draft_matches(&self, source_node_id: Uuid, base_version: Option<&str>) -> bool {
        self.draft_source_node_id == Some(source_node_id)
            && self.draft_base_version.as_deref() == base_version
    }
}

pub(super) struct AssetLabPreviewTexture {
    pub(super) asset_id: Uuid,
    pub(super) version: Option<String>,
    pub(super) path: PathBuf,
    pub(super) frame_index: Option<i64>,
    pub(super) texture: TextureHandle,
    pub(super) size: Vec2,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(super) struct AssetLabNodePreviewKey {
    pub(super) asset_id: Uuid,
    pub(super) version: Option<String>,
    pub(super) frame_index: Option<i64>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(super) enum AssetLabAction {
    SelectVersion(String),
    SetActive(String),
    DuplicateVersion(String),
    ExtractVersion(String),
    ExtractCurrentFrame(String),
    AddNode(Option<Uuid>),
    SelectNode(Uuid),
    ClearNodeSelection,
    SetNodeProvider {
        node_id: Uuid,
        provider_id: Option<Uuid>,
    },
    UpdateNodeInput {
        node_id: Uuid,
        input_name: String,
        value: InputValue,
    },
    ClearNodeInput {
        node_id: Uuid,
        input_name: String,
    },
    GenerateNode(Uuid),
    CreateEditStepFromVersion(String),
    DeleteNode(Uuid),
    DuplicateAsset,
    AddAssetToTimeline,
    RequestDeleteAsset,
    RequestDelete(String),
    ConfirmDelete(String),
    CancelDelete,
    OpenLocation,
}

pub(super) fn asset_lab_video_fps(asset: &Asset, fallback: f64) -> f64 {
    match asset.kind {
        AssetKind::GenerativeVideo { fps, .. } => fps.max(1.0),
        _ => fallback.max(1.0),
    }
}

pub(super) fn asset_source_label(asset: &Asset) -> Option<String> {
    match &asset.kind {
        AssetKind::Video { path } | AssetKind::Image { path } | AssetKind::Audio { path } => {
            Some(path_label(path))
        }
        AssetKind::GenerativeVideo { folder, .. }
        | AssetKind::GenerativeImage { folder, .. }
        | AssetKind::GenerativeAudio { folder, .. } => Some(path_label(folder)),
    }
}

pub(super) fn generative_folder_for_asset(asset: &Asset) -> Option<&PathBuf> {
    match &asset.kind {
        AssetKind::GenerativeVideo { folder, .. }
        | AssetKind::GenerativeImage { folder, .. }
        | AssetKind::GenerativeAudio { folder, .. } => Some(folder),
        _ => None,
    }
}

fn generative_output_extensions(asset: &Asset) -> Option<&'static [&'static str]> {
    match asset.kind {
        AssetKind::GenerativeImage { .. } => Some(IMAGE_EXTENSIONS),
        AssetKind::GenerativeVideo { .. } => Some(VIDEO_EXTENSIONS),
        AssetKind::GenerativeAudio { .. } => Some(AUDIO_EXTENSIONS),
        _ => None,
    }
}

pub(super) fn generative_output_file_for_version(
    project_root: &Path,
    asset: &Asset,
    version: Option<&str>,
) -> Option<PathBuf> {
    let folder = generative_folder_for_asset(asset)?;
    let extensions = generative_output_extensions(asset)?;
    resolve_generative_file(
        project_root,
        folder,
        version.or_else(|| asset.active_version()),
        extensions,
    )
}

pub(super) fn asset_lab_media_path(
    project_root: &Path,
    asset: &Asset,
    version: Option<&str>,
) -> Option<PathBuf> {
    match &asset.kind {
        AssetKind::Video { path } | AssetKind::Image { path } | AssetKind::Audio { path } => {
            Some(project_root.join(path))
        }
        AssetKind::GenerativeVideo { .. }
        | AssetKind::GenerativeImage { .. }
        | AssetKind::GenerativeAudio { .. } => {
            generative_output_file_for_version(project_root, asset, version)
        }
    }
}

pub(super) fn asset_lab_location_path(project_root: &Path, asset: &Asset) -> Option<PathBuf> {
    if let Some(folder) = generative_folder_for_asset(asset) {
        return Some(project_root.join(folder));
    }
    asset_lab_media_path(project_root, asset, None)
        .map(|path| path.parent().map(Path::to_path_buf).unwrap_or(path))
}

pub(super) fn sorted_generation_records(config: &GenerativeConfig) -> Vec<GenerationRecord> {
    let mut records = config.versions.clone();
    records.sort_by(|a, b| {
        match (
            parse_version_index(&a.version),
            parse_version_index(&b.version),
        ) {
            (Some(a_index), Some(b_index)) => b_index.cmp(&a_index),
            _ => natural_case_insensitive_cmp(&b.version, &a.version),
        }
        .then_with(|| b.timestamp.cmp(&a.timestamp))
    });
    records
}

pub(super) fn preferred_asset_lab_version(config: &GenerativeConfig) -> Option<String> {
    if let Some(active) = config.active_version.as_ref() {
        if config
            .versions
            .iter()
            .any(|record| record.version == *active)
        {
            return Some(active.clone());
        }
    }
    sorted_generation_records(config)
        .first()
        .map(|record| record.version.clone())
}

pub(super) fn asset_lab_version_exists(config: Option<&GenerativeConfig>, version: &str) -> bool {
    config.is_some_and(|config| {
        config
            .versions
            .iter()
            .any(|record| record.version == version)
    })
}

pub(super) fn asset_lab_provider_name(providers: &[ProviderEntry], provider_id: Uuid) -> String {
    providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .map(|provider| provider.name.clone())
        .unwrap_or_else(|| provider_id.to_string())
}

pub(super) fn asset_lab_output_type(asset: &Asset) -> Option<ProviderOutputType> {
    match asset.kind {
        AssetKind::GenerativeImage { .. } | AssetKind::Image { .. } => {
            Some(ProviderOutputType::Image)
        }
        AssetKind::GenerativeVideo { .. } | AssetKind::Video { .. } => {
            Some(ProviderOutputType::Video)
        }
        AssetKind::GenerativeAudio { .. } | AssetKind::Audio { .. } => {
            Some(ProviderOutputType::Audio)
        }
    }
}

pub(super) fn asset_lab_provider_is_compatible(asset: &Asset, provider: &ProviderEntry) -> bool {
    asset_lab_output_type(asset) == Some(provider.output_type)
}

pub(super) fn asset_lab_meta_row(ui: &mut Ui, label: &str, value: impl Into<String>) {
    inspector_meta_row(ui, label, value);
}

pub(super) fn asset_lab_preview_header(ui: &mut Ui, timecode_label: Option<String>) {
    let row_h = 14.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
    let painter = ui.painter();
    painter.text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        "PREVIEW",
        FontId::proportional(10.5),
        kit::TEXT_MUTED,
    );
    if let Some(label) = timecode_label {
        painter.text(
            rect.right_center(),
            egui::Align2::RIGHT_CENTER,
            label,
            FontId::monospace(10.5),
            kit::TEXT_MUTED,
        );
    }
}

#[allow(dead_code)]
pub(super) fn asset_lab_version_row(
    ui: &mut Ui,
    record: &GenerationRecord,
    selected: bool,
    active: bool,
) -> Response {
    kit::draw_accent_row_with_status(
        ui,
        ASSET_LAB_VERSION_ROW_H,
        selected,
        kit::IMAGE,
        active.then_some(kit::PRIMARY),
        |ui, content_rect| {
            let title = if active {
                format!("Output {}  ON TIMELINE", record.version)
            } else {
                format!("Output {}", record.version)
            };
            paint_truncated_row_text_top(
                ui,
                Pos2::new(content_rect.left(), content_rect.top()),
                kit::value(title),
                12.0,
                content_rect.width(),
                kit::TEXT,
            );
            paint_truncated_row_text_bottom(
                ui,
                Pos2::new(content_rect.left(), content_rect.bottom()),
                kit::caption(
                    record
                        .timestamp
                        .with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string(),
                ),
                11.0,
                content_rect.width(),
                kit::TEXT_MUTED,
            );
        },
    )
}

#[allow(dead_code)]
fn asset_lab_type_badge(asset: &Asset) -> (&'static str, Color32) {
    if asset.is_video() {
        ("Video Asset", kit::VIDEO)
    } else if asset.is_image() {
        ("Image Asset", kit::IMAGE)
    } else {
        ("Audio Asset", kit::AUDIO)
    }
}

fn asset_lab_output_type_badge(asset: &Asset) -> (&'static str, Color32) {
    if asset.is_video() {
        ("Video", kit::VIDEO)
    } else if asset.is_image() {
        ("Image", kit::IMAGE)
    } else {
        ("Audio", kit::AUDIO)
    }
}

fn asset_lab_generation_ref_for_input(
    asset: &Asset,
    input: &ProviderInputField,
    version: &str,
) -> Option<InputValue> {
    match input.input_type {
        ProviderInputType::Image if asset.is_image() => Some(InputValue::GenerationRef {
            asset_id: asset.id,
            version: version.to_string(),
            frame_reference: None,
        }),
        ProviderInputType::Video if asset.is_video() => Some(InputValue::GenerationRef {
            asset_id: asset.id,
            version: version.to_string(),
            frame_reference: None,
        }),
        ProviderInputType::Image if asset.is_video() => Some(InputValue::GenerationRef {
            asset_id: asset.id,
            version: version.to_string(),
            frame_reference: Some(SourceFrameReference::First),
        }),
        ProviderInputType::Audio if asset.is_audio() => Some(InputValue::GenerationRef {
            asset_id: asset.id,
            version: version.to_string(),
            frame_reference: None,
        }),
        _ => None,
    }
}

fn retain_node_inputs_for_provider(
    inputs: &mut HashMap<String, InputValue>,
    provider: &ProviderEntry,
) {
    inputs.retain(|name, value| {
        provider
            .inputs
            .iter()
            .find(|input| input.name == *name)
            .is_some_and(|input| asset_lab_input_value_valid_for_field(value, input))
    });
}

fn asset_lab_input_value_valid_for_field(value: &InputValue, input: &ProviderInputField) -> bool {
    match value {
        InputValue::Literal { value } => match &input.input_type {
            ProviderInputType::Text => value.is_string(),
            ProviderInputType::Number => input_value_as_f64(value).is_some(),
            ProviderInputType::Integer => input_value_as_i64(value).is_some(),
            ProviderInputType::Boolean => input_value_as_bool(value).is_some(),
            ProviderInputType::Enum { options } => input_value_as_string(value)
                .is_some_and(|value| options.iter().any(|option| option == &value)),
            ProviderInputType::Image | ProviderInputType::Video | ProviderInputType::Audio => false,
        },
        InputValue::AssetRef {
            asset_id: _,
            source_clip_id: _,
            pinned: _,
            frame_reference,
        }
        | InputValue::GenerationRef {
            asset_id: _,
            version: _,
            frame_reference,
        } => match &input.input_type {
            ProviderInputType::Image => true,
            ProviderInputType::Video | ProviderInputType::Audio => frame_reference.is_none(),
            _ => false,
        },
    }
}

fn asset_lab_record_for_version<'a>(
    config: Option<&'a GenerativeConfig>,
    version: Option<&str>,
) -> Option<&'a GenerationRecord> {
    let version = version?;
    config.and_then(|config| {
        config
            .versions
            .iter()
            .find(|record| record.version == version)
    })
}

fn asset_lab_record_is_node_baseline(record: &GenerationRecord, node: &AssetLabNode) -> bool {
    record.lab_node_id == Some(node.id)
        || node.output_version.as_deref() == Some(record.version.as_str())
}

fn asset_lab_input_is_media_link(asset: &Asset, input: &ProviderInputField) -> bool {
    asset_lab_generation_ref_for_input(asset, input, "__probe__").is_some()
}

fn asset_lab_primary_media_input_name<'a>(
    asset: &Asset,
    provider: &'a ProviderEntry,
) -> Option<&'a str> {
    provider
        .inputs
        .iter()
        .find(|input| asset_lab_input_is_media_link(asset, input))
        .map(|input| input.name.as_str())
}

#[derive(Clone, Debug)]
struct AssetLabGraphLayoutEntry {
    node_id: Uuid,
    lane: usize,
    depth: usize,
}

#[derive(Clone, Debug)]
struct AssetLabGhostLayoutEntry {
    parent_node_id: Uuid,
    lane: usize,
    depth: usize,
}

#[derive(Clone, Debug, Default)]
struct AssetLabGraphLayout {
    nodes: Vec<AssetLabGraphLayoutEntry>,
    ghost: Option<AssetLabGhostLayoutEntry>,
}

#[derive(Clone, Debug)]
struct AssetLabMediaRefInput {
    label: String,
    version: String,
}

#[derive(Clone, Debug)]
struct AssetLabMediaRefLink {
    source_node_id: Uuid,
    target_node_id: Uuid,
    label: String,
    port_index: usize,
    port_count: usize,
}

#[derive(Clone, Copy, Debug)]
enum AssetLabNodeIcon {
    Pin,
    Trash,
}

fn asset_lab_graph_layout(
    config: Option<&GenerativeConfig>,
    draft_source_node_id: Option<Uuid>,
) -> AssetLabGraphLayout {
    let Some(config) = config else {
        return AssetLabGraphLayout::default();
    };

    let nodes = &config.lab_graph.nodes;
    let mut layout = AssetLabGraphLayout::default();
    let mut next_lane = 0usize;
    let mut visited = std::collections::HashSet::new();

    fn walk(
        node_id: Uuid,
        depth: usize,
        lane: usize,
        draft_source_node_id: Option<Uuid>,
        nodes: &[AssetLabNode],
        layout: &mut AssetLabGraphLayout,
        next_lane: &mut usize,
        visited: &mut std::collections::HashSet<Uuid>,
    ) {
        if !visited.insert(node_id) {
            return;
        }
        layout.nodes.push(AssetLabGraphLayoutEntry {
            node_id,
            lane,
            depth,
        });

        let mut children: Vec<Uuid> = nodes
            .iter()
            .filter(|node| node.parent_node_id == Some(node_id))
            .map(|node| node.id)
            .collect();

        let ghost_index = if draft_source_node_id == Some(node_id) {
            let index = children.len();
            children.push(Uuid::nil());
            Some(index)
        } else {
            None
        };

        for (index, child_id) in children.into_iter().enumerate() {
            let child_lane = if index == 0 {
                lane
            } else {
                let lane = *next_lane;
                *next_lane += 1;
                lane
            };
            if ghost_index == Some(index) {
                layout.ghost = Some(AssetLabGhostLayoutEntry {
                    parent_node_id: node_id,
                    lane: child_lane,
                    depth: depth + 1,
                });
            } else {
                walk(
                    child_id,
                    depth + 1,
                    child_lane,
                    draft_source_node_id,
                    nodes,
                    layout,
                    next_lane,
                    visited,
                );
            }
        }
    }

    for node in nodes.iter().filter(|node| node.parent_node_id.is_none()) {
        let lane = next_lane;
        next_lane += 1;
        walk(
            node.id,
            0,
            lane,
            draft_source_node_id,
            nodes,
            &mut layout,
            &mut next_lane,
            &mut visited,
        );
    }

    layout
}

fn asset_lab_node_provider<'a>(
    providers: &'a [ProviderEntry],
    node: &AssetLabNode,
) -> Option<&'a ProviderEntry> {
    node.provider_id
        .and_then(|provider_id| providers.iter().find(|provider| provider.id == provider_id))
}

fn asset_lab_ordered_media_ref_inputs(
    asset: &Asset,
    node: &AssetLabNode,
    provider: Option<&ProviderEntry>,
) -> Vec<AssetLabMediaRefInput> {
    let mut refs = Vec::new();
    let mut used_names: Vec<&str> = Vec::new();

    if let Some(provider) = provider {
        for input in &provider.inputs {
            let Some(value) = node.inputs.get(&input.name) else {
                continue;
            };
            let InputValue::GenerationRef {
                asset_id, version, ..
            } = value
            else {
                continue;
            };
            if *asset_id != asset.id
                || !asset_lab_input_is_media_link(asset, input)
                || !asset_lab_input_value_valid_for_field(value, input)
            {
                continue;
            }
            refs.push(AssetLabMediaRefInput {
                label: asset_lab_input_label(input),
                version: version.clone(),
            });
            used_names.push(input.name.as_str());
        }
    }

    let mut leftover_refs: Vec<(&String, &InputValue)> = node
        .inputs
        .iter()
        .filter(|(name, value)| {
            !used_names.iter().any(|used| *used == name.as_str())
                && matches!(
                    value,
                    InputValue::GenerationRef { asset_id, .. } if *asset_id == asset.id
                )
        })
        .collect();
    leftover_refs.sort_by(|(left, _), (right, _)| left.cmp(right));

    for (name, value) in leftover_refs {
        let InputValue::GenerationRef { version, .. } = value else {
            continue;
        };
        let label = provider
            .and_then(|provider| provider.inputs.iter().find(|input| input.name == *name))
            .map(asset_lab_input_label)
            .unwrap_or_else(|| {
                name.replace('_', " ")
                    .replace('-', " ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            });
        refs.push(AssetLabMediaRefInput {
            label,
            version: version.clone(),
        });
    }

    refs
}

fn asset_lab_version_source_nodes(
    nodes: &[AssetLabNode],
    versions: &[GenerationRecord],
) -> HashMap<String, Uuid> {
    let mut sources = HashMap::new();
    for record in versions {
        if let Some(node_id) = record.lab_node_id {
            sources.insert(record.version.clone(), node_id);
        }
    }
    for node in nodes {
        if let Some(version) = node.output_version.as_ref() {
            sources.entry(version.clone()).or_insert(node.id);
        }
    }
    sources
}

fn asset_lab_media_ref_links(
    asset: &Asset,
    nodes: &[AssetLabNode],
    versions: &[GenerationRecord],
    providers: &[ProviderEntry],
) -> Vec<AssetLabMediaRefLink> {
    let version_source_nodes = asset_lab_version_source_nodes(nodes, versions);
    let mut links = Vec::new();

    for node in nodes {
        let provider = asset_lab_node_provider(providers, node);
        let refs = asset_lab_ordered_media_ref_inputs(asset, node, provider);
        let port_count = refs.len();
        for (port_index, media_ref) in refs.into_iter().enumerate() {
            let Some(source_node_id) = version_source_nodes.get(&media_ref.version).copied() else {
                continue;
            };
            if node.parent_node_id == Some(source_node_id) {
                continue;
            }
            links.push(AssetLabMediaRefLink {
                source_node_id,
                target_node_id: node.id,
                label: media_ref.label,
                port_index,
                port_count,
            });
        }
    }

    links
}

fn asset_lab_input_port(node_rect: Rect, index: usize, count: usize, zoom: f32) -> Pos2 {
    let spacing = (24.0 * zoom).clamp(12.0, 34.0);
    let side = if index % 2 == 0 { -1.0 } else { 1.0 };
    let rank = (index / 2 + 1) as f32;
    let raw_x = if count <= 1 {
        node_rect.center().x - spacing
    } else {
        node_rect.center().x + side * rank * spacing
    };
    Pos2::new(
        raw_x.clamp(
            node_rect.left() + (18.0 * zoom).max(8.0),
            node_rect.right() - (18.0 * zoom).max(8.0),
        ),
        node_rect.bottom(),
    )
}

fn asset_lab_media_ref_route(
    source_rect: Rect,
    target_rect: Rect,
    port_index: usize,
    port_count: usize,
    zoom: f32,
) -> Vec<Pos2> {
    let start = source_rect.center_top();
    let end = asset_lab_input_port(target_rect, port_index, port_count, zoom);
    let below_target_gap = (28.0 * zoom).clamp(14.0, 44.0);
    let mut bus_y = if start.y > end.y {
        start.y + (end.y - start.y) * 0.5
    } else {
        end.y + below_target_gap
    };
    let fan_offset = (port_index as f32 - port_count.saturating_sub(1) as f32 * 0.5)
        * (7.0 * zoom).clamp(4.0, 10.0);
    bus_y += fan_offset;

    vec![
        start,
        Pos2::new(start.x, bus_y),
        Pos2::new(end.x, bus_y),
        end,
    ]
}

fn asset_lab_short_port_label(label: &str) -> String {
    let trimmed = label.trim();
    if trimmed.chars().count() <= 18 {
        return trimmed.to_string();
    }

    let prefix: String = trimmed.chars().take(15).collect();
    format!("{prefix}...")
}

fn paint_asset_lab_dashed_segment(
    painter: &egui::Painter,
    start: Pos2,
    end: Pos2,
    stroke: Stroke,
    dash: f32,
    gap: f32,
) {
    let delta = end - start;
    let length = delta.length();
    if length <= f32::EPSILON {
        return;
    }

    let direction = delta / length;
    let mut cursor = 0.0;
    while cursor < length {
        let next = (cursor + dash).min(length);
        painter.line_segment(
            [start + direction * cursor, start + direction * next],
            stroke,
        );
        cursor += dash + gap;
    }
}

fn paint_asset_lab_dashed_rect(painter: &egui::Painter, rect: Rect, stroke: Stroke) {
    let dash = 9.0;
    let gap = 5.0;
    paint_asset_lab_dashed_segment(
        painter,
        rect.left_top(),
        rect.right_top(),
        stroke,
        dash,
        gap,
    );
    paint_asset_lab_dashed_segment(
        painter,
        rect.right_top(),
        rect.right_bottom(),
        stroke,
        dash,
        gap,
    );
    paint_asset_lab_dashed_segment(
        painter,
        rect.right_bottom(),
        rect.left_bottom(),
        stroke,
        dash,
        gap,
    );
    paint_asset_lab_dashed_segment(
        painter,
        rect.left_bottom(),
        rect.left_top(),
        stroke,
        dash,
        gap,
    );
}

fn paint_asset_lab_arrowhead(
    painter: &egui::Painter,
    segment_start: Pos2,
    tip: Pos2,
    stroke: Stroke,
    size: f32,
) {
    let delta = tip - segment_start;
    let length = delta.length();
    if length <= f32::EPSILON {
        return;
    }

    let direction = delta / length;
    let normal = Vec2::new(-direction.y, direction.x);
    let left = tip - direction * size + normal * (size * 0.55);
    let right = tip - direction * size - normal * (size * 0.55);
    painter.line_segment([left, tip], stroke);
    painter.line_segment([right, tip], stroke);
}

fn paint_asset_lab_polyline(painter: &egui::Painter, points: &[Pos2], stroke: Stroke) {
    for pair in points.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if (end - start).length() <= f32::EPSILON {
            continue;
        }
        painter.line_segment([start, end], stroke);
    }
}

fn paint_asset_lab_polyline_arrow(
    painter: &egui::Painter,
    points: &[Pos2],
    stroke: Stroke,
    arrow_size: f32,
) {
    let mut last_segment = None;
    for pair in points.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if (end - start).length() <= f32::EPSILON {
            continue;
        }
        painter.line_segment([start, end], stroke);
        last_segment = Some((start, end));
    }
    if let Some((start, end)) = last_segment {
        paint_asset_lab_arrowhead(painter, start, end, stroke, arrow_size);
    }
}

fn paint_asset_lab_dashed_polyline_arrow(
    painter: &egui::Painter,
    points: &[Pos2],
    stroke: Stroke,
    arrow_size: f32,
) {
    let mut last_segment = None;
    for pair in points.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if (end - start).length() <= f32::EPSILON {
            continue;
        }
        paint_asset_lab_dashed_segment(painter, start, end, stroke, 9.0, 5.0);
        last_segment = Some((start, end));
    }
    if let Some((start, end)) = last_segment {
        paint_asset_lab_arrowhead(painter, start, end, stroke, arrow_size);
    }
}

fn asset_lab_fit_texture_rect(bounds: Rect, size: Vec2) -> Rect {
    if bounds.width() <= 0.0 || bounds.height() <= 0.0 || size.x <= 0.0 || size.y <= 0.0 {
        return bounds;
    }
    let scale = (bounds.width() / size.x).min(bounds.height() / size.y);
    Rect::from_center_size(bounds.center(), size * scale.max(0.01))
}

fn paint_asset_lab_node_preview(
    painter: &egui::Painter,
    rect: Rect,
    asset: &Asset,
    preview: Option<(TextureId, Vec2)>,
    accent: Color32,
    label: &str,
) {
    painter.rect_filled(rect, kit::field_radius(), kit::FIELD_BG);
    painter.rect_stroke(
        rect,
        kit::field_radius(),
        Stroke::new(1.0, kit::BORDER_SOFT),
        egui::StrokeKind::Inside,
    );

    if let Some((texture_id, size)) = preview {
        let image_rect = asset_lab_fit_texture_rect(rect.shrink(4.0), size);
        painter.image(
            texture_id,
            image_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_stroke(
            rect,
            kit::field_radius(),
            Stroke::new(1.0, accent.gamma_multiply(0.68)),
            egui::StrokeKind::Inside,
        );
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            asset_icon(asset),
            FontId::proportional(16.0),
            accent.gamma_multiply(0.92),
        );
    }

    let label_rect = Rect::from_min_size(
        rect.left_bottom() - Vec2::new(0.0, 22.0),
        Vec2::new(rect.width(), 22.0),
    );
    painter.rect_filled(
        label_rect,
        kit::field_radius(),
        Color32::from_rgba_unmultiplied(8, 10, 12, 170),
    );
    painter.text(
        label_rect.left_center() + Vec2::new(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.0),
        kit::TEXT,
    );
}

fn paint_asset_lab_node_icon(
    painter: &egui::Painter,
    rect: Rect,
    icon: AssetLabNodeIcon,
    color: Color32,
) {
    let center = rect.center();
    match icon {
        AssetLabNodeIcon::Pin => {
            painter.circle_filled(center + Vec2::new(0.0, -5.0), 3.2, color);
            painter.line_segment(
                [center + Vec2::new(0.0, -2.0), center + Vec2::new(0.0, 7.0)],
                Stroke::new(1.7, color),
            );
            painter.line_segment(
                [center + Vec2::new(-4.0, 0.0), center + Vec2::new(4.0, 0.0)],
                Stroke::new(1.5, color),
            );
            painter.line_segment(
                [center + Vec2::new(0.0, 7.0), center + Vec2::new(-3.0, 11.0)],
                Stroke::new(1.5, color.gamma_multiply(0.85)),
            );
        }
        AssetLabNodeIcon::Trash => {
            let body = Rect::from_center_size(center + Vec2::new(0.0, 3.0), Vec2::new(10.0, 11.0));
            painter.rect_stroke(body, 1.5, Stroke::new(1.5, color), egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    center + Vec2::new(-7.0, -5.0),
                    center + Vec2::new(7.0, -5.0),
                ],
                Stroke::new(1.6, color),
            );
            painter.line_segment(
                [
                    center + Vec2::new(-3.0, -8.0),
                    center + Vec2::new(3.0, -8.0),
                ],
                Stroke::new(1.5, color),
            );
            painter.line_segment(
                [center + Vec2::new(-2.5, 0.0), center + Vec2::new(-2.5, 8.0)],
                Stroke::new(1.0, color.gamma_multiply(0.85)),
            );
            painter.line_segment(
                [center + Vec2::new(2.5, 0.0), center + Vec2::new(2.5, 8.0)],
                Stroke::new(1.0, color.gamma_multiply(0.85)),
            );
        }
    }
}

fn asset_lab_node_icon_button(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    icon: AssetLabNodeIcon,
    enabled: bool,
    active: bool,
    danger: bool,
    tooltip: &str,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense).on_hover_text(tooltip);
    let fill = if active {
        kit::PRIMARY.gamma_multiply(0.55)
    } else if danger {
        Color32::from_rgb(75, 24, 28)
    } else {
        Color32::from_rgb(23, 25, 29)
    };
    let hover_fill = if danger {
        Color32::from_rgb(105, 30, 35)
    } else {
        Color32::from_rgb(33, 37, 42)
    };
    let stroke = if active {
        kit::PRIMARY
    } else if danger {
        kit::DANGER
    } else {
        kit::BORDER_SOFT
    };
    let icon_color = if enabled || active {
        kit::TEXT
    } else {
        kit::TEXT_DIM
    };
    ui.painter().rect_filled(
        rect,
        kit::field_radius(),
        if response.hovered() && enabled {
            hover_fill
        } else {
            fill
        },
    );
    ui.painter().rect_stroke(
        rect,
        kit::field_radius(),
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    paint_asset_lab_node_icon(ui.painter(), rect, icon, icon_color);
    response
}

fn asset_lab_input_label(input: &ProviderInputField) -> String {
    let raw = if input.label.trim().is_empty() {
        input.name.trim()
    } else {
        input.label.trim()
    };
    let mut label = raw
        .trim_start_matches("LatentSlate ")
        .trim_start_matches("LatentSlate_")
        .trim_start_matches("latentslate ")
        .trim_start_matches("latentslate_")
        .replace('_', " ")
        .replace('-', " ");
    while label.contains("  ") {
        label = label.replace("  ", " ");
    }
    let normalized = label.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "POS PROMPT" | "POSITIVE" | "POSITIVE PROMPT" => "Positive Prompt".to_string(),
        "NEG PROMPT" | "NEGATIVE" | "NEGATIVE PROMPT" => "Negative Prompt".to_string(),
        "LWDNESS" => "Wildness".to_string(),
        "CFG" | "CFG SCALE" => "CFG Scale".to_string(),
        "STEPS" => "Steps".to_string(),
        "SEED" => "Seed".to_string(),
        "WIDTH" => "Width".to_string(),
        "HEIGHT" => "Height".to_string(),
        _ if label.chars().any(|ch| ch.is_ascii_lowercase()) => label.trim().to_string(),
        _ => label
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => format!(
                        "{}{}",
                        first.to_ascii_uppercase(),
                        chars.as_str().to_ascii_lowercase()
                    ),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

pub(super) fn asset_lab_preview(
    ui: &mut Ui,
    asset: &Asset,
    preview: Option<(TextureId, Vec2)>,
    state: &mut AssetLabState,
) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ASSET_LAB_PREVIEW_H),
        Sense::click_and_drag(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, kit::field_radius(), kit::FIELD_BG);
    painter.rect_stroke(
        rect,
        kit::field_radius(),
        Stroke::new(1.0, kit::BORDER_SOFT),
        egui::StrokeKind::Inside,
    );

    if let Some((texture_id, size)) = preview {
        let image_bounds = rect.shrink(10.0);
        let fit_scale = (image_bounds.width() / size.x.max(1.0))
            .min(image_bounds.height() / size.y.max(1.0))
            .max(0.01);
        if state.preview_auto_fit {
            state.preview_zoom = fit_scale;
            state.preview_pan = Vec2::ZERO;
        }

        if response.double_clicked() {
            state.preview_auto_fit = true;
            state.preview_zoom = fit_scale;
            state.preview_pan = Vec2::ZERO;
        }

        let scroll_delta = preview_scroll_delta(ui, rect);
        if scroll_delta.abs() > f32::EPSILON {
            let old_zoom = state.preview_zoom.max(PREVIEW_ZOOM_MIN);
            let zoom_factor =
                (1.0 + scroll_delta * PREVIEW_SCROLL_ZOOM_SENSITIVITY).clamp(0.5, 2.0);
            let new_zoom = (old_zoom * zoom_factor).clamp(PREVIEW_ZOOM_MIN, PREVIEW_ZOOM_MAX);
            if let Some(pointer) = ui.ctx().pointer_hover_pos() {
                let old_center = image_bounds.center() + state.preview_pan;
                let ratio = new_zoom / old_zoom;
                state.preview_pan =
                    pointer - (pointer - old_center) * ratio - image_bounds.center();
            }
            state.preview_zoom = new_zoom;
            state.preview_auto_fit = false;
        }

        let secondary_down =
            ui.input(|input| input.pointer.button_down(egui::PointerButton::Secondary));
        if secondary_down && (response.hovered() || state.preview_pan_drag.is_some()) {
            if let Some(pointer) = ui.ctx().pointer_hover_pos() {
                let (start_pan, start_pointer) = state
                    .preview_pan_drag
                    .get_or_insert((state.preview_pan, pointer));
                state.preview_pan = *start_pan + (pointer - *start_pointer);
                state.preview_auto_fit = false;
            }
        } else {
            state.preview_pan_drag = None;
        }

        let scale = state.preview_zoom.max(PREVIEW_ZOOM_MIN);
        let image_rect = Rect::from_center_size(image_bounds.center(), size * scale);
        let image_rect = image_rect.translate(state.preview_pan);
        painter.image(
            texture_id,
            image_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            asset_icon(asset),
            FontId::proportional(18.0),
            asset_accent(asset),
        );
    }
}

pub(super) fn asset_lab_scrub_tick_step(duration: f64, width: f32) -> f64 {
    if duration <= 10.0 || width <= 0.0 {
        return 1.0;
    }
    let target_ticks = (width as f64 / 76.0).clamp(2.0, 16.0);
    let rough_step = duration / target_ticks;
    if rough_step <= 1.0 {
        1.0
    } else if rough_step <= 2.0 {
        2.0
    } else if rough_step <= 5.0 {
        5.0
    } else if rough_step <= 10.0 {
        10.0
    } else if rough_step <= 30.0 {
        30.0
    } else {
        60.0
    }
}

pub(super) fn copy_generative_version_files(
    folder_path: &Path,
    source_version: &str,
    target_version: &str,
) -> Result<(), String> {
    let entries = std::fs::read_dir(folder_path)
        .map_err(|err| format!("Failed to read generation folder: {err}"))?;
    let mut copied_any = false;
    for entry in entries {
        let source_path = entry
            .map_err(|err| format!("Failed to read generation folder entry: {err}"))?
            .path();
        if !source_path.is_file() {
            continue;
        }
        let stem = source_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if stem != source_version {
            continue;
        }
        let extension = source_path
            .extension()
            .and_then(|extension| extension.to_str())
            .ok_or_else(|| format!("Version file {} has no extension.", source_path.display()))?;
        let target_path = folder_path.join(format!("{target_version}.{extension}"));
        std::fs::copy(&source_path, &target_path)
            .map_err(|err| format!("Failed to duplicate {}: {err}", source_path.display()))?;
        copied_any = true;
    }
    if copied_any {
        Ok(())
    } else {
        Err(format!("No output files found for {source_version}."))
    }
}

fn generation_version_dependents(project: &Project, asset_id: Uuid, version: &str) -> Vec<String> {
    let mut dependents = Vec::new();
    for (config_asset_id, config) in &project.generative_configs {
        if *config_asset_id == asset_id {
            continue;
        }
        if !generative_config_references_version(config, asset_id, version) {
            continue;
        }
        let label = project
            .find_asset(*config_asset_id)
            .map(|asset| format!("{} ({})", asset_display_name(asset), config_asset_id))
            .unwrap_or_else(|| config_asset_id.to_string());
        dependents.push(label);
    }
    dependents.sort();
    dependents
}

fn generative_config_references_version(
    config: &GenerativeConfig,
    asset_id: Uuid,
    version: &str,
) -> bool {
    config
        .inputs
        .values()
        .chain(config.reference_slots.values())
        .any(|input| input_references_generation_version(input, asset_id, version))
        || config.lab_graph.nodes.iter().any(|node| {
            node.inputs
                .values()
                .any(|input| input_references_generation_version(input, asset_id, version))
        })
}

fn input_references_generation_version(input: &InputValue, asset_id: Uuid, version: &str) -> bool {
    matches!(
        input,
        InputValue::GenerationRef {
            asset_id: ref_asset_id,
            version: ref_version,
            ..
        } if *ref_asset_id == asset_id && ref_version == version
    )
}

pub(super) fn asset_thumbnail_candidates(project_root: &Path, asset: &Asset) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    match &asset.kind {
        AssetKind::Image { path } => {
            candidates.push(project_root.join(path));
        }
        AssetKind::GenerativeImage {
            folder,
            active_version,
        } => {
            if let Some(path) = resolve_generative_file(
                project_root,
                folder,
                active_version.as_deref(),
                IMAGE_EXTENSIONS,
            ) {
                candidates.push(path);
            }
        }
        AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. } => {}
        AssetKind::Audio { .. } | AssetKind::GenerativeAudio { .. } => return candidates,
    }

    if asset.is_visual() {
        if let Some(path) = cached_asset_thumbnail(project_root, asset.id) {
            candidates.push(path);
        }
    }
    candidates
}

fn cached_asset_thumbnail(project_root: &Path, asset_id: Uuid) -> Option<PathBuf> {
    let dir = project_root
        .join(".cache")
        .join("thumbnails")
        .join(asset_id.to_string());
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .metadata()
                    .map(|metadata| metadata.len() > 1024)
                    .unwrap_or(false)
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        ["jpg", "jpeg", "png", "webp"]
                            .iter()
                            .any(|allowed| allowed.eq_ignore_ascii_case(ext))
                    })
        })
        .collect();
    entries.sort();
    entries.into_iter().next()
}

pub(super) fn resolve_generative_file(
    project_root: &Path,
    folder: &Path,
    active_version: Option<&str>,
    extensions: &[&str],
) -> Option<PathBuf> {
    let active_version = active_version?;
    let folder_path = project_root.join(folder);
    for ext in extensions {
        let candidate = folder_path.join(format!("{active_version}.{ext}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

pub(super) fn load_thumbnail_image(path: &Path) -> Option<(ColorImage, Vec2)> {
    load_preview_image(path, 96)
}

pub(super) fn load_preview_image(path: &Path, max_edge: u32) -> Option<(ColorImage, Vec2)> {
    let max_edge = max_edge.max(1);
    let image = image::open(path)
        .ok()?
        .thumbnail(max_edge, max_edge)
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let display_size = Vec2::new(size[0] as f32, size[1] as f32);
    let color_image = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some((color_image, display_size))
}
impl LatentSlateApp {
    pub(super) fn open_asset_lab(&mut self, asset_id: Uuid) {
        self.open_asset_lab_at_time(asset_id, None);
    }

    pub(super) fn open_asset_lab_at_time(
        &mut self,
        asset_id: Uuid,
        local_time_seconds: Option<f64>,
    ) {
        let selected_version = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(preferred_asset_lab_version)
            .or_else(|| {
                self.editor
                    .project
                    .find_asset(asset_id)
                    .and_then(|asset| asset.active_version().map(str::to_string))
            });
        self.asset_lab = AssetLabState {
            asset_id: Some(asset_id),
            selected_version,
            pending_delete_version: None,
            local_time_seconds: local_time_seconds.unwrap_or(0.0).max(0.0),
            preview_auto_fit: true,
            preview_zoom: 1.0,
            preview_pan: Vec2::ZERO,
            preview_pan_drag: None,
            graph_pan: Vec2::ZERO,
            graph_zoom: 1.0,
            graph_pan_drag: None,
            draft_source_node_id: None,
            draft_base_version: None,
            draft_provider_id: None,
            draft_inputs: HashMap::new(),
        };
        self.asset_lab_preview_texture = None;
        self.asset_lab_node_preview_textures.clear();
        self.editor.overlays.asset_lab = true;
        self.ensure_asset_lab_graph_for_versions(asset_id);
    }

    pub(super) fn close_asset_lab(&mut self) {
        self.editor.overlays.asset_lab = false;
        self.asset_lab = AssetLabState::default();
        self.asset_lab_preview_texture = None;
        self.asset_lab_node_preview_textures.clear();
    }

    pub(super) fn asset_lab_modal(&mut self, ctx: &Context) {
        let Some(asset_id) = self.asset_lab.asset_id else {
            self.close_asset_lab();
            return;
        };
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.close_asset_lab();
            return;
        };

        let config_snapshot = self.editor.project.generative_config(asset_id).cloned();
        if asset.is_generative() {
            let selected_is_valid = self
                .asset_lab
                .selected_version
                .as_deref()
                .is_some_and(|version| asset_lab_version_exists(config_snapshot.as_ref(), version));
            if !selected_is_valid {
                self.asset_lab.selected_version = config_snapshot
                    .as_ref()
                    .and_then(preferred_asset_lab_version)
                    .or_else(|| asset.active_version().map(str::to_string));
            }
        }

        let mut open = true;
        let mut close_clicked = false;
        let mut action: Option<AssetLabAction> = None;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "asset_lab", true);
        let size = modal_size(ctx, ASSET_LAB_MODAL_SIZE, [660.0, 460.0]);
        let subtitle = if asset.is_generative() {
            let active = config_snapshot
                .as_ref()
                .and_then(|config| config.active_version.as_deref())
                .or_else(|| asset.active_version())
                .unwrap_or("none");
            let output_count = config_snapshot
                .as_ref()
                .map(|config| config.versions.len())
                .unwrap_or_default();
            format!(
                "{}  |  {}  |  Timeline: {}  |  Lab outputs: {}",
                asset.name,
                asset_kind_label(&asset.kind),
                active,
                output_count
            )
        } else {
            format!("{}  |  {}", asset.name, asset_kind_label(&asset.kind))
        };

        egui::Window::new("Asset Lab")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked =
                    kit::modal_header_with_close(ui, "Asset Lab", Some(&subtitle), true);
                kit::modal_body(ui, |ui| {
                    self.asset_lab_modal_contents(
                        ui,
                        &asset,
                        config_snapshot.as_ref(),
                        &mut action,
                    );
                });
            });

        if let Some(action) = action {
            self.handle_asset_lab_action(asset_id, action);
        } else if close_clicked || outside_clicked || !open {
            self.close_asset_lab();
        }
    }

    pub(super) fn asset_lab_modal_contents(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: Option<&GenerativeConfig>,
        action: &mut Option<AssetLabAction>,
    ) {
        if !asset.is_generative() {
            self.asset_lab_basic_asset_contents(ui, asset, action);
            return;
        }

        let versions = config.map(sorted_generation_records).unwrap_or_default();
        let selected_version = self.asset_lab.selected_version.clone();
        let active_version = asset.active_version().map(str::to_string);
        let pending_delete = self.asset_lab.pending_delete_version.clone();
        let selected_node_id = config
            .and_then(|config| config.lab_graph.selected_node_id)
            .filter(|node_id| {
                config.is_some_and(|config| {
                    config
                        .lab_graph
                        .nodes
                        .iter()
                        .any(|node| node.id == *node_id)
                })
            });
        let compatible_providers: Vec<ProviderEntry> = self
            .editor
            .provider_entries
            .iter()
            .filter(|provider| asset_lab_provider_is_compatible(asset, provider))
            .cloned()
            .collect();

        StripBuilder::new(ui)
            .clip(true)
            .size(Size::remainder().at_least(540.0))
            .size(Size::exact(460.0))
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    self.asset_lab_flow_column(
                        ui,
                        asset,
                        config,
                        &versions,
                        selected_node_id,
                        active_version.as_deref(),
                        &compatible_providers,
                        action,
                    );
                });
                strip.cell(|ui| {
                    self.asset_lab_node_inspector(
                        ui,
                        asset,
                        config,
                        &versions,
                        selected_node_id,
                        selected_version.as_deref(),
                        active_version.as_deref(),
                        pending_delete.as_deref(),
                        &compatible_providers,
                        action,
                    );
                });
            });
    }

    pub(super) fn asset_lab_flow_column(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: Option<&GenerativeConfig>,
        versions: &[GenerationRecord],
        selected_node_id: Option<Uuid>,
        active_version: Option<&str>,
        compatible_providers: &[ProviderEntry],
        action: &mut Option<AssetLabAction>,
    ) {
        kit::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                kit::field_label(ui, "Flow");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    kit::media_pill(
                        ui,
                        &format!("{:.0}%", self.asset_lab.graph_zoom * 100.0),
                        kit::TEXT_MUTED,
                    );
                    if config
                        .map(|config| config.lab_graph.nodes.is_empty())
                        .unwrap_or(true)
                        && kit::secondary_button(ui, "+ Step", 82.0).clicked()
                    {
                        let provider_id = compatible_providers.first().map(|provider| provider.id);
                        *action = Some(AssetLabAction::AddNode(provider_id));
                    }
                });
            });
            ui.add_space(kit::FORM_ROW_GAP);
            ui.add(
                egui::Label::new(kit::caption(
                    "Select a generation, edit its settings, then generate the staged variant.",
                ))
                .wrap(),
            );
            ui.add_space(kit::FORM_ROW_GAP);
            self.asset_lab_flow_canvas(
                ui,
                asset,
                config,
                versions,
                selected_node_id,
                active_version,
                compatible_providers,
                action,
            );
        });
    }

    #[allow(dead_code)]
    pub(super) fn asset_lab_outputs_section(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        versions: &[GenerationRecord],
        selected_version: Option<&str>,
        active_version: Option<&str>,
        action: &mut Option<AssetLabAction>,
    ) {
        ui.horizontal(|ui| {
            kit::field_label(ui, "Lab Outputs");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(active_version) = active_version {
                    kit::media_pill(ui, &format!("Timeline: {active_version}"), kit::PRIMARY);
                } else {
                    kit::media_pill(ui, "No Timeline Output", kit::TEXT_MUTED);
                }
            });
        });
        ui.add_space(kit::FORM_ROW_GAP);
        if versions.is_empty() {
            let (label, color) = asset_lab_output_type_badge(asset);
            ui.horizontal(|ui| {
                kit::media_pill(ui, label, color);
                ui.label(kit::caption("Run a step to create the first lab output."));
            });
        } else {
            egui::ScrollArea::vertical()
                .id_salt(("asset_lab_outputs", asset.id))
                .max_height(78.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                    for record in versions.iter() {
                        let selected = selected_version == Some(record.version.as_str());
                        let active = active_version == Some(record.version.as_str());
                        let response = asset_lab_version_row(ui, record, selected, active);
                        if response.clicked() {
                            *action = Some(AssetLabAction::SelectVersion(record.version.clone()));
                        }
                    }
                });
        }
    }

    #[allow(dead_code)]
    pub(super) fn asset_lab_preview_column(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        versions: &[GenerationRecord],
        selected_version: Option<&str>,
        active_version: Option<&str>,
        action: &mut Option<AssetLabAction>,
    ) {
        kit::card_frame().show(ui, |ui| {
            let selected_output_path =
                selected_version.and_then(|version| {
                    self.editor.project.project_path.as_ref().and_then(|root| {
                        generative_output_file_for_version(root, asset, Some(version))
                    })
                });
            let selected_video_duration = asset
                .duration_seconds
                .filter(|duration| *duration > 0.0)
                .or_else(|| {
                    selected_output_path
                        .as_deref()
                        .filter(|_| asset.is_video())
                        .and_then(probe_duration_seconds)
                })
                .unwrap_or(0.0)
                .max(0.0);
            let selected_video_fps = asset_lab_video_fps(asset, self.editor.project.settings.fps);

            let preview_timecode =
                (asset.is_video() && selected_output_path.is_some()).then(|| {
                    let current = self
                        .asset_lab
                        .local_time_seconds
                        .min(selected_video_duration);
                    format!(
                        "{} / {}",
                        timecode(current),
                        timecode(selected_video_duration)
                    )
                });
            asset_lab_preview_header(ui, preview_timecode);
            ui.add_space(kit::FORM_ROW_GAP);
            let preview = self.asset_lab_preview_texture(ui.ctx(), asset, selected_version);
            asset_lab_preview(ui, asset, preview, &mut self.asset_lab);
            if asset.is_video() && selected_output_path.is_some() {
                self.asset_lab_video_scrubber(ui, selected_video_duration, selected_video_fps);
            }

            ui.add_space(kit::ACTION_GAP);
            self.asset_lab_timeline_status_row(ui, asset, selected_version, active_version, action);

            ui.add_space(kit::ACTION_GAP);
            self.asset_lab_outputs_section(
                ui,
                asset,
                versions,
                selected_version,
                active_version,
                action,
            );
        });
    }

    pub(super) fn asset_lab_flow_canvas(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: Option<&GenerativeConfig>,
        versions: &[GenerationRecord],
        selected_node_id: Option<Uuid>,
        active_version: Option<&str>,
        compatible_providers: &[ProviderEntry],
        action: &mut Option<AssetLabAction>,
    ) {
        let nodes = config
            .map(|config| config.lab_graph.nodes.as_slice())
            .unwrap_or(&[]);
        let available = ui.available_size();
        let viewport_h = available.y.max(220.0);
        let (canvas_rect, response) =
            ui.allocate_exact_size(Vec2::new(available.x, viewport_h), Sense::click_and_drag());
        let painter = ui.painter().with_clip_rect(canvas_rect);
        painter.rect_filled(canvas_rect, kit::field_radius(), kit::FIELD_BG);
        painter.rect_stroke(
            canvas_rect,
            kit::field_radius(),
            Stroke::new(1.0, kit::BORDER_SOFT),
            egui::StrokeKind::Inside,
        );

        if nodes.is_empty() {
            painter.text(
                canvas_rect.center_top() + Vec2::new(0.0, 76.0),
                egui::Align2::CENTER_CENTER,
                "Add a generation step",
                FontId::proportional(13.0),
                kit::TEXT_MUTED,
            );
            painter.text(
                canvas_rect.center_top() + Vec2::new(0.0, 98.0),
                egui::Align2::CENTER_CENTER,
                "The lineage graph appears here once this asset has staged steps.",
                FontId::proportional(11.0),
                kit::TEXT_DIM,
            );
            if kit::secondary_button(ui, "+ Step", 82.0).clicked() {
                let provider_id = compatible_providers.first().map(|provider| provider.id);
                *action = Some(AssetLabAction::AddNode(provider_id));
            }
            return;
        }

        let layout = asset_lab_graph_layout(config, self.asset_lab.draft_source_node_id);
        let lane_pitch = 346.0f32;
        let depth_pitch = 244.0f32;
        let node_size = Vec2::new(284.0, 194.0);
        let graph_margin = 32.0f32;
        let max_lane = layout
            .nodes
            .iter()
            .map(|entry| entry.lane)
            .chain(layout.ghost.iter().map(|entry| entry.lane))
            .max()
            .unwrap_or(0);
        let max_depth = layout
            .nodes
            .iter()
            .map(|entry| entry.depth)
            .chain(layout.ghost.iter().map(|entry| entry.depth))
            .max()
            .unwrap_or(0);
        let content_w = graph_margin * 2.0 + (max_lane as f32 + 1.0) * lane_pitch;
        let content_h = graph_margin * 2.0 + (max_depth as f32 + 1.0) * depth_pitch;
        let content_size = Vec2::new(content_w, content_h);

        let pointer = ui
            .ctx()
            .pointer_interact_pos()
            .or_else(|| ui.ctx().pointer_hover_pos());
        let pointer_in_canvas = pointer
            .map(|pointer| canvas_rect.contains(pointer))
            .unwrap_or(false);
        if ui.input(|input| input.pointer.secondary_pressed()) && pointer_in_canvas {
            if let Some(start_pointer) = pointer {
                self.asset_lab.graph_pan_drag = Some((self.asset_lab.graph_pan, start_pointer));
            }
        }
        if let Some((start_pan, start_pointer)) = self.asset_lab.graph_pan_drag {
            if ui.input(|input| input.pointer.secondary_down()) {
                if let Some(pointer) = pointer {
                    self.asset_lab.graph_pan = start_pan + (pointer - start_pointer);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::AllScroll);
                    ui.ctx().request_repaint();
                }
            } else {
                self.asset_lab.graph_pan_drag = None;
            }
        }

        let wheel_delta = preview_scroll_delta(ui, canvas_rect);
        if wheel_delta.abs() > f32::EPSILON {
            let old_zoom = self.asset_lab.graph_zoom.clamp(0.45, 2.4);
            let zoom_factor = (1.0 + wheel_delta * 0.015).clamp(0.82, 1.22);
            let new_zoom = (old_zoom * zoom_factor).clamp(0.45, 2.4);
            if (new_zoom - old_zoom).abs() > f32::EPSILON {
                if let Some(pointer) = ui
                    .ctx()
                    .pointer_hover_pos()
                    .filter(|pointer| canvas_rect.contains(*pointer))
                {
                    let old_origin = canvas_rect.center() + self.asset_lab.graph_pan
                        - content_size * old_zoom * 0.5;
                    let before = (pointer - old_origin) / old_zoom.max(0.0001);
                    self.asset_lab.graph_zoom = new_zoom;
                    self.asset_lab.graph_pan = pointer - canvas_rect.center()
                        + content_size * new_zoom * 0.5
                        - before * new_zoom;
                } else {
                    self.asset_lab.graph_zoom = new_zoom;
                }
            }
        }

        let zoom = self.asset_lab.graph_zoom.clamp(0.45, 2.4);
        self.asset_lab.graph_zoom = zoom;
        let origin = canvas_rect.center() + self.asset_lab.graph_pan - content_size * zoom * 0.5;
        let scaled_node = node_size * zoom;
        let scaled_lane_pitch = lane_pitch * zoom;
        let scaled_depth_pitch = depth_pitch * zoom;
        let scaled_margin = graph_margin * zoom;
        let mut node_rects: HashMap<Uuid, Rect> = HashMap::new();

        for entry in &layout.nodes {
            let x = origin.x + scaled_margin + entry.lane as f32 * scaled_lane_pitch;
            let y = origin.y
                + scaled_margin
                + (max_depth.saturating_sub(entry.depth)) as f32 * scaled_depth_pitch;
            let rect = Rect::from_min_size(Pos2::new(x, y), scaled_node);
            node_rects.insert(entry.node_id, rect);
        }

        let media_ref_links =
            asset_lab_media_ref_links(asset, nodes, versions, &self.editor.provider_entries);
        let mut media_ref_links_by_target: HashMap<Uuid, Vec<usize>> = HashMap::new();
        for (index, link) in media_ref_links.iter().enumerate() {
            media_ref_links_by_target
                .entry(link.target_node_id)
                .or_default()
                .push(index);
        }

        for link in &media_ref_links {
            let Some(source_rect) = node_rects.get(&link.source_node_id).copied() else {
                continue;
            };
            let Some(target_rect) = node_rects.get(&link.target_node_id).copied() else {
                continue;
            };
            let focused = selected_node_id.is_none_or(|node_id| {
                node_id == link.source_node_id || node_id == link.target_node_id
            });
            let stroke = Stroke::new(
                (1.15 * zoom).clamp(0.9, 1.8),
                if focused {
                    Color32::from_rgba_unmultiplied(75, 196, 123, 150)
                } else {
                    Color32::from_rgba_unmultiplied(75, 196, 123, 58)
                },
            );
            let route = asset_lab_media_ref_route(
                source_rect,
                target_rect,
                link.port_index,
                link.port_count,
                zoom,
            );
            paint_asset_lab_polyline(&painter, &route, stroke);
        }

        for node in nodes {
            let Some(node_rect) = node_rects.get(&node.id).copied() else {
                continue;
            };
            if let Some(parent_id) = node.parent_node_id {
                if let Some(parent_rect) = node_rects.get(&parent_id).copied() {
                    let start = parent_rect.center_top();
                    let end = node_rect.center_bottom();
                    let mid_y = start.y + (end.y - start.y) * 0.5;
                    let stroke = Stroke::new(
                        (1.6 * zoom).clamp(1.25, 2.4),
                        asset_accent(asset).gamma_multiply(0.78),
                    );
                    paint_asset_lab_polyline_arrow(
                        &painter,
                        &[
                            start,
                            Pos2::new(start.x, mid_y),
                            Pos2::new(end.x, mid_y),
                            end,
                        ],
                        stroke,
                        (8.0 * zoom).clamp(6.0, 12.0),
                    );
                }
            }
        }

        let mut node_or_sidecar_clicked = false;
        let mut ghost_generate_overlay: Option<(Rect, Uuid, bool)> = None;
        if let Some(ghost) = layout.ghost.as_ref() {
            if let Some(source_rect) = node_rects.get(&ghost.parent_node_id).copied() {
                let x = origin.x + scaled_margin + ghost.lane as f32 * scaled_lane_pitch;
                let y = origin.y
                    + scaled_margin
                    + (max_depth.saturating_sub(ghost.depth)) as f32 * scaled_depth_pitch;
                let ghost_rect = Rect::from_min_size(Pos2::new(x, y), scaled_node);
                let start = source_rect.center_top();
                let end = ghost_rect.center_bottom();
                let mid_y = start.y + (end.y - start.y) * 0.5;
                let stroke = Stroke::new(
                    (1.5 * zoom).clamp(1.2, 2.2),
                    kit::MARKER.gamma_multiply(0.92),
                );
                paint_asset_lab_dashed_polyline_arrow(
                    &painter,
                    &[
                        start,
                        Pos2::new(start.x, mid_y),
                        Pos2::new(end.x, mid_y),
                        end,
                    ],
                    stroke,
                    (8.0 * zoom).clamp(6.0, 12.0),
                );
                painter.rect_filled(
                    ghost_rect,
                    kit::field_radius(),
                    Color32::from_rgba_unmultiplied(244, 127, 45, 20),
                );
                paint_asset_lab_dashed_rect(&painter, ghost_rect, stroke);
                if zoom >= 0.65 {
                    let preview_rect = Rect::from_min_size(
                        ghost_rect.min + Vec2::splat(10.0 * zoom),
                        Vec2::new(
                            (ghost_rect.width() - 20.0 * zoom).max(20.0),
                            (ghost_rect.height() * 0.58).max(36.0),
                        ),
                    );
                    painter.rect_filled(
                        preview_rect,
                        kit::field_radius(),
                        Color32::from_rgba_unmultiplied(244, 127, 45, 24),
                    );
                    paint_asset_lab_dashed_rect(&painter, preview_rect, stroke);
                    painter.text(
                        ghost_rect.center_bottom() - Vec2::new(0.0, ghost_rect.height() * 0.25),
                        egui::Align2::CENTER_CENTER,
                        "Ungenerated variant",
                        FontId::proportional(12.0),
                        kit::MARKER,
                    );
                }
                let pending_job = self.editor.generation_queue.iter().rev().any(|job| {
                    job.lab_node_id == Some(ghost.parent_node_id)
                        && matches!(
                            job.status,
                            GenerationJobStatus::Queued | GenerationJobStatus::Running
                        )
                });
                let can_generate = asset.is_generative()
                    && self.asset_lab.draft_source_node_id == Some(ghost.parent_node_id)
                    && self.asset_lab.draft_provider_id.is_some()
                    && !pending_job;
                let button_size = Vec2::new((ghost_rect.width() - 28.0).clamp(104.0, 148.0), 34.0);
                let button_rect =
                    Rect::from_min_size(ghost_rect.left_top() + Vec2::new(14.0, 14.0), button_size);
                ghost_generate_overlay = Some((button_rect, ghost.parent_node_id, can_generate));
            }
        }

        for entry in &layout.nodes {
            let Some(node) = nodes.iter().find(|node| node.id == entry.node_id) else {
                continue;
            };
            let Some(node_rect) = node_rects.get(&node.id).copied() else {
                continue;
            };
            let selected = selected_node_id == Some(node.id);
            let active_output = node
                .output_version
                .as_deref()
                .is_some_and(|version| active_version == Some(version));
            let response = crate::core::automation::instrument_response(
                ui.interact(
                    node_rect,
                    ui.id().with(("asset_lab_node", node.id)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand),
                "step",
                Some(
                    node.provider_id
                        .map(|id| asset_lab_provider_name(&self.editor.provider_entries, id))
                        .unwrap_or_else(|| "Select provider".to_string()),
                ),
                true,
                false,
            );
            if response.clicked() {
                node_or_sidecar_clicked = true;
                *action = Some(AssetLabAction::SelectNode(node.id));
            }

            let node_label = node
                .provider_id
                .map(|id| asset_lab_provider_name(&self.editor.provider_entries, id))
                .unwrap_or_else(|| "Staged step".to_string());
            let node_depth = config
                .map(|config| config.lineage_depth(node.id))
                .unwrap_or(0);
            let pending_job_status = self
                .editor
                .generation_queue
                .iter()
                .rev()
                .find(|job| {
                    job.lab_node_id == Some(node.id)
                        && matches!(
                            job.status,
                            GenerationJobStatus::Queued | GenerationJobStatus::Running
                        )
                })
                .map(|job| job.status);
            let output_label = if let Some(status) = pending_job_status {
                match status {
                    GenerationJobStatus::Queued => "Queued".to_string(),
                    GenerationJobStatus::Running => "Running".to_string(),
                    _ => "In progress".to_string(),
                }
            } else {
                node.output_version
                    .as_deref()
                    .map(|version| {
                        if active_output {
                            format!("{version} on timeline")
                        } else {
                            format!("Output {version}")
                        }
                    })
                    .unwrap_or_else(|| "Staged".to_string())
            };

            let fill = if selected {
                Color32::from_rgb(25, 38, 36)
            } else if response.hovered() {
                Color32::from_rgb(27, 30, 34)
            } else {
                Color32::from_rgb(19, 21, 24)
            };
            painter.rect_filled(node_rect, kit::field_radius(), fill);
            painter.rect_stroke(
                node_rect,
                kit::field_radius(),
                Stroke::new(
                    1.1,
                    if active_output {
                        kit::PRIMARY
                    } else if selected {
                        kit::BORDER_FOCUS
                    } else {
                        kit::BORDER_SOFT
                    },
                ),
                egui::StrokeKind::Inside,
            );
            painter.rect_filled(
                Rect::from_min_size(node_rect.left_top(), Vec2::new(4.0, node_rect.height())),
                kit::field_radius(),
                asset_accent(asset),
            );

            let inner = node_rect.shrink(10.0 * zoom);
            let preview_h = (node_rect.height() * 0.58).clamp(56.0, 118.0 * zoom.max(0.72));
            let preview_rect =
                Rect::from_min_size(inner.left_top(), Vec2::new(inner.width(), preview_h));
            let preview_response = ui.interact(
                preview_rect,
                ui.id().with(("asset_lab_node_preview", node.id)),
                Sense::hover(),
            );
            let output_path =
                node.output_version.as_deref().and_then(|version| {
                    self.editor.project.project_path.as_ref().and_then(|root| {
                        generative_output_file_for_version(root, asset, Some(version))
                    })
                });
            let video_duration = if asset.is_video() {
                asset
                    .duration_seconds
                    .filter(|duration| *duration > 0.0)
                    .or_else(|| output_path.as_deref().and_then(probe_duration_seconds))
                    .unwrap_or(0.0)
                    .max(0.0)
            } else {
                0.0
            };
            let mut scrub_fraction = if asset.is_video() && video_duration > 0.0 {
                (self.asset_lab.local_time_seconds / video_duration).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };
            let mut preview_time = if selected && asset.is_video() {
                self.asset_lab.local_time_seconds.min(video_duration)
            } else {
                0.0
            };
            if asset.is_video() && output_path.is_some() && preview_response.hovered() {
                if let Some(pointer) = ui.ctx().pointer_hover_pos() {
                    scrub_fraction =
                        ((pointer.x - preview_rect.left()) / preview_rect.width()).clamp(0.0, 1.0);
                    preview_time = video_duration * scrub_fraction as f64;
                    if selected {
                        self.asset_lab.local_time_seconds = preview_time;
                        self.asset_lab_preview_texture = None;
                    }
                }
            }
            let preview = if node.output_version.is_some() {
                self.asset_lab_node_preview_texture(
                    ui.ctx(),
                    asset,
                    node.output_version.as_deref(),
                    preview_time,
                )
            } else {
                None
            };
            paint_asset_lab_node_preview(
                &painter,
                preview_rect,
                asset,
                preview,
                asset_accent(asset),
                &output_label,
            );
            if asset.is_video() && output_path.is_some() {
                let scrub_y = preview_rect.bottom() - 4.0;
                painter.line_segment(
                    [
                        Pos2::new(preview_rect.left() + 8.0, scrub_y),
                        Pos2::new(preview_rect.right() - 8.0, scrub_y),
                    ],
                    Stroke::new(1.0, kit::BORDER_SOFT),
                );
                let scrub_x = egui::lerp(
                    (preview_rect.left() + 8.0)..=(preview_rect.right() - 8.0),
                    scrub_fraction,
                );
                painter.line_segment(
                    [
                        Pos2::new(scrub_x, preview_rect.top() + 7.0),
                        Pos2::new(scrub_x, preview_rect.bottom() - 5.0),
                    ],
                    Stroke::new(1.4, kit::MARKER),
                );
            }

            if zoom >= 0.64 {
                let text_top = preview_rect.bottom() + 8.0;
                let text_left = inner.left();
                painter.text(
                    Pos2::new(text_left, text_top),
                    egui::Align2::LEFT_TOP,
                    format!("Step {}", node_depth + 1),
                    FontId::proportional(12.5),
                    kit::TEXT,
                );
                painter.text(
                    Pos2::new(text_left, text_top + 20.0),
                    egui::Align2::LEFT_TOP,
                    node_label,
                    FontId::proportional(11.0),
                    kit::TEXT_MUTED,
                );
                painter.text(
                    Pos2::new(text_left, text_top + 39.0),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "{} media refs | {} inputs",
                        node.inputs
                            .values()
                            .filter(|value| matches!(value, InputValue::GenerationRef { .. }))
                            .count(),
                        node.inputs.len()
                    ),
                    FontId::proportional(10.5),
                    kit::TEXT_DIM,
                );
            }

            if let Some(link_indexes) = media_ref_links_by_target.get(&node.id) {
                let focused_node = selected_node_id.is_none_or(|selected| selected == node.id);
                let rail_y = node_rect.bottom() - (1.5 * zoom).clamp(1.0, 2.0);
                painter.line_segment(
                    [
                        Pos2::new(node_rect.left() + 14.0 * zoom, rail_y),
                        Pos2::new(node_rect.right() - 14.0 * zoom, rail_y),
                    ],
                    Stroke::new(
                        (1.0 * zoom).clamp(0.7, 1.25),
                        if focused_node {
                            Color32::from_rgba_unmultiplied(75, 196, 123, 112)
                        } else {
                            Color32::from_rgba_unmultiplied(75, 196, 123, 46)
                        },
                    ),
                );
                for link_index in link_indexes {
                    let Some(link) = media_ref_links.get(*link_index) else {
                        continue;
                    };
                    let endpoint_focused = selected_node_id.is_none_or(|selected| {
                        selected == link.source_node_id || selected == link.target_node_id
                    });
                    let port =
                        asset_lab_input_port(node_rect, link.port_index, link.port_count, zoom);
                    let port_radius = (4.0 * zoom).clamp(2.4, 5.2);
                    painter.circle_filled(
                        port,
                        port_radius,
                        if endpoint_focused {
                            Color32::from_rgba_unmultiplied(82, 220, 136, 220)
                        } else {
                            Color32::from_rgba_unmultiplied(82, 220, 136, 92)
                        },
                    );
                    painter.circle_stroke(
                        port,
                        port_radius + 1.2,
                        Stroke::new(1.0, Color32::from_rgba_unmultiplied(7, 13, 10, 190)),
                    );

                    if selected && zoom >= 0.86 {
                        let label = asset_lab_short_port_label(&link.label);
                        let label_size = Vec2::new((label.len() as f32 * 5.6 + 14.0) * zoom, 16.0);
                        let label_rect =
                            Rect::from_center_size(port - Vec2::new(0.0, 17.0 * zoom), label_size);
                        painter.rect_filled(
                            label_rect,
                            kit::field_radius(),
                            Color32::from_rgba_unmultiplied(15, 42, 29, 218),
                        );
                        painter.rect_stroke(
                            label_rect,
                            kit::field_radius(),
                            Stroke::new(1.0, Color32::from_rgba_unmultiplied(82, 220, 136, 140)),
                            egui::StrokeKind::Inside,
                        );
                        painter.text(
                            label_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            label,
                            FontId::proportional((9.0 * zoom).clamp(8.0, 10.0)),
                            kit::TEXT,
                        );
                    }
                }
            }

            if selected {
                let icon_size = Vec2::splat(26.0);
                let action_top = inner.bottom() - icon_size.y - 6.0;
                let action_right = inner.right() - 6.0;
                let trash_rect = Rect::from_min_size(
                    Pos2::new(action_right - icon_size.x, action_top),
                    icon_size,
                );
                if let Some(version) = node.output_version.as_deref() {
                    let pin_rect = trash_rect.translate(Vec2::new(-(icon_size.x + 6.0), 0.0));
                    let pin_enabled = !active_output;
                    let pin = asset_lab_node_icon_button(
                        ui,
                        pin_rect,
                        ui.id().with(("asset_lab_node_pin", node.id)),
                        AssetLabNodeIcon::Pin,
                        pin_enabled,
                        active_output,
                        false,
                        if active_output {
                            "Active output"
                        } else {
                            "Set active output"
                        },
                    );
                    if pin_enabled && pin.clicked() {
                        node_or_sidecar_clicked = true;
                        *action = Some(AssetLabAction::SetActive(version.to_string()));
                    }
                } else {
                    let generate_w = (inner.width() - icon_size.x - 14.0).clamp(72.0, 96.0);
                    let generate_rect = Rect::from_min_size(
                        Pos2::new(trash_rect.left() - generate_w - 6.0, action_top),
                        Vec2::new(generate_w, icon_size.y),
                    );
                    let generate_label = pending_job_status
                        .map(|status| match status {
                            GenerationJobStatus::Queued => "Queued",
                            GenerationJobStatus::Running => "Running",
                            _ => "Generate",
                        })
                        .unwrap_or("Generate");
                    let can_generate = asset.is_generative()
                        && node.provider_id.is_some()
                        && pending_job_status.is_none();
                    let mut generate_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(generate_rect)
                            .layout(Layout::top_down(Align::Min)),
                    );
                    generate_ui.set_min_size(generate_rect.size());
                    generate_ui.shrink_clip_rect(generate_rect);
                    generate_ui.set_width(generate_rect.width());
                    generate_ui.set_max_width(generate_rect.width());
                    generate_ui.add_enabled_ui(can_generate, |ui| {
                        if kit::primary_button(ui, generate_label, ui.available_width()).clicked() {
                            node_or_sidecar_clicked = true;
                            *action = Some(AssetLabAction::GenerateNode(node.id));
                        }
                    });
                }
                if asset_lab_node_icon_button(
                    ui,
                    trash_rect,
                    ui.id().with(("asset_lab_node_delete", node.id)),
                    AssetLabNodeIcon::Trash,
                    true,
                    false,
                    true,
                    "Delete step",
                )
                .clicked()
                {
                    node_or_sidecar_clicked = true;
                    *action = Some(AssetLabAction::DeleteNode(node.id));
                }
            }
        }

        if let Some((sidecar_rect, source_node_id, can_generate)) = ghost_generate_overlay {
            let mut sidecar_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(sidecar_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            sidecar_ui.set_min_size(sidecar_rect.size());
            sidecar_ui.shrink_clip_rect(sidecar_rect);
            sidecar_ui.set_width(sidecar_rect.width());
            sidecar_ui.set_max_width(sidecar_rect.width());
            let width = sidecar_ui.available_width();
            sidecar_ui.add_enabled_ui(can_generate, |ui| {
                if kit::primary_button(ui, "Generate Variant", width).clicked() {
                    node_or_sidecar_clicked = true;
                    *action = Some(AssetLabAction::GenerateNode(source_node_id));
                }
            });
        }

        if response.clicked() && !node_or_sidecar_clicked && action.is_none() {
            *action = Some(AssetLabAction::ClearNodeSelection);
        }
    }

    pub(super) fn asset_lab_node_inspector(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: Option<&GenerativeConfig>,
        versions: &[GenerationRecord],
        selected_node_id: Option<Uuid>,
        selected_version: Option<&str>,
        _active_version: Option<&str>,
        _pending_delete: Option<&str>,
        compatible_providers: &[ProviderEntry],
        action: &mut Option<AssetLabAction>,
    ) {
        kit::card_frame().show(ui, |ui| {
            kit::field_label(ui, "Inspector");
            ui.add_space(kit::FORM_ROW_GAP);

            let selected_node = config.and_then(|config| {
                selected_node_id
                    .and_then(|id| config.lab_graph.nodes.iter().find(|node| node.id == id))
            });

            let Some(node) = selected_node else {
                ui.label(kit::caption(
                    "Select a node to edit provider settings and generation parameters.",
                ));
                return;
            };

            let selected_record = asset_lab_record_for_version(config, selected_version);
            let baseline_record =
                selected_record.filter(|record| asset_lab_record_is_node_baseline(record, node));
            let draft_active = baseline_record
                .is_some_and(|_| self.asset_lab.draft_matches(node.id, selected_version));

            let mut display_node = node.clone();
            if draft_active {
                display_node.provider_id = self.asset_lab.draft_provider_id;
                display_node.inputs = self.asset_lab.draft_inputs.clone();
                display_node.output_version = None;
            } else if let Some(record) = baseline_record {
                display_node.provider_id = Some(record.provider_id);
                display_node.inputs = record.inputs_snapshot.clone();
                display_node.output_version = Some(record.version.clone());
            }

            let selected_provider = display_node.provider_id.and_then(|provider_id| {
                compatible_providers
                    .iter()
                    .find(|provider| provider.id == provider_id)
            });

            let scroll_height = ui.available_height().max(160.0);
            egui::ScrollArea::vertical()
                .id_salt(("asset_lab_inspector", asset.id, node.id))
                .max_height(scroll_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        kit::field_label(ui, "Settings");
                        if draft_active {
                            kit::media_pill(ui, "Ungenerated Variant", kit::MARKER);
                        } else if baseline_record.is_some() {
                            kit::media_pill(ui, "Matches Output", kit::PRIMARY);
                        } else if display_node.output_version.is_some() {
                            kit::media_pill(ui, "Generated", kit::PRIMARY);
                        } else {
                            kit::media_pill(ui, "Staged", kit::TEXT_MUTED);
                        }
                    });
                    ui.add_space(kit::FORM_ROW_GAP);

                    let provider_label = selected_provider
                        .map(|provider| provider.name.clone())
                        .unwrap_or_else(|| "Select provider".to_string());
                    let mut provider_choice = display_node.provider_id;
                    kit::labeled_combo_field(
                        ui,
                        "Provider",
                        ("asset_lab_node_provider", node.id),
                        provider_label,
                        |ui| {
                            automation_selectable_value(ui, &mut provider_choice, None, "None");
                            for provider in compatible_providers {
                                automation_selectable_value(
                                    ui,
                                    &mut provider_choice,
                                    Some(provider.id),
                                    &provider.name,
                                );
                            }
                        },
                    );
                    if provider_choice != display_node.provider_id {
                        *action = Some(AssetLabAction::SetNodeProvider {
                            node_id: node.id,
                            provider_id: provider_choice,
                        });
                    }

                    ui.add_space(kit::ACTION_GAP);
                    if let Some(provider) = selected_provider {
                        kit::field_label(ui, "Media Wiring");
                        ui.add_space(kit::FORM_ROW_GAP);
                        let mut any_media = false;
                        for input in provider
                            .inputs
                            .iter()
                            .filter(|input| asset_lab_input_is_media_link(asset, input))
                        {
                            if any_media {
                                ui.add_space(kit::FORM_ROW_GAP);
                            }
                            any_media = true;
                            self.asset_lab_node_media_input_field(
                                ui,
                                asset,
                                &display_node,
                                input,
                                versions,
                                action,
                            );
                        }
                        if !any_media {
                            ui.label(kit::caption("This provider has no media wiring."));
                        }

                        ui.add_space(kit::ACTION_GAP);
                        kit::field_label(ui, "Parameters");
                        ui.add_space(kit::FORM_ROW_GAP);
                        let mut any_scalar = false;
                        for input in provider
                            .inputs
                            .iter()
                            .filter(|input| !asset_lab_input_is_media_link(asset, input))
                        {
                            if any_scalar {
                                ui.add_space(kit::FORM_ROW_GAP);
                            }
                            any_scalar = true;
                            self.asset_lab_node_input_field(
                                ui,
                                asset,
                                &display_node,
                                input,
                                versions,
                                action,
                            );
                        }
                        if !any_scalar {
                            ui.label(kit::caption("This provider has no scalar parameters."));
                        }
                    } else {
                        ui.label(kit::caption("Choose a provider before wiring inputs."));
                    }
                });
        });
    }

    pub(super) fn asset_lab_node_input_field(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        node: &AssetLabNode,
        input: &ProviderInputField,
        versions: &[GenerationRecord],
        action: &mut Option<AssetLabAction>,
    ) {
        let clean_label = asset_lab_input_label(input);
        let label = if input.required {
            format!("{clean_label} *")
        } else {
            clean_label
        };
        let current_value = node
            .inputs
            .get(&input.name)
            .and_then(|value| match value {
                InputValue::Literal { value } => Some(value.clone()),
                InputValue::AssetRef { .. } | InputValue::GenerationRef { .. } => None,
            })
            .or_else(|| input.default.clone());

        match &input.input_type {
            ProviderInputType::Text => {
                let mut value = current_value
                    .as_ref()
                    .and_then(input_value_as_string)
                    .unwrap_or_default();
                let multiline = input.ui.as_ref().map(|ui| ui.multiline).unwrap_or(false);
                let changed = if multiline {
                    provider_input_multiline_text_field(
                        ui,
                        &label,
                        input,
                        &mut value,
                        kit::MultilineTextFieldOptions::rows(3),
                    )
                } else {
                    provider_input_text_field(ui, &label, input, &mut value)
                };
                if changed {
                    *action = Some(AssetLabAction::UpdateNodeInput {
                        node_id: node.id,
                        input_name: input.name.clone(),
                        value: InputValue::Literal {
                            value: serde_json::Value::String(value),
                        },
                    });
                }
            }
            ProviderInputType::Number => {
                let mut value = current_value
                    .as_ref()
                    .and_then(input_value_as_f64)
                    .unwrap_or(0.0);
                let step = input.ui.as_ref().and_then(|ui| ui.step).unwrap_or(0.1);
                let width = ui.available_width();
                if provider_input_drag_f64(ui, &label, input, &mut value, step, width) {
                    if let Some(number) = serde_json::Number::from_f64(value) {
                        *action = Some(AssetLabAction::UpdateNodeInput {
                            node_id: node.id,
                            input_name: input.name.clone(),
                            value: InputValue::Literal {
                                value: serde_json::Value::Number(number),
                            },
                        });
                    }
                }
            }
            ProviderInputType::Integer => {
                let mut value = current_value
                    .as_ref()
                    .and_then(input_value_as_i64)
                    .unwrap_or(0);
                let step = input.ui.as_ref().and_then(|ui| ui.step).unwrap_or(1.0);
                let width = ui.available_width();
                if provider_input_drag_i64(ui, &label, input, &mut value, step, width) {
                    *action = Some(AssetLabAction::UpdateNodeInput {
                        node_id: node.id,
                        input_name: input.name.clone(),
                        value: InputValue::Literal {
                            value: serde_json::Value::Number(value.into()),
                        },
                    });
                }
            }
            ProviderInputType::Boolean => {
                let mut value = current_value
                    .as_ref()
                    .and_then(input_value_as_bool)
                    .unwrap_or(false);
                if provider_input_bool_field(ui, &label, input, &mut value) {
                    *action = Some(AssetLabAction::UpdateNodeInput {
                        node_id: node.id,
                        input_name: input.name.clone(),
                        value: InputValue::Literal {
                            value: serde_json::Value::Bool(value),
                        },
                    });
                }
            }
            ProviderInputType::Enum { options } => {
                let mut value = current_value
                    .as_ref()
                    .and_then(input_value_as_string)
                    .or_else(|| options.first().cloned())
                    .unwrap_or_default();
                let before = value.clone();
                provider_input_labeled_combo_field(
                    ui,
                    &label,
                    input,
                    ("asset_lab_node_enum", node.id, &input.name),
                    empty_dash(&value).to_string(),
                    |ui| {
                        for option in options {
                            automation_selectable_value(ui, &mut value, option.clone(), option);
                        }
                    },
                );
                if value != before {
                    *action = Some(AssetLabAction::UpdateNodeInput {
                        node_id: node.id,
                        input_name: input.name.clone(),
                        value: InputValue::Literal {
                            value: serde_json::Value::String(value),
                        },
                    });
                }
            }
            ProviderInputType::Image | ProviderInputType::Video | ProviderInputType::Audio => {
                self.asset_lab_node_media_input_field(ui, asset, node, input, versions, action);
            }
        }
    }

    pub(super) fn asset_lab_node_media_input_field(
        &self,
        ui: &mut Ui,
        asset: &Asset,
        node: &AssetLabNode,
        input: &ProviderInputField,
        versions: &[GenerationRecord],
        action: &mut Option<AssetLabAction>,
    ) {
        let current_value = node.inputs.get(&input.name).cloned();
        let current_label = current_value
            .as_ref()
            .and_then(|value| self.asset_lab_node_input_label(value))
            .unwrap_or_else(|| "None selected".to_string());
        let mut next_value = current_value.clone();
        provider_input_labeled_combo_field(
            ui,
            &asset_lab_input_label(input),
            input,
            ("asset_lab_node_media_input", node.id, &input.name),
            current_label,
            |ui| {
                automation_selectable_value(ui, &mut next_value, None, "None");

                let internal_sources =
                    self.asset_lab_internal_generation_sources(asset, node, input, versions);
                if !internal_sources.is_empty() {
                    ui.separator();
                    ui.label(kit::caption("This asset outputs"));
                    for (label, value) in internal_sources {
                        automation_selectable_value(ui, &mut next_value, Some(value), &label);
                    }
                }

                let external_candidates: Vec<_> = self
                    .asset_input_candidates(input, None)
                    .into_iter()
                    .filter(|candidate| candidate.asset_id != asset.id)
                    .collect();
                if !external_candidates.is_empty() {
                    ui.separator();
                    ui.label(kit::caption("Project / Timeline sources"));
                    for candidate in external_candidates {
                        let value = InputValue::AssetRef {
                            asset_id: candidate.asset_id,
                            source_clip_id: candidate.source_clip_id,
                            pinned: true,
                            frame_reference: candidate.frame_reference,
                        };
                        let label =
                            format!("External source: {}  {}", candidate.label, candidate.detail);
                        automation_selectable_value(ui, &mut next_value, Some(value), &label);
                    }
                }
            },
        );
        if next_value != current_value {
            *action = match next_value {
                Some(value) => Some(AssetLabAction::UpdateNodeInput {
                    node_id: node.id,
                    input_name: input.name.clone(),
                    value,
                }),
                None => Some(AssetLabAction::ClearNodeInput {
                    node_id: node.id,
                    input_name: input.name.clone(),
                }),
            };
        }
    }

    pub(super) fn asset_lab_internal_generation_sources(
        &self,
        asset: &Asset,
        node: &AssetLabNode,
        input: &ProviderInputField,
        versions: &[GenerationRecord],
    ) -> Vec<(String, InputValue)> {
        let mut values = Vec::new();
        let (type_label, _) = asset_lab_output_type_badge(asset);
        for record in versions {
            if node.output_version.as_deref() == Some(record.version.as_str()) {
                continue;
            }
            if let Some(value) = asset_lab_generation_ref_for_input(asset, input, &record.version) {
                let frame_suffix = match &value {
                    InputValue::GenerationRef {
                        frame_reference: Some(frame_reference),
                        ..
                    } => format!(" · {}", frame_reference.label()),
                    _ => String::new(),
                };
                values.push((
                    format!(
                        "Lab Output {} · {}{}",
                        record.version, type_label, frame_suffix
                    ),
                    value,
                ));
            }
            if matches!(input.input_type, ProviderInputType::Image) && asset.is_video() {
                for frame_reference in [SourceFrameReference::First, SourceFrameReference::Last] {
                    if frame_reference == SourceFrameReference::First {
                        continue;
                    }
                    values.push((
                        format!(
                            "Lab Output {} · {}",
                            record.version,
                            frame_reference.label()
                        ),
                        InputValue::GenerationRef {
                            asset_id: asset.id,
                            version: record.version.clone(),
                            frame_reference: Some(frame_reference),
                        },
                    ));
                }
            }
        }
        values
    }

    pub(super) fn asset_lab_node_input_label(&self, value: &InputValue) -> Option<String> {
        match value {
            InputValue::GenerationRef {
                asset_id,
                version,
                frame_reference,
            } => {
                self.editor.project.find_asset(*asset_id)?;
                let frame = frame_reference
                    .map(|frame| format!(" · {}", frame.label()))
                    .unwrap_or_default();
                Some(format!("This Asset · Output {}{}", version, frame))
            }
            InputValue::AssetRef { .. } => self
                .asset_input_label(value, None)
                .map(|label| format!("External · {label}")),
            InputValue::Literal { .. } => None,
        }
    }

    pub(super) fn asset_lab_basic_asset_contents(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        action: &mut Option<AssetLabAction>,
    ) {
        kit::card_frame().show(ui, |ui| {
            let media_path = self
                .editor
                .project
                .project_path
                .as_ref()
                .and_then(|root| asset_lab_media_path(root, asset, None));
            let media_duration = asset
                .duration_seconds
                .filter(|duration| *duration > 0.0)
                .or_else(|| {
                    media_path
                        .as_deref()
                        .filter(|_| asset.is_video())
                        .and_then(probe_duration_seconds)
                })
                .unwrap_or(0.0)
                .max(0.0);
            let media_fps = asset_lab_video_fps(asset, self.editor.project.settings.fps);

            let preview_timecode = (asset.is_video() && media_path.is_some()).then(|| {
                let current = self.asset_lab.local_time_seconds.min(media_duration);
                format!("{} / {}", timecode(current), timecode(media_duration))
            });
            asset_lab_preview_header(ui, preview_timecode);
            ui.add_space(kit::FORM_ROW_GAP);
            let preview = self.asset_lab_preview_texture(ui.ctx(), asset, None);
            asset_lab_preview(ui, asset, preview, &mut self.asset_lab);
            if asset.is_video() && media_path.is_some() {
                self.asset_lab_video_scrubber(ui, media_duration, media_fps);
            }

            ui.add_space(kit::ACTION_GAP);
            let details_height = ui.available_height().max(80.0);
            egui::ScrollArea::vertical()
                .id_salt(("asset_lab_basic_details", asset.id))
                .max_height(details_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    kit::field_label(ui, "Asset Details");
                    ui.add_space(kit::FORM_ROW_GAP);
                    asset_lab_meta_row(ui, "Name", asset_display_name(asset));
                    asset_lab_meta_row(ui, "Type", asset_kind_label(&asset.kind));
                    if let Some(dimensions) = self.asset_source_dimensions(asset) {
                        asset_lab_meta_row(
                            ui,
                            "Dimensions",
                            format!("{:.0} x {:.0}", dimensions.x, dimensions.y),
                        );
                    }
                    if asset.is_video() {
                        asset_lab_meta_row(
                            ui,
                            "Local Time",
                            timecode(self.asset_lab.local_time_seconds),
                        );
                        if media_duration > 0.0 {
                            asset_lab_meta_row(ui, "Duration", timecode(media_duration));
                        }
                    }
                    if let Some(source) = asset_source_label(asset) {
                        asset_lab_meta_row(ui, "Source", source);
                    }
                    if let Some(path) = media_path.as_ref() {
                        asset_lab_meta_row(ui, "File", path_label(path));
                    }

                    ui.add_space(kit::ACTION_GAP);
                    self.asset_lab_basic_action_rows(ui, action);
                });
        });
    }

    pub(super) fn asset_lab_basic_action_rows(
        &mut self,
        ui: &mut Ui,
        action: &mut Option<AssetLabAction>,
    ) {
        kit::equal_width_action_row(
            ui,
            2,
            kit::SECONDARY_BUTTON_H,
            kit::FIELD_COMPOUND_GAP,
            |ui, index, width| match index {
                0 => {
                    if kit::secondary_button(ui, "Duplicate Asset", width).clicked() {
                        *action = Some(AssetLabAction::DuplicateAsset);
                    }
                }
                _ => {
                    if kit::secondary_button(ui, "Add to Timeline", width).clicked() {
                        *action = Some(AssetLabAction::AddAssetToTimeline);
                    }
                }
            },
        );
        ui.add_space(kit::FORM_ROW_GAP);
        if kit::secondary_button(ui, "Open Location", ui.available_width()).clicked() {
            *action = Some(AssetLabAction::OpenLocation);
        }
        ui.add_space(kit::FORM_ROW_GAP);
        if kit::danger_button(ui, "Delete Asset...", ui.available_width()).clicked() {
            *action = Some(AssetLabAction::RequestDeleteAsset);
        }
    }

    #[allow(dead_code)]
    pub(super) fn asset_lab_timeline_status_row(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        selected_version: Option<&str>,
        active_version: Option<&str>,
        action: &mut Option<AssetLabAction>,
    ) {
        let (asset_badge, asset_color) = asset_lab_type_badge(asset);
        ui.horizontal_wrapped(|ui| {
            kit::media_pill(ui, asset_badge, asset_color);
            match (selected_version, active_version) {
                (Some(selected), Some(active)) if selected == active => {
                    kit::media_pill(ui, "On Timeline", kit::PRIMARY);
                    ui.label(kit::caption("This selected output is visible on the timeline."));
                }
                (Some(_), _) => {
                    kit::media_pill(ui, "Lab Draft", kit::MARKER);
                    ui.label(kit::caption(
                        "Saved inside this asset. Timeline changes only when you choose Use on Timeline.",
                    ));
                }
                (None, Some(_)) => {
                    kit::media_pill(ui, "No Selection", kit::TEXT_MUTED);
                    ui.label(kit::caption("Select a lab output to preview or reuse it."));
                }
                (None, None) => {
                    kit::media_pill(ui, "No Output", kit::TEXT_MUTED);
                    ui.label(kit::caption("Run a step to create a lab output."));
                }
            }
        });

        ui.add_space(kit::FORM_ROW_GAP);
        kit::equal_width_action_row(
            ui,
            2,
            kit::SECONDARY_BUTTON_H,
            kit::FIELD_COMPOUND_GAP,
            |ui, index, width| match index {
                0 => {
                    let can_use = selected_version.is_some() && selected_version != active_version;
                    if can_use {
                        if kit::primary_button(ui, "Use on Timeline", width).clicked() {
                            if let Some(version) = selected_version {
                                *action = Some(AssetLabAction::SetActive(version.to_string()));
                            }
                        }
                    } else {
                        let label = if selected_version.is_some() {
                            "Already on Timeline"
                        } else {
                            "Use on Timeline"
                        };
                        ui.add_enabled_ui(false, |ui| {
                            kit::secondary_button(ui, label, width);
                        });
                    }
                }
                _ => {
                    ui.add_enabled_ui(selected_version.is_some(), |ui| {
                        if kit::secondary_button(ui, "Fork Step", width).clicked() {
                            if let Some(version) = selected_version {
                                *action = Some(AssetLabAction::CreateEditStepFromVersion(
                                    version.to_string(),
                                ));
                            }
                        }
                    });
                }
            },
        );
    }

    #[allow(dead_code)]
    pub(super) fn asset_lab_action_rows(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        selected_version: Option<&str>,
        active_version: Option<&str>,
        pending_delete: Option<&str>,
        action: &mut Option<AssetLabAction>,
    ) {
        let Some(version) = selected_version else {
            return;
        };

        if let Some(pending_version) = pending_delete {
            ui.label(
                RichText::new(format!(
                    "Delete {pending_version}? This removes its output file."
                ))
                .color(kit::DANGER)
                .strong(),
            );
            ui.add_space(kit::FORM_ROW_GAP);
            kit::equal_width_action_row(
                ui,
                2,
                kit::SECONDARY_BUTTON_H,
                kit::FIELD_COMPOUND_GAP,
                |ui, index, width| match index {
                    0 => {
                        if kit::secondary_button(ui, "Cancel", width).clicked() {
                            *action = Some(AssetLabAction::CancelDelete);
                        }
                    }
                    _ => {
                        if kit::danger_button(ui, "Delete Output", width).clicked() {
                            *action =
                                Some(AssetLabAction::ConfirmDelete(pending_version.to_string()));
                        }
                    }
                },
            );
            return;
        }

        kit::equal_width_action_row(
            ui,
            2,
            kit::SECONDARY_BUTTON_H,
            kit::FIELD_COMPOUND_GAP,
            |ui, index, width| match index {
                0 => {
                    if kit::secondary_button(ui, "Duplicate Output", width).clicked() {
                        *action = Some(AssetLabAction::DuplicateVersion(version.to_string()));
                    }
                }
                _ => {
                    if kit::secondary_button(ui, "Extract as Asset", width).clicked() {
                        *action = Some(AssetLabAction::ExtractVersion(version.to_string()));
                    }
                }
            },
        );
        ui.add_space(kit::FORM_ROW_GAP);
        kit::equal_width_action_row(
            ui,
            2,
            kit::SECONDARY_BUTTON_H,
            kit::FIELD_COMPOUND_GAP,
            |ui, index, width| match index {
                0 => {
                    if asset.is_video() {
                        if kit::secondary_button(ui, "Extract Frame", width).clicked() {
                            *action =
                                Some(AssetLabAction::ExtractCurrentFrame(version.to_string()));
                        }
                    } else if kit::secondary_button(ui, "Open Location", width).clicked() {
                        *action = Some(AssetLabAction::OpenLocation);
                    }
                }
                _ => {
                    if kit::secondary_button(ui, "Open Location", width).clicked() {
                        *action = Some(AssetLabAction::OpenLocation);
                    }
                }
            },
        );
        ui.add_space(kit::FORM_ROW_GAP);
        if kit::danger_button(ui, "Delete Output", ui.available_width()).clicked() {
            *action = Some(AssetLabAction::RequestDelete(version.to_string()));
        }

        if active_version != Some(version) {
            ui.add_space(kit::FORM_ROW_GAP);
            ui.label(kit::caption(
                "This lab draft is saved, but timeline clips keep using the current timeline output.",
            ));
        }
        if !asset.is_visual() {
            ui.add_space(kit::FORM_ROW_GAP);
            ui.label(kit::caption(
                "Non-visual assets do not show preview thumbnails yet.",
            ));
        }
    }

    pub(super) fn asset_lab_video_scrubber(&mut self, ui: &mut Ui, duration: f64, fps: f64) {
        let duration = duration.max(0.0);
        let fps = fps.max(1.0);

        if !ui.ctx().text_edit_focused() {
            let (left, right) = ui.input(|input| {
                (
                    input.key_pressed(egui::Key::ArrowLeft),
                    input.key_pressed(egui::Key::ArrowRight),
                )
            });
            if left {
                self.asset_lab.local_time_seconds =
                    previous_frame_time(self.asset_lab.local_time_seconds, fps).min(duration);
                self.asset_lab_preview_texture = None;
            }
            if right {
                self.asset_lab.local_time_seconds =
                    next_frame_time(self.asset_lab.local_time_seconds, duration, fps);
                self.asset_lab_preview_texture = None;
            }
        }

        ui.add_space(ASSET_LAB_PREVIEW_SCRUB_GAP);
        let desired_size = Vec2::new(ui.available_width(), 42.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());

        if duration > 0.0 && (response.clicked() || response.dragged()) {
            if let Some(pointer) = response.interact_pointer_pos() {
                let fraction = ((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
                let raw_time = duration * fraction as f64;
                let frame = frames_from_seconds(raw_time, fps).round();
                self.asset_lab.local_time_seconds = seconds_from_frames(frame, fps).min(duration);
                self.asset_lab_preview_texture = None;
            }
        }

        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, kit::field_radius(), kit::FIELD_BG);
        painter.rect_stroke(
            rect,
            kit::field_radius(),
            Stroke::new(1.0, kit::BORDER_SOFT),
            egui::StrokeKind::Inside,
        );

        let inner = rect.shrink2(Vec2::new(8.0, 7.0));
        if duration > 0.0 && inner.width() > 1.0 {
            let step = asset_lab_scrub_tick_step(duration, inner.width());
            let mut tick = 0.0;
            while tick <= duration + step * 0.5 {
                let x = inner.left() + inner.width() * (tick / duration).clamp(0.0, 1.0) as f32;
                painter.line_segment(
                    [
                        Pos2::new(x, inner.top() + 12.0),
                        Pos2::new(x, inner.bottom() - 6.0),
                    ],
                    Stroke::new(1.0, kit::BORDER_SOFT),
                );
                painter.text(
                    Pos2::new(x + 3.0, inner.top()),
                    egui::Align2::LEFT_TOP,
                    timecode(tick),
                    FontId::monospace(9.0),
                    kit::TEXT_DIM,
                );
                tick += step;
            }

            let current = self.asset_lab.local_time_seconds.min(duration);
            let x = inner.left() + inner.width() * (current / duration).clamp(0.0, 1.0) as f32;
            painter.line_segment(
                [Pos2::new(x, inner.top()), Pos2::new(x, inner.bottom())],
                Stroke::new(2.0, kit::PLAYHEAD),
            );
            painter.circle_filled(Pos2::new(x, inner.top()), 4.0, kit::PLAYHEAD);
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "00:00.00",
                FontId::monospace(10.0),
                kit::TEXT_DIM,
            );
        }
    }

    pub(super) fn handle_asset_lab_action(&mut self, asset_id: Uuid, action: AssetLabAction) {
        match action {
            AssetLabAction::SelectVersion(version) => {
                let selected_node_id =
                    self.editor
                        .project
                        .generative_config(asset_id)
                        .and_then(|config| {
                            config
                                .versions
                                .iter()
                                .find(|record| record.version == version)
                                .and_then(|record| record.lab_node_id)
                        });
                self.asset_lab.selected_version = Some(version);
                self.asset_lab.pending_delete_version = None;
                self.asset_lab_preview_texture = None;
                self.asset_lab.clear_draft();
                if let Some(node_id) = selected_node_id {
                    let updated =
                        self.editor
                            .project
                            .update_generative_config(asset_id, |config| {
                                config.lab_graph.selected_node_id = Some(node_id);
                            });
                    if updated {
                        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
                            self.editor.status = format!("Failed to save step selection: {err}");
                        }
                    }
                }
            }
            AssetLabAction::SetActive(version) => {
                let _ = self.set_generative_active_version(asset_id, &version);
                self.asset_lab.selected_version = Some(version);
                self.asset_lab.pending_delete_version = None;
                self.asset_lab.clear_draft();
            }
            AssetLabAction::DuplicateVersion(version) => {
                let _ = self.duplicate_generative_version(asset_id, &version);
                self.asset_lab.pending_delete_version = None;
            }
            AssetLabAction::ExtractVersion(version) => {
                match self
                    .editor
                    .extract_generation_version_as_asset(asset_id, Some(&version))
                {
                    Ok(new_asset_id) => self.warm_asset_thumbnails(&[new_asset_id]),
                    Err(err) => self.editor.status = err,
                }
            }
            AssetLabAction::ExtractCurrentFrame(version) => {
                self.extract_asset_lab_current_frame(asset_id, &version);
            }
            AssetLabAction::AddNode(provider_id) => {
                self.add_asset_lab_node(asset_id, provider_id);
            }
            AssetLabAction::SelectNode(node_id) => {
                self.select_asset_lab_node(asset_id, node_id);
            }
            AssetLabAction::ClearNodeSelection => {
                self.clear_asset_lab_node_selection(asset_id);
            }
            AssetLabAction::SetNodeProvider {
                node_id,
                provider_id,
            } => {
                self.set_asset_lab_node_provider(asset_id, node_id, provider_id);
            }
            AssetLabAction::UpdateNodeInput {
                node_id,
                input_name,
                value,
            } => {
                self.update_asset_lab_node_input(asset_id, node_id, input_name, Some(value));
            }
            AssetLabAction::ClearNodeInput {
                node_id,
                input_name,
            } => {
                self.update_asset_lab_node_input(asset_id, node_id, input_name, None);
            }
            AssetLabAction::GenerateNode(node_id) => {
                self.generate_asset_lab_node(asset_id, node_id);
            }
            AssetLabAction::CreateEditStepFromVersion(version) => {
                self.create_asset_lab_edit_step_from_version(asset_id, &version);
            }
            AssetLabAction::DeleteNode(node_id) => {
                let _ = self.delete_asset_lab_node(asset_id, node_id);
            }
            AssetLabAction::DuplicateAsset => match self.editor.duplicate_asset(asset_id) {
                Ok(new_asset_id) => self.warm_asset_thumbnails(&[new_asset_id]),
                Err(err) => self.editor.status = err,
            },
            AssetLabAction::AddAssetToTimeline => {
                match self.editor.add_asset_to_timeline(asset_id, None) {
                    Ok(_) => {
                        self.editor.status = "Added asset to timeline".to_string();
                    }
                    Err(err) => self.editor.status = err,
                }
            }
            AssetLabAction::RequestDeleteAsset => {
                self.close_asset_lab();
                self.request_delete_assets(&[asset_id]);
            }
            AssetLabAction::RequestDelete(version) => {
                self.asset_lab.pending_delete_version = Some(version);
            }
            AssetLabAction::ConfirmDelete(version) => {
                let _ = self.delete_generative_version(asset_id, &version);
                self.asset_lab.pending_delete_version = None;
            }
            AssetLabAction::CancelDelete => {
                self.asset_lab.pending_delete_version = None;
            }
            AssetLabAction::OpenLocation => {
                let result = self
                    .editor
                    .project
                    .project_path
                    .as_ref()
                    .and_then(|root| {
                        self.editor
                            .project
                            .find_asset(asset_id)
                            .and_then(|asset| asset_lab_location_path(root, asset))
                    })
                    .ok_or_else(|| "Project folder is unavailable.".to_string())
                    .and_then(|path| open_path_in_file_manager(&path));
                if let Err(err) = result {
                    self.editor.status = err;
                }
            }
        }
    }

    pub(super) fn ensure_asset_lab_graph_for_versions(&mut self, asset_id: Uuid) {
        let selected_version = self.asset_lab.selected_version.clone();
        let mut changed = false;
        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                let mut existing_node_ids: HashSet<Uuid> =
                    config.lab_graph.nodes.iter().map(|node| node.id).collect();
                let mut nodes_by_output: HashMap<String, Uuid> = config
                    .lab_graph
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        node.output_version
                            .as_ref()
                            .map(|version| (version.clone(), node.id))
                    })
                    .collect();

                let mut version_indices: Vec<usize> = (0..config.versions.len()).collect();
                version_indices.sort_by(|left, right| {
                    config.versions[*left]
                        .timestamp
                        .cmp(&config.versions[*right].timestamp)
                });
                let mut previous_node_id = None;

                for index in version_indices {
                    let version = config.versions[index].version.clone();
                    let inputs_snapshot = config.versions[index].inputs_snapshot.clone();
                    let valid_record_node = config.versions[index]
                        .lab_node_id
                        .is_some_and(|node_id| existing_node_ids.contains(&node_id));

                    if valid_record_node {
                        if let Some(node_id) = config.versions[index].lab_node_id {
                            nodes_by_output.entry(version.clone()).or_insert(node_id);
                            if let Some(parent_id) = previous_node_id {
                                if let Some(node) = config
                                    .lab_graph
                                    .nodes
                                    .iter_mut()
                                    .find(|node| node.id == node_id)
                                {
                                    let imported_root = node.parent_node_id.is_none()
                                        && node.output_version.as_deref() == Some(version.as_str())
                                        && node.inputs == inputs_snapshot;
                                    if imported_root {
                                        node.parent_node_id = Some(parent_id);
                                        changed = true;
                                    }
                                }
                            }
                            previous_node_id = Some(node_id);
                        }
                        continue;
                    }

                    if let Some(node_id) = nodes_by_output.get(&version).copied() {
                        config.versions[index].lab_node_id = Some(node_id);
                        previous_node_id = Some(node_id);
                        changed = true;
                        continue;
                    }

                    let mut node = AssetLabNode::new_with_parent(
                        Some(config.versions[index].provider_id),
                        previous_node_id,
                    );
                    node.inputs = inputs_snapshot;
                    node.output_version = Some(version.clone());
                    let node_id = node.id;
                    config.lab_graph.nodes.push(node);
                    config.versions[index].lab_node_id = Some(node_id);
                    existing_node_ids.insert(node_id);
                    nodes_by_output.insert(version, node_id);
                    previous_node_id = Some(node_id);
                    changed = true;
                }

                if config
                    .lab_graph
                    .selected_node_id
                    .is_some_and(|node_id| !existing_node_ids.contains(&node_id))
                {
                    config.lab_graph.selected_node_id = None;
                    changed = true;
                }

                let selected_node_id = selected_version.as_deref().and_then(|version| {
                    config
                        .versions
                        .iter()
                        .find(|record| record.version == version)
                        .and_then(|record| record.lab_node_id)
                });

                if let Some(node_id) = selected_node_id {
                    if config.lab_graph.selected_node_id != Some(node_id) {
                        config.lab_graph.selected_node_id = Some(node_id);
                        changed = true;
                    }
                } else if config.lab_graph.selected_node_id.is_none() {
                    config.lab_graph.selected_node_id =
                        config.lab_graph.nodes.first().map(|node| node.id);
                    changed |= config.lab_graph.selected_node_id.is_some();
                }
            });

        if !updated || !changed {
            return;
        }

        self.asset_lab.clear_draft();
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Failed to save Asset Lab graph: {err}");
        }
    }

    pub(super) fn add_asset_lab_node(&mut self, asset_id: Uuid, provider_id: Option<Uuid>) {
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.editor.status = "Asset not found.".to_string();
            return;
        };
        let provider_id = match provider_id {
            Some(provider_id) => {
                let Some(provider) = self
                    .editor
                    .provider_entries
                    .iter()
                    .find(|provider| provider.id == provider_id)
                else {
                    self.editor.status = "Provider is unavailable.".to_string();
                    return;
                };
                if !asset_lab_provider_is_compatible(&asset, provider) {
                    self.editor.status =
                        "Provider output type does not match this asset.".to_string();
                    return;
                }
                Some(provider_id)
            }
            None => None,
        };

        let node = AssetLabNode::new(provider_id);
        let node_id = node.id;
        if !self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                config.lab_graph.selected_node_id = Some(node_id);
                config.lab_graph.nodes.push(node);
            })
        {
            self.editor.status = "Asset does not support Asset Lab steps.".to_string();
            return;
        }
        self.asset_lab.clear_draft();
        self.save_asset_lab_config(asset_id, "Added step.");
    }

    pub(super) fn create_asset_lab_edit_step_from_version(
        &mut self,
        asset_id: Uuid,
        version: &str,
    ) {
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.editor.status = "Asset not found.".to_string();
            return;
        };
        let Some(config) = self.editor.project.generative_config(asset_id) else {
            self.editor.status = "Asset does not support Asset Lab steps.".to_string();
            return;
        };
        let Some(source_record) = config
            .versions
            .iter()
            .find(|record| record.version == version)
            .cloned()
        else {
            self.editor.status = format!("Output {version} was not found.");
            return;
        };

        let provider = self
            .editor
            .provider_entries
            .iter()
            .filter(|provider| asset_lab_provider_is_compatible(&asset, provider))
            .find(|provider| {
                provider.id == source_record.provider_id
                    && provider.inputs.iter().any(|input| {
                        asset_lab_generation_ref_for_input(&asset, input, version).is_some()
                    })
            })
            .or_else(|| {
                self.editor
                    .provider_entries
                    .iter()
                    .filter(|provider| asset_lab_provider_is_compatible(&asset, provider))
                    .find(|provider| {
                        provider.inputs.iter().any(|input| {
                            asset_lab_generation_ref_for_input(&asset, input, version).is_some()
                        })
                    })
            })
            .or_else(|| {
                self.editor
                    .provider_entries
                    .iter()
                    .find(|provider| provider.id == source_record.provider_id)
            })
            .filter(|provider| asset_lab_provider_is_compatible(&asset, provider))
            .or_else(|| {
                self.editor
                    .provider_entries
                    .iter()
                    .find(|provider| asset_lab_provider_is_compatible(&asset, provider))
            })
            .cloned();

        let mut node = AssetLabNode::new_with_parent(
            provider.as_ref().map(|provider| provider.id),
            source_record.lab_node_id,
        );
        node.inputs = generation_record_source_inputs(config, &source_record);
        if let Some(provider) = provider.as_ref() {
            retain_node_inputs_for_provider(&mut node.inputs, provider);
        }
        if let Some(provider) = provider.as_ref() {
            if let Some((input_name, value)) = provider.inputs.iter().find_map(|input| {
                asset_lab_generation_ref_for_input(&asset, input, version)
                    .map(|value| (input.name.clone(), value))
            }) {
                node.inputs.insert(input_name, value);
            }
        }
        let node_id = node.id;
        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                config.lab_graph.selected_node_id = Some(node_id);
                config.lab_graph.nodes.push(node);
            });
        if !updated {
            self.editor.status = "Asset does not support Asset Lab steps.".to_string();
            return;
        }
        self.asset_lab.clear_draft();
        self.asset_lab.selected_version = Some(version.to_string());
        self.save_asset_lab_config(
            asset_id,
            &format!("Created edit step from output {version}. Timeline unchanged."),
        );
    }

    pub(super) fn select_asset_lab_node(&mut self, asset_id: Uuid, node_id: Uuid) {
        let selected_version = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(|config| {
                config
                    .lab_graph
                    .nodes
                    .iter()
                    .find(|node| node.id == node_id)
                    .and_then(|node| node.output_version.clone())
                    .or_else(|| {
                        config
                            .versions
                            .iter()
                            .rev()
                            .find(|record| record.lab_node_id == Some(node_id))
                            .map(|record| record.version.clone())
                    })
            });
        if !self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                config.lab_graph.selected_node_id = Some(node_id);
            })
        {
            return;
        }
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Failed to save step selection: {err}");
        }
        if selected_version.is_some() {
            self.asset_lab.selected_version = selected_version;
            self.asset_lab_preview_texture = None;
        }
        self.asset_lab.clear_draft();
    }

    pub(super) fn clear_asset_lab_node_selection(&mut self, asset_id: Uuid) {
        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                config.lab_graph.selected_node_id = None;
            });
        if updated {
            if let Err(err) = self.editor.project.save_generative_config(asset_id) {
                self.editor.status = format!("Failed to clear step selection: {err}");
            }
        }
        self.asset_lab.pending_delete_version = None;
        self.asset_lab.clear_draft();
    }

    fn asset_lab_draft_baseline(
        &self,
        asset_id: Uuid,
        node_id: Uuid,
    ) -> Option<(String, Uuid, HashMap<String, InputValue>)> {
        let version = self.asset_lab.selected_version.as_deref()?;
        let config = self.editor.project.generative_config(asset_id)?;
        let node = config
            .lab_graph
            .nodes
            .iter()
            .find(|node| node.id == node_id)?;
        let record = config
            .versions
            .iter()
            .find(|record| record.version == version)?;
        if !asset_lab_record_is_node_baseline(record, node) {
            return None;
        }
        Some((
            record.version.clone(),
            record.provider_id,
            record.inputs_snapshot.clone(),
        ))
    }

    fn ensure_asset_lab_draft(&mut self, asset_id: Uuid, node_id: Uuid) -> bool {
        let Some((version, provider_id, inputs)) = self.asset_lab_draft_baseline(asset_id, node_id)
        else {
            return false;
        };
        if !self
            .asset_lab
            .draft_matches(node_id, Some(version.as_str()))
        {
            self.asset_lab.draft_source_node_id = Some(node_id);
            self.asset_lab.draft_base_version = Some(version);
            self.asset_lab.draft_provider_id = Some(provider_id);
            self.asset_lab.draft_inputs = inputs;
        }
        true
    }

    fn prune_clean_asset_lab_draft(&mut self, asset_id: Uuid, node_id: Uuid) {
        let Some((version, provider_id, inputs)) = self.asset_lab_draft_baseline(asset_id, node_id)
        else {
            return;
        };
        if self
            .asset_lab
            .draft_matches(node_id, Some(version.as_str()))
            && self.asset_lab.draft_provider_id == Some(provider_id)
            && self.asset_lab.draft_inputs == inputs
        {
            self.asset_lab.clear_draft();
        }
    }

    pub(super) fn set_asset_lab_node_provider(
        &mut self,
        asset_id: Uuid,
        node_id: Uuid,
        provider_id: Option<Uuid>,
    ) {
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.editor.status = "Asset not found.".to_string();
            return;
        };
        let provider: Option<ProviderEntry> = match provider_id {
            Some(provider_id) => {
                let Some(provider) = self
                    .editor
                    .provider_entries
                    .iter()
                    .find(|provider| provider.id == provider_id)
                else {
                    self.editor.status = "Provider is unavailable.".to_string();
                    return;
                };
                if !asset_lab_provider_is_compatible(&asset, provider) {
                    self.editor.status =
                        "Provider output type does not match this asset.".to_string();
                    return;
                }
                Some(provider.clone())
            }
            None => None,
        };

        if self.ensure_asset_lab_draft(asset_id, node_id) {
            self.asset_lab.draft_provider_id = provider_id;
            if let Some(provider) = provider.as_ref() {
                retain_node_inputs_for_provider(&mut self.asset_lab.draft_inputs, provider);
            } else {
                self.asset_lab.draft_inputs.clear();
            }
            self.prune_clean_asset_lab_draft(asset_id, node_id);
            self.editor.status = if self.asset_lab.draft_source_node_id.is_some() {
                "Staged variant.".to_string()
            } else {
                "Reverted to selected output.".to_string()
            };
            return;
        }

        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                if let Some(node) = config
                    .lab_graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == node_id)
                {
                    node.provider_id = provider_id;
                    if let Some(provider) = provider.as_ref() {
                        retain_node_inputs_for_provider(&mut node.inputs, provider);
                    } else {
                        node.inputs.clear();
                    }
                    config.lab_graph.selected_node_id = Some(node_id);
                }
            });
        if !updated {
            self.editor.status = "Asset does not support Asset Lab steps.".to_string();
            return;
        }
        self.save_asset_lab_config(asset_id, "Updated step provider.");
    }

    pub(super) fn update_asset_lab_node_input(
        &mut self,
        asset_id: Uuid,
        node_id: Uuid,
        input_name: String,
        value: Option<InputValue>,
    ) {
        if self.ensure_asset_lab_draft(asset_id, node_id) {
            match value {
                Some(value) => {
                    self.asset_lab.draft_inputs.insert(input_name, value);
                }
                None => {
                    self.asset_lab.draft_inputs.remove(&input_name);
                }
            }
            self.prune_clean_asset_lab_draft(asset_id, node_id);
            self.editor.status = if self.asset_lab.draft_source_node_id.is_some() {
                "Staged variant.".to_string()
            } else {
                "Reverted to selected output.".to_string()
            };
            return;
        }

        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                if let Some(node) = config
                    .lab_graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == node_id)
                {
                    match value {
                        Some(value) => {
                            node.inputs.insert(input_name.clone(), value);
                        }
                        None => {
                            node.inputs.remove(&input_name);
                        }
                    }
                    config.lab_graph.selected_node_id = Some(node_id);
                }
            });
        if !updated {
            self.editor.status = "Asset does not support Asset Lab steps.".to_string();
            return;
        }
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Failed to save step input: {err}");
        }
    }

    fn commit_asset_lab_draft(&mut self, asset_id: Uuid, source_node_id: Uuid) -> Option<Uuid> {
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.editor.status = "Asset not found.".to_string();
            return None;
        };
        let Some((base_version, _, _)) = self.asset_lab_draft_baseline(asset_id, source_node_id)
        else {
            self.editor.status = "Selected output is no longer available.".to_string();
            return None;
        };
        if !self
            .asset_lab
            .draft_matches(source_node_id, Some(base_version.as_str()))
        {
            self.editor.status = "No staged variant to generate.".to_string();
            return None;
        }
        let Some(provider_id) = self.asset_lab.draft_provider_id else {
            self.editor.status = "Select a provider for this variant first.".to_string();
            return None;
        };
        let Some(provider) = self
            .editor
            .provider_entries
            .iter()
            .find(|provider| provider.id == provider_id)
            .filter(|provider| asset_lab_provider_is_compatible(&asset, provider))
            .cloned()
        else {
            self.editor.status = "Selected provider is unavailable.".to_string();
            return None;
        };

        let mut node = AssetLabNode::new_with_parent(Some(provider.id), Some(source_node_id));
        node.inputs = self.asset_lab.draft_inputs.clone();
        retain_node_inputs_for_provider(&mut node.inputs, &provider);
        if let Some(input_name) = asset_lab_primary_media_input_name(&asset, &provider) {
            if !node.inputs.contains_key(input_name) {
                if let Some(input) = provider
                    .inputs
                    .iter()
                    .find(|input| input.name == input_name)
                {
                    if let Some(value) =
                        asset_lab_generation_ref_for_input(&asset, input, &base_version)
                    {
                        node.inputs.insert(input_name.to_string(), value);
                    }
                }
            }
        }

        let node_id = node.id;
        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                config.lab_graph.selected_node_id = Some(node_id);
                config.lab_graph.nodes.push(node);
            });
        if !updated {
            self.editor.status = "Asset does not support Asset Lab steps.".to_string();
            return None;
        }
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Failed to save staged variant: {err}");
            return None;
        }
        self.asset_lab.clear_draft();
        Some(node_id)
    }

    pub(super) fn generate_asset_lab_node(&mut self, asset_id: Uuid, node_id: Uuid) {
        if self.asset_lab.draft_source_node_id == Some(node_id) {
            if let Some(committed_node_id) = self.commit_asset_lab_draft(asset_id, node_id) {
                self.generate_asset_lab_node(asset_id, committed_node_id);
            }
            return;
        }

        let Some((folder, output_type)) =
            generative_output_for_asset(&self.editor.project, asset_id)
        else {
            self.editor.status = "Asset does not support generation.".to_string();
            return;
        };
        let Some(project_root) = self.editor.project.project_path.clone() else {
            self.editor.status = "Project folder is unavailable.".to_string();
            return;
        };
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.editor.status = "Asset not found.".to_string();
            return;
        };
        let asset_label = asset.name.clone();
        let config_snapshot = self
            .editor
            .project
            .generative_config(asset_id)
            .cloned()
            .unwrap_or_default();
        let Some(node) = config_snapshot
            .lab_graph
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .cloned()
        else {
            self.editor.status = "Step was not found.".to_string();
            return;
        };
        let Some(provider_id) = node.provider_id else {
            self.editor.status = "Select a provider for this step first.".to_string();
            return;
        };
        let Some(provider) = self
            .editor
            .provider_entries
            .iter()
            .find(|provider| provider.id == provider_id)
            .cloned()
        else {
            self.editor.status = "Selected provider is unavailable.".to_string();
            return;
        };
        if provider.output_type != output_type {
            self.editor.status = "Provider output type does not match this asset.".to_string();
            return;
        }

        let parent_output_version = node.parent_node_id.and_then(|parent_id| {
            config_snapshot
                .lab_graph
                .nodes
                .iter()
                .find(|candidate| candidate.id == parent_id)
                .and_then(|parent| parent.output_version.clone())
        });
        if node.parent_node_id.is_some() && parent_output_version.is_none() {
            self.editor.status = "Generate the parent step first.".to_string();
            return;
        }

        let mut node_config = config_snapshot;
        node_config.provider_id = Some(provider.id);
        node_config.inputs = node.inputs.clone();
        if let Some(parent_version) = parent_output_version.as_deref() {
            if let Some(input_name) = asset_lab_primary_media_input_name(&asset, &provider) {
                if !node_config.inputs.contains_key(input_name) {
                    if let Some(value) = asset_lab_generation_ref_for_input(
                        &asset,
                        provider
                            .inputs
                            .iter()
                            .find(|input| input.name == input_name)
                            .expect("primary media input should exist"),
                        parent_version,
                    ) {
                        node_config.inputs.insert(input_name.to_string(), value);
                    }
                }
            }
        }
        node_config.lab_graph.selected_node_id = Some(node_id);
        let folder_path = project_root.join(folder);
        match self.enqueue_generation_jobs(
            asset_id,
            None,
            Some(node_id),
            provider,
            node_config,
            folder_path,
            asset_label,
        ) {
            Ok(status) => {
                self.editor.status = format!("{status} from Asset Lab step.");
            }
            Err(err) => self.editor.status = err,
        }
    }

    pub(super) fn delete_asset_lab_node(
        &mut self,
        asset_id: Uuid,
        node_id: Uuid,
    ) -> Result<(), String> {
        if self.asset_lab.draft_source_node_id == Some(node_id) {
            self.asset_lab.clear_draft();
        }
        let Some(config_snapshot) = self.editor.project.generative_config(asset_id).cloned() else {
            let message = "Asset does not support Asset Lab steps.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        };
        let Some(parent_node_id) = config_snapshot
            .lab_graph
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .map(|node| node.parent_node_id)
        else {
            let message = "Asset Lab node not found.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        };
        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                config.lab_graph.nodes.retain(|node| node.id != node_id);
                for child in config
                    .lab_graph
                    .nodes
                    .iter_mut()
                    .filter(|node| node.parent_node_id == Some(node_id))
                {
                    child.parent_node_id = parent_node_id;
                }
                for record in config
                    .versions
                    .iter_mut()
                    .filter(|record| record.lab_node_id == Some(node_id))
                {
                    record.lab_node_id = parent_node_id;
                }
                if config.lab_graph.selected_node_id == Some(node_id) {
                    config.lab_graph.selected_node_id = config
                        .lab_graph
                        .nodes
                        .iter()
                        .find(|node| node.parent_node_id.is_none())
                        .map(|node| node.id)
                        .or_else(|| config.lab_graph.nodes.first().map(|node| node.id));
                }
                config.normalize_lab_graph_lineage();
            });
        if !updated {
            let message = "Asset does not support Asset Lab steps.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        }
        self.save_asset_lab_config_result(asset_id, "Deleted step. Outputs were kept.")
    }

    pub(super) fn save_asset_lab_config(&mut self, asset_id: Uuid, status: &str) {
        let _ = self.save_asset_lab_config_result(asset_id, status);
    }

    pub(super) fn save_asset_lab_config_result(
        &mut self,
        asset_id: Uuid,
        status: &str,
    ) -> Result<(), String> {
        match self.editor.project.save_generative_config(asset_id) {
            Ok(_) => {
                self.editor.status = status.to_string();
                Ok(())
            }
            Err(err) => {
                let message = format!("Failed to save Asset Lab config: {err}");
                self.editor.status = message.clone();
                Err(message)
            }
        }
    }

    pub(super) fn extract_asset_lab_current_frame(&mut self, asset_id: Uuid, version: &str) {
        let Some(project_root) = self.editor.project.project_path.clone() else {
            self.editor.status = "Project folder is unavailable.".to_string();
            return;
        };
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.editor.status = "Asset not found.".to_string();
            return;
        };
        if !asset.is_video() {
            self.editor.status =
                "Current-frame extraction is only available for video assets.".to_string();
            return;
        }
        let Some(path) = generative_output_file_for_version(&project_root, &asset, Some(version))
        else {
            self.editor.status = format!("No output file was found for {version}.");
            return;
        };

        let fps = asset_lab_video_fps(&asset, self.editor.project.settings.fps);
        let duration = probe_duration_seconds(&path)
            .or(asset.duration_seconds)
            .unwrap_or(0.0)
            .max(0.0);
        let frame_time = if duration > 0.0 {
            self.asset_lab.local_time_seconds.min(duration)
        } else {
            self.asset_lab.local_time_seconds.max(0.0)
        };
        let frame_index = frames_from_seconds(frame_time, fps).round().max(0.0);
        let snapped_time = seconds_from_frames(frame_index, fps);

        let Some(response) = self.asset_lab_video_decoder.decode(
            &path,
            snapped_time,
            0,
            self.editor.layout.hardware_decode,
        ) else {
            self.editor.status = "Could not decode current video frame.".to_string();
            return;
        };
        let Some(frame) = response.image else {
            self.editor.status = "Could not decode current video frame.".to_string();
            return;
        };

        match self
            .editor
            .add_extracted_frame_asset(asset_id, Some(version), snapped_time, frame)
        {
            Ok(new_asset_id) => self.warm_asset_thumbnails(&[new_asset_id]),
            Err(err) => self.editor.status = err,
        }
    }

    pub(super) fn set_generative_active_version(
        &mut self,
        asset_id: Uuid,
        version: &str,
    ) -> Result<(), String> {
        let config_snapshot = self.editor.project.generative_config(asset_id).cloned();
        let Some(record) = config_snapshot
            .as_ref()
            .and_then(|config| {
                config
                    .versions
                    .iter()
                    .find(|record| record.version == version)
            })
            .cloned()
        else {
            let message = format!("Version {version} was not found.");
            self.editor.status = message.clone();
            return Err(message);
        };

        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                config.active_version = Some(version.to_string());
                config.provider_id = Some(record.provider_id);
                config.inputs = record.inputs_snapshot;
                if let Some(node_id) = record.lab_node_id {
                    config.lab_graph.selected_node_id = Some(node_id);
                    if let Some(node) = config
                        .lab_graph
                        .nodes
                        .iter_mut()
                        .find(|node| node.id == node_id)
                    {
                        node.output_version = Some(version.to_string());
                    }
                }
            });
        if !updated {
            let message = "Generative asset not found.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        }
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            let message = format!("Failed to save active version: {err}");
            self.editor.status = message.clone();
            return Err(message);
        }
        self.invalidate_generative_asset_runtime(asset_id);
        self.editor.status = format!("Set {version} active.");
        Ok(())
    }

    pub(super) fn duplicate_generative_version(
        &mut self,
        asset_id: Uuid,
        version: &str,
    ) -> Result<String, String> {
        let Some(project_root) = self.editor.project.project_path.clone() else {
            let message = "Project folder is unavailable.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        };
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            let message = "Asset not found.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        };
        let Some(folder) = generative_folder_for_asset(&asset).cloned() else {
            let message = "Asset has no generation folder.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        };
        let Some(config_snapshot) = self.editor.project.generative_config(asset_id).cloned() else {
            let message = "Generation config was not found.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        };
        let Some(source_record) = config_snapshot
            .versions
            .iter()
            .find(|record| record.version == version)
            .cloned()
        else {
            let message = format!("Version {version} was not found.");
            self.editor.status = message.clone();
            return Err(message);
        };
        let new_version = next_version_label(&config_snapshot);
        let folder_path = project_root.join(&folder);
        if let Err(err) = copy_generative_version_files(&folder_path, version, &new_version) {
            self.editor.status = err.clone();
            return Err(err);
        }

        let new_record = GenerationRecord {
            version: new_version.clone(),
            timestamp: chrono::Utc::now(),
            provider_id: source_record.provider_id,
            inputs_snapshot: source_record.inputs_snapshot,
            lab_node_id: source_record.lab_node_id,
        };
        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                config.active_version = Some(new_version.clone());
                config.provider_id = Some(new_record.provider_id);
                config.inputs = new_record.inputs_snapshot.clone();
                if let Some(node_id) = new_record.lab_node_id {
                    config.lab_graph.selected_node_id = Some(node_id);
                    if let Some(node) = config
                        .lab_graph
                        .nodes
                        .iter_mut()
                        .find(|node| node.id == node_id)
                    {
                        node.output_version = Some(new_version.clone());
                    }
                }
                config.versions.push(new_record);
            });
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            let message = format!("Duplicated version, but config save failed: {err}");
            self.editor.status = message.clone();
            return Err(message);
        }
        self.asset_lab.selected_version = Some(new_version.clone());
        self.invalidate_generative_asset_runtime(asset_id);
        self.editor.status = format!("Duplicated {version} as {new_version}.");
        Ok(new_version)
    }

    pub(super) fn delete_generative_version(
        &mut self,
        asset_id: Uuid,
        version: &str,
    ) -> Result<(), String> {
        let Some(project_root) = self.editor.project.project_path.clone() else {
            let message = "Project folder is unavailable.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        };
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            let message = "Asset not found.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        };
        let Some(folder) = generative_folder_for_asset(&asset).cloned() else {
            let message = "Asset has no generation folder.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        };
        let Some(config_snapshot) = self.editor.project.generative_config(asset_id).cloned() else {
            let message = "Generation config was not found.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        };
        if !config_snapshot
            .versions
            .iter()
            .any(|record| record.version == version)
        {
            let message = format!("Version {version} was not found.");
            self.editor.status = message.clone();
            return Err(message);
        };
        let dependents = generation_version_dependents(&self.editor.project, asset_id, version);
        if !dependents.is_empty() {
            let message = format!(
                "Version {version} is referenced by other generative assets: {}.",
                dependents.join(", ")
            );
            self.editor.status = message.clone();
            return Err(message);
        }
        if let Err(err) = delete_generative_version_files(&project_root.join(folder), version) {
            let message = format!("Failed to delete version files: {err}");
            self.editor.status = message.clone();
            return Err(message);
        }

        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                config.versions.retain(|record| record.version != version);
                config.inputs.retain(|_, input| {
                    !matches!(
                        input,
                        InputValue::GenerationRef {
                            asset_id: ref_asset_id,
                            version: ref_version,
                            ..
                        } if *ref_asset_id == asset_id && ref_version == version
                    )
                });
                config.reference_slots.retain(|_, input| {
                    !matches!(
                        input,
                        InputValue::GenerationRef {
                            asset_id: ref_asset_id,
                            version: ref_version,
                            ..
                        } if *ref_asset_id == asset_id && ref_version == version
                    )
                });
                for node in config.lab_graph.nodes.iter_mut() {
                    if node.output_version.as_deref() == Some(version) {
                        node.output_version = None;
                    }
                    node.inputs.retain(|_, input| {
                        !matches!(
                            input,
                            InputValue::GenerationRef {
                                asset_id: ref_asset_id,
                                version: ref_version,
                                ..
                            } if *ref_asset_id == asset_id && ref_version == version
                        )
                    });
                }
                if config.active_version.as_deref() == Some(version) {
                    config.active_version = preferred_asset_lab_version(config);
                    if let Some(active) = config.active_version.clone() {
                        if let Some(record) = config
                            .versions
                            .iter()
                            .find(|record| record.version == active)
                            .cloned()
                        {
                            config.provider_id = Some(record.provider_id);
                            config.inputs = record.inputs_snapshot;
                        }
                    }
                }
            });
        if !updated {
            let message = "Generative asset not found.".to_string();
            self.editor.status = message.clone();
            return Err(message);
        }
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            let message = format!("Deleted version, but config save failed: {err}");
            self.editor.status = message.clone();
            return Err(message);
        }
        let next_selected = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(preferred_asset_lab_version);
        self.asset_lab.selected_version = next_selected;
        self.invalidate_generative_asset_runtime(asset_id);
        self.editor.status = format!("Deleted {version}.");
        Ok(())
    }

    pub(super) fn invalidate_generative_asset_runtime(&mut self, asset_id: Uuid) {
        if let (Some(project_root), Some(asset)) = (
            self.editor.project.project_path.as_ref(),
            self.editor.project.find_asset(asset_id),
        ) {
            if let Some(folder) = generative_folder_for_asset(asset) {
                self.editor
                    .previewer
                    .invalidate_folder(&project_root.join(folder));
            }
        }
        self.invalidate_preview_render_jobs();
        self.preview_prefetch_in_flight
            .store(false, Ordering::Relaxed);
        self.preview_idle_prefetched_time = None;
        self.invalidate_asset_visual_cache(asset_id);
        self.asset_lab_preview_texture = None;
        self.asset_lab_node_preview_textures
            .retain(|key, _| key.asset_id != asset_id);
        self.editor.preview_dirty = true;
    }

    pub(super) fn asset_lab_node_preview_texture(
        &mut self,
        ctx: &Context,
        asset: &Asset,
        version: Option<&str>,
        local_time_seconds: f64,
    ) -> Option<(TextureId, Vec2)> {
        let project_root = self.editor.project.project_path.as_ref()?;
        let path = asset_lab_media_path(project_root, asset, version)?;
        if !asset.is_visual() {
            return None;
        }

        let fps = asset_lab_video_fps(asset, self.editor.project.settings.fps);
        let frame_index = asset.is_video().then(|| {
            frames_from_seconds(local_time_seconds.max(0.0), fps)
                .round()
                .max(0.0) as i64
        });
        let key = AssetLabNodePreviewKey {
            asset_id: asset.id,
            version: version.map(str::to_string),
            frame_index,
        };

        if let Some(cached) = self.asset_lab_node_preview_textures.get(&key) {
            if cached.path == path {
                return Some((cached.texture.id(), cached.size));
            }
        }

        let (image, size) = if asset.is_video() {
            let frame_time = seconds_from_frames(frame_index.unwrap_or(0) as f64, fps);
            let response = self.asset_lab_video_decoder.decode(
                &path,
                frame_time,
                0,
                self.editor.layout.hardware_decode,
            )?;
            let frame = response.image?;
            let size = Vec2::new(frame.width().max(1) as f32, frame.height().max(1) as f32);
            let image = ColorImage::from_rgba_unmultiplied(
                [frame.width() as usize, frame.height() as usize],
                frame.as_raw(),
            );
            (image, size)
        } else {
            load_preview_image(&path, 384)?
        };

        if self.asset_lab_node_preview_textures.len() > 128 {
            self.asset_lab_node_preview_textures.clear();
        }

        let texture = ctx.load_texture(
            format!(
                "asset-lab-node-preview-{}-{}-{}",
                asset.id,
                version.unwrap_or("fallback"),
                frame_index.unwrap_or(-1)
            ),
            image,
            TextureOptions::LINEAR,
        );
        let texture_id = texture.id();
        self.asset_lab_node_preview_textures.insert(
            key,
            AssetLabPreviewTexture {
                asset_id: asset.id,
                version: version.map(str::to_string),
                path,
                frame_index,
                texture,
                size,
            },
        );
        Some((texture_id, size))
    }

    pub(super) fn asset_lab_preview_texture(
        &mut self,
        ctx: &Context,
        asset: &Asset,
        version: Option<&str>,
    ) -> Option<(TextureId, Vec2)> {
        let project_root = self.editor.project.project_path.as_ref()?;
        let path = asset_lab_media_path(project_root, asset, version)?;
        if !asset.is_visual() {
            return None;
        }
        let fps = asset_lab_video_fps(asset, self.editor.project.settings.fps);
        let frame_index = asset.is_video().then(|| {
            frames_from_seconds(self.asset_lab.local_time_seconds.max(0.0), fps)
                .round()
                .max(0.0) as i64
        });

        if let Some(cached) = self.asset_lab_preview_texture.as_ref() {
            if cached.asset_id == asset.id
                && cached.version.as_deref() == version
                && cached.path == path
                && cached.frame_index == frame_index
            {
                return Some((cached.texture.id(), cached.size));
            }
        }

        let (image, size) = if asset.is_video() {
            let frame_time = seconds_from_frames(frame_index.unwrap_or(0) as f64, fps);
            let response = self.asset_lab_video_decoder.decode(
                &path,
                frame_time,
                0,
                self.editor.layout.hardware_decode,
            )?;
            let frame = response.image?;
            let size = Vec2::new(frame.width().max(1) as f32, frame.height().max(1) as f32);
            let image = ColorImage::from_rgba_unmultiplied(
                [frame.width() as usize, frame.height() as usize],
                frame.as_raw(),
            );
            (image, size)
        } else {
            load_preview_image(&path, 512)?
        };
        let texture = ctx.load_texture(
            format!(
                "asset-lab-preview-{}-{}-{}",
                asset.id,
                version.unwrap_or("fallback"),
                frame_index.unwrap_or(-1)
            ),
            image,
            TextureOptions::LINEAR,
        );
        let texture_id = texture.id();
        self.asset_lab_preview_texture = Some(AssetLabPreviewTexture {
            asset_id: asset.id,
            version: version.map(str::to_string),
            path,
            frame_index,
            texture,
            size,
        });
        Some((texture_id, size))
    }
}
