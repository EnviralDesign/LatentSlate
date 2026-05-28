use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Color32, ColorImage, FontId, Pos2, Rect, Response, Sense, Stroke, TextureHandle,
    TextureId, Ui, Vec2,
};
use uuid::Uuid;

use crate::state::{
    parse_version_index, Asset, AssetKind, GenerationRecord, GenerativeConfig, ProviderEntry,
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

#[derive(Clone, Debug)]
pub(super) enum AssetLabAction {
    SelectVersion(String),
    SetActive(String),
    DuplicateVersion(String),
    ExtractVersion(String),
    ExtractCurrentFrame(String),
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
    resolve_generative_file(project_root, folder, version, extensions)
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
                format!("{}  ACTIVE", record.version)
            } else {
                record.version.clone()
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
    let folder_path = project_root.join(folder);
    if let Some(version) = active_version {
        for ext in extensions {
            let candidate = folder_path.join(format!("{version}.{ext}"));
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(folder_path)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        extensions
                            .iter()
                            .any(|allowed| allowed.eq_ignore_ascii_case(ext))
                    })
        })
        .collect();
    entries.sort();
    entries.into_iter().next()
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
