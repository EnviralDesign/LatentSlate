use std::path::{Path, PathBuf};

use super::*;

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
impl NlaEguiApp {
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
        let subtitle = format!("{}  |  {}", asset.name, asset_kind_label(&asset.kind));

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

        StripBuilder::new(ui)
            .clip(true)
            .size(Size::exact(285.0))
            .size(Size::remainder().at_least(320.0))
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    kit::card_frame().show(ui, |ui| {
                        kit::field_label(ui, "Versions");
                        ui.add_space(kit::FORM_ROW_GAP);
                        if versions.is_empty() {
                            ui.label(kit::caption("No generated versions yet."));
                        } else {
                            let list_height = (ui.available_height() - 4.0).max(120.0);
                            egui::ScrollArea::vertical()
                                .id_salt(("asset_lab_versions", asset.id))
                                .max_height(list_height)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                                    for record in versions.iter() {
                                        let selected = selected_version.as_deref()
                                            == Some(record.version.as_str());
                                        let active = active_version.as_deref()
                                            == Some(record.version.as_str());
                                        let response =
                                            asset_lab_version_row(ui, record, selected, active);
                                        if response.clicked() {
                                            *action = Some(AssetLabAction::SelectVersion(
                                                record.version.clone(),
                                            ));
                                        }
                                    }
                                });
                        }
                    });
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
                        let details_height = ui.available_height().max(80.0);
                        egui::ScrollArea::vertical()
                            .id_salt(("asset_lab_details", asset.id))
                            .max_height(details_height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                kit::field_label(ui, "Version Details");
                                ui.add_space(kit::FORM_ROW_GAP);
                                if let Some(version) = selected_version.as_deref() {
                                    asset_lab_meta_row(ui, "Version", version);
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
                                    ui.label(kit::caption("Select a generated version."));
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
            });
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
                        if kit::danger_button(ui, "Delete Version", width).clicked() {
                            *action =
                                Some(AssetLabAction::ConfirmDelete(pending_version.to_string()));
                        }
                    }
                },
            );
            return;
        }

        let can_set_active = active_version != Some(version);
        kit::equal_width_action_row(
            ui,
            2,
            kit::SECONDARY_BUTTON_H,
            kit::FIELD_COMPOUND_GAP,
            |ui, index, width| match index {
                0 => {
                    ui.add_enabled_ui(can_set_active, |ui| {
                        if kit::secondary_button(ui, "Set Active", width).clicked() {
                            *action = Some(AssetLabAction::SetActive(version.to_string()));
                        }
                    });
                }
                _ => {
                    if kit::secondary_button(ui, "Duplicate Version", width).clicked() {
                        *action = Some(AssetLabAction::DuplicateVersion(version.to_string()));
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
                    if kit::secondary_button(ui, "Extract Version", width).clicked() {
                        *action = Some(AssetLabAction::ExtractVersion(version.to_string()));
                    }
                }
                _ => {
                    if asset.is_video() {
                        if kit::secondary_button(ui, "Extract Current Frame", width).clicked() {
                            *action =
                                Some(AssetLabAction::ExtractCurrentFrame(version.to_string()));
                        }
                    } else if kit::secondary_button(ui, "Open Location", width).clicked() {
                        *action = Some(AssetLabAction::OpenLocation);
                    }
                }
            },
        );
        ui.add_space(kit::FORM_ROW_GAP);
        if asset.is_video() {
            if kit::secondary_button(ui, "Open Location", ui.available_width()).clicked() {
                *action = Some(AssetLabAction::OpenLocation);
            }
            ui.add_space(kit::FORM_ROW_GAP);
        }
        if kit::danger_button(ui, "Delete Version", ui.available_width()).clicked() {
            *action = Some(AssetLabAction::RequestDelete(version.to_string()));
        }

        if active_version != Some(version) {
            ui.add_space(kit::FORM_ROW_GAP);
            ui.label(kit::caption(
                "Timeline and preview use the active version. Set this version active to route it into existing clips.",
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
        };
        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                config.active_version = Some(new_version.clone());
                config.provider_id = Some(new_record.provider_id);
                config.inputs = new_record.inputs_snapshot.clone();
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
