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
        }
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

const ASSET_LAB_NODE_CARD_W: f32 = 188.0;
const ASSET_LAB_NODE_CARD_H: f32 = 88.0;
const ASSET_LAB_NODE_GAP: f32 = 34.0;
const ASSET_LAB_SOURCE_W: f32 = 92.0;
const ASSET_LAB_SOURCE_H: f32 = 26.0;

#[derive(Clone, Debug)]
pub(super) enum AssetLabAction {
    SelectVersion(String),
    SetActive(String),
    DuplicateVersion(String),
    ExtractVersion(String),
    ExtractCurrentFrame(String),
    AddNode(Option<Uuid>),
    SelectNode(Uuid),
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
    let input_names: HashSet<&str> = provider
        .inputs
        .iter()
        .map(|input| input.name.as_str())
        .collect();
    inputs.retain(|name, _| input_names.contains(name.as_str()));
}

fn retain_literal_node_inputs(inputs: &mut HashMap<String, InputValue>) {
    inputs.retain(|_, value| matches!(value, InputValue::Literal { .. }));
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
        };
        self.asset_lab_preview_texture = None;
        self.editor.overlays.asset_lab = true;
    }

    pub(super) fn close_asset_lab(&mut self) {
        self.editor.overlays.asset_lab = false;
        self.asset_lab = AssetLabState::default();
        self.asset_lab_preview_texture = None;
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
            .or_else(|| {
                config.and_then(|config| config.lab_graph.nodes.first().map(|node| node.id))
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
            .size(Size::exact(330.0))
            .size(Size::remainder().at_least(360.0))
            .size(Size::exact(330.0))
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
                    kit::card_frame().show(ui, |ui| {
                        let selected_output_path =
                            selected_version.as_deref().and_then(|version| {
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
                        let selected_video_fps =
                            asset_lab_video_fps(asset, self.editor.project.settings.fps);

                        let preview_timecode = (asset.is_video() && selected_output_path.is_some())
                            .then(|| {
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
                        let preview = self.asset_lab_preview_texture(
                            ui.ctx(),
                            asset,
                            selected_version.as_deref(),
                        );
                        asset_lab_preview(ui, asset, preview, &mut self.asset_lab);
                        if asset.is_video() && selected_output_path.is_some() {
                            self.asset_lab_video_scrubber(
                                ui,
                                selected_video_duration,
                                selected_video_fps,
                            );
                        }

                        ui.add_space(kit::ACTION_GAP);
                        self.asset_lab_timeline_status_row(
                            ui,
                            asset,
                            selected_version.as_deref(),
                            active_version.as_deref(),
                            action,
                        );

                        ui.add_space(kit::ACTION_GAP);
                        self.asset_lab_outputs_section(
                            ui,
                            asset,
                            &versions,
                            selected_version.as_deref(),
                            active_version.as_deref(),
                            action,
                        );

                        ui.add_space(kit::ACTION_GAP);
                        let details_height = ui.available_height().max(80.0);
                        egui::ScrollArea::vertical()
                            .id_salt(("asset_lab_details", asset.id))
                            .max_height(details_height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                kit::field_label(ui, "Selected Output");
                                ui.add_space(kit::FORM_ROW_GAP);
                                if let Some(version) = selected_version.as_deref() {
                                    asset_lab_meta_row(ui, "Output", version);
                                    if asset.is_video() {
                                        asset_lab_meta_row(
                                            ui,
                                            "Local Time",
                                            timecode(self.asset_lab.local_time_seconds),
                                        );
                                    }
                                    if let Some(record) = config.and_then(|config| {
                                        config
                                            .versions
                                            .iter()
                                            .find(|record| record.version == version)
                                    }) {
                                        asset_lab_meta_row(
                                            ui,
                                            "Created",
                                            record
                                                .timestamp
                                                .with_timezone(&chrono::Local)
                                                .format("%Y-%m-%d %H:%M:%S")
                                                .to_string(),
                                        );
                                        asset_lab_meta_row(
                                            ui,
                                            "Provider",
                                            asset_lab_provider_name(
                                                &self.editor.provider_entries,
                                                record.provider_id,
                                            ),
                                        );
                                        asset_lab_meta_row(
                                            ui,
                                            "Inputs",
                                            format!("{} captured", record.inputs_snapshot.len()),
                                        );
                                    }
                                    if let Some(path) = selected_output_path.as_ref() {
                                        asset_lab_meta_row(ui, "File", path_label(path));
                                    }
                                } else {
                                    ui.label(kit::caption(
                                        "Run a step or select a lab output to inspect it.",
                                    ));
                                }

                                ui.add_space(kit::ACTION_GAP);
                                self.asset_lab_action_rows(
                                    ui,
                                    asset,
                                    selected_version.as_deref(),
                                    active_version.as_deref(),
                                    pending_delete.as_deref(),
                                    action,
                                );
                            });
                    });
                });
                strip.cell(|ui| {
                    self.asset_lab_node_inspector(
                        ui,
                        asset,
                        config,
                        &versions,
                        selected_node_id,
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
                    if kit::secondary_button(ui, "+ Step", 82.0).clicked() {
                        let provider_id = compatible_providers.first().map(|provider| provider.id);
                        *action = Some(AssetLabAction::AddNode(provider_id));
                    }
                });
            });
            ui.add_space(kit::FORM_ROW_GAP);
            ui.add(
                egui::Label::new(kit::caption(
                    "Steps capture provider settings and lineage. Outputs are selected in the preview panel.",
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
                action,
            );
        });
    }

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

    pub(super) fn asset_lab_flow_canvas(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: Option<&GenerativeConfig>,
        versions: &[GenerationRecord],
        selected_node_id: Option<Uuid>,
        active_version: Option<&str>,
        action: &mut Option<AssetLabAction>,
    ) {
        let nodes = config
            .map(|config| config.lab_graph.nodes.as_slice())
            .unwrap_or(&[]);
        let available = ui.available_size();
        let viewport_h = available.y.max(220.0);

        if nodes.is_empty() {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(available.x, viewport_h), Sense::hover());
            ui.painter()
                .rect_filled(rect, kit::field_radius(), kit::FIELD_BG);
            ui.painter().rect_stroke(
                rect,
                kit::field_radius(),
                Stroke::new(1.0, kit::BORDER_SOFT),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center_top() + Vec2::new(0.0, 72.0),
                egui::Align2::CENTER_CENTER,
                "Add a generation step",
                FontId::proportional(13.0),
                kit::TEXT_MUTED,
            );
            ui.painter().text(
                rect.center_top() + Vec2::new(0.0, 94.0),
                egui::Align2::CENTER_CENTER,
                "Run it to create lab outputs without changing the timeline.",
                FontId::proportional(11.0),
                kit::TEXT_DIM,
            );
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt(("asset_lab_flow_canvas", asset.id))
            .max_height(viewport_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let row_count = nodes.len().max(versions.len()).max(1);
                let content_h = 18.0 + row_count as f32 * (ASSET_LAB_NODE_CARD_H + 16.0);
                let (rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), content_h),
                    Sense::hover(),
                );
                let painter = ui.painter().with_clip_rect(rect);
                painter.rect_filled(rect, kit::field_radius(), kit::FIELD_BG);
                painter.rect_stroke(
                    rect,
                    kit::field_radius(),
                    Stroke::new(1.0, kit::BORDER_SOFT),
                    egui::StrokeKind::Inside,
                );

                let source_x = rect.left() + 12.0;
                let node_x = rect.right() - ASSET_LAB_NODE_CARD_W - 12.0;
                let source_rects: Vec<(String, Rect)> = versions
                    .iter()
                    .enumerate()
                    .map(|(index, record)| {
                        let y = rect.top() + 16.0 + index as f32 * (ASSET_LAB_SOURCE_H + 10.0);
                        (
                            record.version.clone(),
                            Rect::from_min_size(
                                Pos2::new(source_x, y),
                                Vec2::new(ASSET_LAB_SOURCE_W, ASSET_LAB_SOURCE_H),
                            ),
                        )
                    })
                    .collect();
                let node_rects: Vec<(Uuid, Rect)> = nodes
                    .iter()
                    .enumerate()
                    .map(|(index, node)| {
                        let y = rect.top()
                            + 16.0
                            + index as f32 * (ASSET_LAB_NODE_CARD_H + ASSET_LAB_NODE_GAP);
                        (
                            node.id,
                            Rect::from_min_size(
                                Pos2::new(node_x.max(source_x + ASSET_LAB_SOURCE_W + 20.0), y),
                                Vec2::new(ASSET_LAB_NODE_CARD_W, ASSET_LAB_NODE_CARD_H),
                            ),
                        )
                    })
                    .collect();

                for node in nodes {
                    let Some((_, node_rect)) =
                        node_rects.iter().find(|(node_id, _)| *node_id == node.id)
                    else {
                        continue;
                    };
                    for input in node.inputs.values() {
                        let InputValue::GenerationRef { version, .. } = input else {
                            continue;
                        };
                        let Some((_, source_rect)) = source_rects
                            .iter()
                            .find(|(source_version, _)| source_version == version)
                        else {
                            continue;
                        };
                        let start = source_rect.right_center();
                        let end = node_rect.left_center();
                        let mid_x = start.x + (end.x - start.x) * 0.5;
                        let stroke = Stroke::new(1.35, kit::PRIMARY.gamma_multiply(0.82));
                        painter.line_segment([start, Pos2::new(mid_x, start.y)], stroke);
                        painter.line_segment(
                            [Pos2::new(mid_x, start.y), Pos2::new(mid_x, end.y)],
                            stroke,
                        );
                        painter.line_segment([Pos2::new(mid_x, end.y), end], stroke);
                    }
                }

                for (version, source_rect) in source_rects {
                    let active = active_version == Some(version.as_str());
                    let fill = if active {
                        kit::PRIMARY.gamma_multiply(0.16)
                    } else {
                        Color32::from_rgb(22, 24, 27)
                    };
                    painter.rect_filled(source_rect, kit::field_radius(), fill);
                    painter.rect_stroke(
                        source_rect,
                        kit::field_radius(),
                        Stroke::new(
                            1.0,
                            if active {
                                kit::PRIMARY
                            } else {
                                kit::BORDER_SOFT
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    painter.text(
                        source_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("Output {version}"),
                        FontId::proportional(11.0),
                        if active {
                            kit::PRIMARY_HOVER
                        } else {
                            kit::TEXT_MUTED
                        },
                    );
                }

                for (node_index, node) in nodes.iter().enumerate() {
                    let Some((_, node_rect)) =
                        node_rects.iter().find(|(node_id, _)| *node_id == node.id)
                    else {
                        continue;
                    };
                    let provider_label = node
                        .provider_id
                        .map(|id| asset_lab_provider_name(&self.editor.provider_entries, id))
                        .unwrap_or_else(|| "Select provider".to_string());
                    let response = crate::core::automation::instrument_response(
                        ui.interact(
                            *node_rect,
                            ui.id().with(("asset_lab_node", node.id)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand),
                        "step",
                        Some(provider_label.clone()),
                        true,
                        false,
                    );
                    if response.clicked() {
                        *action = Some(AssetLabAction::SelectNode(node.id));
                    }
                    let selected = selected_node_id == Some(node.id);
                    let output_count = versions
                        .iter()
                        .filter(|record| record.lab_node_id == Some(node.id))
                        .count();
                    let active_output = node
                        .output_version
                        .as_deref()
                        .is_some_and(|version| active_version == Some(version));
                    let fill = if selected {
                        Color32::from_rgb(28, 44, 38)
                    } else if response.hovered() {
                        Color32::from_rgb(27, 29, 33)
                    } else {
                        Color32::from_rgb(21, 23, 26)
                    };
                    painter.rect_filled(*node_rect, kit::field_radius(), fill);
                    painter.rect_stroke(
                        *node_rect,
                        kit::field_radius(),
                        Stroke::new(
                            1.0,
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
                        Rect::from_min_size(
                            node_rect.left_top(),
                            Vec2::new(4.0, node_rect.height()),
                        ),
                        kit::field_radius(),
                        asset_accent(asset),
                    );
                    painter.text(
                        node_rect.left_top() + Vec2::new(12.0, 12.0),
                        egui::Align2::LEFT_TOP,
                        format!("Step {} - {}", node_index + 1, provider_label),
                        FontId::proportional(12.0),
                        kit::TEXT,
                    );
                    let output = node
                        .output_version
                        .as_deref()
                        .map(|version| {
                            if active_output {
                                format!("Output {version} on timeline")
                            } else {
                                format!("Latest output {version}")
                            }
                        })
                        .unwrap_or_else(|| "Run step to create output".to_string());
                    painter.text(
                        node_rect.left_top() + Vec2::new(12.0, 36.0),
                        egui::Align2::LEFT_TOP,
                        output,
                        FontId::proportional(11.0),
                        if active_output {
                            kit::PRIMARY_HOVER
                        } else {
                            kit::TEXT_MUTED
                        },
                    );
                    painter.text(
                        node_rect.left_bottom() + Vec2::new(12.0, -14.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{} inputs  |  {} outputs", node.inputs.len(), output_count),
                        FontId::proportional(10.5),
                        kit::TEXT_DIM,
                    );
                }
            });
    }

    pub(super) fn asset_lab_node_inspector(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: Option<&GenerativeConfig>,
        versions: &[GenerationRecord],
        selected_node_id: Option<Uuid>,
        compatible_providers: &[ProviderEntry],
        action: &mut Option<AssetLabAction>,
    ) {
        kit::card_frame().show(ui, |ui| {
            kit::field_label(ui, "Step Settings");
            ui.add_space(kit::FORM_ROW_GAP);

            let selected_node = config
                .and_then(|config| {
                    selected_node_id
                        .and_then(|id| config.lab_graph.nodes.iter().find(|node| node.id == id))
                })
                .or_else(|| config.and_then(|config| config.lab_graph.nodes.first()));

            let Some(node) = selected_node else {
                ui.label(kit::caption(
                    "Add a step to choose a provider, wire inputs, and create lab outputs.",
                ));
                return;
            };

            let selected_provider = node.provider_id.and_then(|provider_id| {
                compatible_providers
                    .iter()
                    .find(|provider| provider.id == provider_id)
            });
            let provider_label = selected_provider
                .map(|provider| provider.name.clone())
                .unwrap_or_else(|| "Select provider".to_string());
            let mut provider_choice = node.provider_id;
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
            if provider_choice != node.provider_id {
                *action = Some(AssetLabAction::SetNodeProvider {
                    node_id: node.id,
                    provider_id: provider_choice,
                });
            }

            ui.add_space(kit::ACTION_GAP);
            if let Some(provider) = selected_provider {
                kit::field_label(ui, "Inputs");
                ui.add_space(kit::FORM_ROW_GAP);
                egui::ScrollArea::vertical()
                    .id_salt(("asset_lab_node_inputs", asset.id, node.id))
                    .max_height((ui.available_height() - 220.0).max(120.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, input) in provider.inputs.iter().enumerate() {
                            if index > 0 {
                                ui.add_space(kit::FORM_ROW_GAP);
                            }
                            self.asset_lab_node_input_field(
                                ui, asset, node, input, versions, action,
                            );
                        }
                    });
            } else {
                ui.label(kit::caption("Choose a provider before wiring inputs."));
            }

            ui.add_space(kit::ACTION_GAP);
            kit::field_label(ui, "Run");
            ui.add_space(kit::FORM_ROW_GAP);
            let can_generate = selected_provider.is_some() && asset.is_generative();
            ui.add_enabled_ui(can_generate, |ui| {
                if kit::primary_button(ui, "Run Step", ui.available_width()).clicked() {
                    *action = Some(AssetLabAction::GenerateNode(node.id));
                }
            });
            ui.add(
                egui::Label::new(kit::caption(
                    "Creates a new lab output. The timeline is unchanged until Use on Timeline.",
                ))
                .wrap(),
            );
            ui.add_space(kit::FORM_ROW_GAP);
            if kit::danger_button(ui, "Delete Step", ui.available_width()).clicked() {
                *action = Some(AssetLabAction::DeleteNode(node.id));
            }
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
                    inspector_multiline_text_field(
                        ui,
                        &label,
                        &mut value,
                        kit::MultilineTextFieldOptions::rows(3),
                    )
                } else {
                    inspector_text_field(ui, &label, &mut value)
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
                if inspector_drag_f64(ui, &label, &mut value, step, width) {
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
                if inspector_drag_i64(ui, &label, &mut value, step, width) {
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
                if inspector_bool_field(ui, &label, &mut value) {
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
                kit::labeled_combo_field(
                    ui,
                    &label,
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
        kit::labeled_combo_field(
            ui,
            &asset_lab_input_label(input),
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
                        if kit::secondary_button(ui, "Create Edit Step", width).clicked() {
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
                self.asset_lab.selected_version = Some(version);
                self.asset_lab.pending_delete_version = None;
                self.asset_lab_preview_texture = None;
            }
            AssetLabAction::SetActive(version) => {
                self.set_generative_active_version(asset_id, &version);
                self.asset_lab.selected_version = Some(version);
                self.asset_lab.pending_delete_version = None;
            }
            AssetLabAction::DuplicateVersion(version) => {
                self.duplicate_generative_version(asset_id, &version);
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
                self.delete_asset_lab_node(asset_id, node_id);
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
                self.delete_generative_version(asset_id, &version);
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

        let mut node = AssetLabNode::new(provider.as_ref().map(|provider| provider.id));
        node.inputs = generation_record_source_inputs(config, &source_record);
        retain_literal_node_inputs(&mut node.inputs);
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
        self.asset_lab.selected_version = Some(version.to_string());
        self.save_asset_lab_config(
            asset_id,
            &format!("Created edit step from output {version}. Timeline unchanged."),
        );
    }

    pub(super) fn select_asset_lab_node(&mut self, asset_id: Uuid, node_id: Uuid) {
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
        let valid_input_names: Option<Vec<String>> = match provider_id {
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
                Some(
                    provider
                        .inputs
                        .iter()
                        .map(|input| input.name.clone())
                        .collect(),
                )
            }
            None => None,
        };

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
                    if let Some(valid_input_names) = valid_input_names.as_ref() {
                        node.inputs
                            .retain(|name, _| valid_input_names.iter().any(|valid| valid == name));
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

    pub(super) fn generate_asset_lab_node(&mut self, asset_id: Uuid, node_id: Uuid) {
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
        let asset_label = self
            .editor
            .project
            .find_asset(asset_id)
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| "Generative Asset".to_string());
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

        let mut node_config = config_snapshot;
        node_config.provider_id = Some(provider.id);
        node_config.inputs = node.inputs.clone();
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

    pub(super) fn delete_asset_lab_node(&mut self, asset_id: Uuid, node_id: Uuid) {
        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                config.lab_graph.nodes.retain(|node| node.id != node_id);
                if config.lab_graph.selected_node_id == Some(node_id) {
                    config.lab_graph.selected_node_id =
                        config.lab_graph.nodes.first().map(|node| node.id);
                }
            });
        if !updated {
            self.editor.status = "Asset does not support Asset Lab steps.".to_string();
            return;
        }
        self.save_asset_lab_config(asset_id, "Deleted step. Outputs were kept.");
    }

    pub(super) fn save_asset_lab_config(&mut self, asset_id: Uuid, status: &str) {
        match self.editor.project.save_generative_config(asset_id) {
            Ok(_) => self.editor.status = status.to_string(),
            Err(err) => self.editor.status = format!("Failed to save Asset Lab config: {err}"),
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

    pub(super) fn set_generative_active_version(&mut self, asset_id: Uuid, version: &str) {
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
            self.editor.status = format!("Version {version} was not found.");
            return;
        };

        self.editor
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
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Failed to save active version: {err}");
            return;
        }
        self.invalidate_generative_asset_runtime(asset_id);
        self.editor.status = format!("Set {version} active.");
    }

    pub(super) fn duplicate_generative_version(&mut self, asset_id: Uuid, version: &str) {
        let Some(project_root) = self.editor.project.project_path.clone() else {
            self.editor.status = "Project folder is unavailable.".to_string();
            return;
        };
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.editor.status = "Asset not found.".to_string();
            return;
        };
        let Some(folder) = generative_folder_for_asset(&asset).cloned() else {
            self.editor.status = "Asset has no generation folder.".to_string();
            return;
        };
        let Some(config_snapshot) = self.editor.project.generative_config(asset_id).cloned() else {
            self.editor.status = "Generation config was not found.".to_string();
            return;
        };
        let Some(source_record) = config_snapshot
            .versions
            .iter()
            .find(|record| record.version == version)
            .cloned()
        else {
            self.editor.status = format!("Version {version} was not found.");
            return;
        };
        let new_version = next_version_label(&config_snapshot);
        let folder_path = project_root.join(&folder);
        if let Err(err) = copy_generative_version_files(&folder_path, version, &new_version) {
            self.editor.status = err;
            return;
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
            self.editor.status = format!("Duplicated version, but config save failed: {err}");
            return;
        }
        self.asset_lab.selected_version = Some(new_version.clone());
        self.invalidate_generative_asset_runtime(asset_id);
        self.editor.status = format!("Duplicated {version} as {new_version}.");
    }

    pub(super) fn delete_generative_version(&mut self, asset_id: Uuid, version: &str) {
        let Some(project_root) = self.editor.project.project_path.clone() else {
            self.editor.status = "Project folder is unavailable.".to_string();
            return;
        };
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.editor.status = "Asset not found.".to_string();
            return;
        };
        let Some(folder) = generative_folder_for_asset(&asset).cloned() else {
            self.editor.status = "Asset has no generation folder.".to_string();
            return;
        };
        if let Err(err) = delete_generative_version_files(&project_root.join(folder), version) {
            self.editor.status = format!("Failed to delete version files: {err}");
            return;
        }

        self.editor
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
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Deleted version, but config save failed: {err}");
            return;
        }
        let next_selected = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(preferred_asset_lab_version);
        self.asset_lab.selected_version = next_selected;
        self.invalidate_generative_asset_runtime(asset_id);
        self.editor.status = format!("Deleted {version}.");
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
        self.editor.preview_dirty = true;
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
