use std::cmp::Ordering as CmpOrdering;

use super::*;

use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Stroke, Ui, Vec2};

use crate::state::{asset_display_name, Asset, AssetKind};
use crate::ui_kit as kit;

use super::{
    format_duration, ASSET_ROW_H, ASSET_ROW_TEXT_GAP, ASSET_ROW_THUMBNAIL_SIZE,
    ASSET_THUMBNAIL_IMAGE_INSET,
};

pub(super) fn asset_row(
    ui: &mut Ui,
    asset: &Asset,
    selected: bool,
    thumbnail: Option<(egui::TextureId, Vec2)>,
) -> egui::Response {
    let accent = asset_accent(asset);
    kit::draw_accent_row(ui, ASSET_ROW_H, selected, accent, |ui, content_rect| {
        let thumb_rect = Rect::from_min_size(
            Pos2::new(
                content_rect.left(),
                content_rect.center().y - ASSET_ROW_THUMBNAIL_SIZE.y * 0.5,
            ),
            ASSET_ROW_THUMBNAIL_SIZE,
        );
        paint_asset_thumbnail(ui, thumb_rect, asset, accent, thumbnail);

        let text_left = thumb_rect.right() + ASSET_ROW_TEXT_GAP;
        let text_width = (content_rect.right() - text_left).max(24.0);
        paint_truncated_row_text_top(
            ui,
            Pos2::new(text_left, thumb_rect.top()),
            kit::value(asset_display_name(asset)),
            12.0,
            text_width,
            kit::TEXT,
        );
        paint_truncated_row_text_bottom(
            ui,
            Pos2::new(text_left, thumb_rect.bottom()),
            kit::caption(asset_row_subtitle(asset)),
            11.0,
            text_width,
            kit::TEXT_MUTED,
        );
    })
}

pub(super) fn paint_truncated_row_text_top(
    ui: &mut Ui,
    pos: Pos2,
    text: RichText,
    font_size: f32,
    max_width: f32,
    fallback_color: Color32,
) -> Vec2 {
    let font_id = FontId::proportional(font_size);
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        max_width,
        font_id,
    );
    let size = galley.size();
    ui.painter().galley(pos, galley, fallback_color);
    size
}

pub(super) fn paint_truncated_row_text_bottom(
    ui: &mut Ui,
    bottom_left: Pos2,
    text: RichText,
    font_size: f32,
    max_width: f32,
    fallback_color: Color32,
) -> Vec2 {
    let font_id = FontId::proportional(font_size);
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        max_width,
        font_id,
    );
    let size = galley.size();
    ui.painter().galley(
        Pos2::new(bottom_left.x, bottom_left.y - size.y),
        galley,
        fallback_color,
    );
    size
}

pub(super) fn paint_asset_thumbnail(
    ui: &mut Ui,
    rect: Rect,
    asset: &Asset,
    accent: Color32,
    thumbnail: Option<(egui::TextureId, Vec2)>,
) {
    ui.painter()
        .rect_filled(rect, kit::field_radius(), kit::FIELD_BG);
    ui.painter().rect_stroke(
        rect,
        kit::field_radius(),
        Stroke::new(1.0, kit::BORDER_SOFT),
        egui::StrokeKind::Inside,
    );

    if let Some((texture_id, size)) = thumbnail {
        let image_bounds = rect.shrink(ASSET_THUMBNAIL_IMAGE_INSET);
        let scale = (image_bounds.width() / size.x)
            .min(image_bounds.height() / size.y)
            .max(0.01);
        let image_rect = Rect::from_center_size(image_bounds.center(), size * scale);
        ui.painter().image(
            texture_id,
            image_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        ui.painter().rect_stroke(
            rect,
            kit::field_radius(),
            Stroke::new(1.0, accent.gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );
        return;
    }

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        asset_icon(asset),
        FontId::proportional(10.5),
        accent,
    );
}

fn asset_row_subtitle(asset: &Asset) -> String {
    let mut parts = vec![asset_kind_label(&asset.kind).to_string()];
    if let Some(duration) = asset.duration_seconds {
        parts.push(format_duration(duration));
    }
    parts.join("  ")
}

pub(super) fn asset_natural_cmp(a: &Asset, b: &Asset) -> CmpOrdering {
    natural_case_insensitive_cmp(&asset_display_name(a), &asset_display_name(b))
        .then_with(|| asset_kind_label(&a.kind).cmp(asset_kind_label(&b.kind)))
        .then_with(|| a.id.cmp(&b.id))
}

pub(super) fn natural_case_insensitive_cmp(a: &str, b: &str) -> CmpOrdering {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let mut a_index = 0usize;
    let mut b_index = 0usize;

    while a_index < a_chars.len() && b_index < b_chars.len() {
        let (a_digits, a_token, next_a) = natural_token(&a_chars, a_index);
        let (b_digits, b_token, next_b) = natural_token(&b_chars, b_index);

        let ordering = if a_digits && b_digits {
            natural_number_cmp(&a_token, &b_token)
        } else {
            a_token
                .to_ascii_lowercase()
                .cmp(&b_token.to_ascii_lowercase())
        };
        if ordering != CmpOrdering::Equal {
            return ordering;
        }

        a_index = next_a;
        b_index = next_b;
    }

    a_chars.len().cmp(&b_chars.len())
}

fn natural_token(chars: &[char], start: usize) -> (bool, String, usize) {
    let is_digits = chars[start].is_ascii_digit();
    let mut end = start + 1;
    while end < chars.len() && chars[end].is_ascii_digit() == is_digits {
        end += 1;
    }
    (is_digits, chars[start..end].iter().collect(), end)
}

fn natural_number_cmp(a: &str, b: &str) -> CmpOrdering {
    let a_trimmed = a.trim_start_matches('0');
    let b_trimmed = b.trim_start_matches('0');
    let a_digits = if a_trimmed.is_empty() { "0" } else { a_trimmed };
    let b_digits = if b_trimmed.is_empty() { "0" } else { b_trimmed };

    a_digits
        .len()
        .cmp(&b_digits.len())
        .then_with(|| a_digits.cmp(b_digits))
}

pub(super) fn asset_icon(asset: &Asset) -> &'static str {
    match asset.kind {
        AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. } => "VID",
        AssetKind::Image { .. } | AssetKind::GenerativeImage { .. } => "IMG",
        AssetKind::Audio { .. } | AssetKind::GenerativeAudio { .. } => "AUD",
    }
}

pub(super) fn asset_accent(asset: &Asset) -> Color32 {
    match asset.kind {
        AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. } => kit::VIDEO,
        AssetKind::Image { .. } | AssetKind::GenerativeImage { .. } => kit::IMAGE,
        AssetKind::Audio { .. } | AssetKind::GenerativeAudio { .. } => kit::AUDIO,
    }
}

pub(super) fn asset_kind_label(kind: &AssetKind) -> &'static str {
    match kind {
        AssetKind::Video { .. } => "Video",
        AssetKind::Image { .. } => "Image",
        AssetKind::Audio { .. } => "Audio",
        AssetKind::GenerativeVideo { .. } => "Generative Video",
        AssetKind::GenerativeImage { .. } => "Generative Image",
        AssetKind::GenerativeAudio { .. } => "Generative Audio",
    }
}
impl LatentSlateApp {
    pub(super) fn left_panel(&mut self, root: &mut Ui) {
        if self.editor.layout.left_collapsed {
            let response = egui::Panel::left(self.project_panel_id("assets_collapsed"))
                .exact_size(kit::COLLAPSED_RAIL_W)
                .frame(kit::collapsed_dock_frame())
                .show_inside(root, |ui| {
                    if kit::collapsed_rail_button(ui, "▶").clicked() {
                        self.editor.layout.left_collapsed = false;
                    }
                });
            self.asset_drop_target_rect = Some(response.response.rect);
            self.asset_drop_target_hovered = response.response.hovered();
            kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Right);
            return;
        }

        let response = egui::Panel::left(self.project_panel_id("assets"))
            .resizable(true)
            .default_size(self.editor.layout.left_width)
            .size_range(180.0..=420.0)
            .frame(kit::dock_frame())
            .show_inside(root, |ui| {
                kit::fixed_panel_body(ui, |ui| self.assets_panel(ui));
            });
        self.asset_drop_target_rect = Some(response.response.rect);
        self.asset_drop_target_hovered = response.response.hovered();
        self.editor.layout.left_width = response.response.rect.width().clamp(180.0, 420.0);
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Right);
    }

    pub(super) fn assets_panel(&mut self, ui: &mut Ui) {
        kit::panel_header(ui, "ASSETS", Some("◀"), || {
            self.editor.layout.left_collapsed = true;
        });
        ui.add_space(8.0);
        let mut create_video = false;
        let mut create_image = false;
        let mut create_audio = false;
        kit::stack_card_panel(ui, ADD_ASSETS_CARD_H, |ui| {
            kit::field_label(ui, "Add Assets");
            ui.add_space(6.0);
            let import_button_w = ui.available_width();
            if kit::secondary_button(ui, "Import Files...", import_button_w).clicked() {
                let initial_dir = self
                    .editor
                    .project
                    .project_path
                    .clone()
                    .unwrap_or_else(default_projects_dir);
                let options = kit::BrowseFileOptions::new()
                    .id_salt("asset_import_file")
                    .initial_dir(initial_dir.as_path())
                    .remember_last_dir()
                    .filters(ASSET_IMPORT_FILTERS);
                if let Some(path) = kit::pick_file_dialog(ui, options) {
                    self.import_asset_files(vec![path]);
                }
            }

            ui.add_space(kit::FORM_ROW_GAP);
            kit::field_label(ui, "New Generative");
            ui.add_space(6.0);
            kit::equal_media_pill_row(
                ui,
                &[
                    ("Video", kit::VIDEO),
                    ("Image", kit::IMAGE),
                    ("Audio", kit::AUDIO),
                ],
                |index| match index {
                    0 => create_video = true,
                    1 => create_image = true,
                    2 => create_audio = true,
                    _ => {}
                },
            );
        });
        if create_video {
            self.editor.overlays.generative_video = true;
        }
        if create_image {
            if let Err(err) = self.editor.create_generative_image() {
                self.editor.status = err;
            }
        }
        if create_audio {
            if let Err(err) = self.editor.create_generative_audio() {
                self.editor.status = err;
            }
        }

        ui.add_space(kit::FORM_ROW_GAP);
        let mut clear_selection = false;
        kit::scroll_body(ui, |ui| {
            ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
            let mut assets: Vec<(usize, Asset)> = self
                .editor
                .project
                .assets
                .iter()
                .cloned()
                .enumerate()
                .collect();
            assets.sort_by(|(a_index, a), (b_index, b)| {
                asset_natural_cmp(a, b).then_with(|| a_index.cmp(b_index))
            });
            for (_, asset) in assets {
                let selected = self.editor.selection.asset_ids.contains(&asset.id);
                let thumbnail = self.asset_thumbnail(ui.ctx(), &asset);
                let response = asset_row(ui, &asset, selected, thumbnail);
                response.dnd_set_drag_payload(AssetTimelineDragPayload { asset_id: asset.id });
                if response.clicked() {
                    if multi_select_modifier(ui) {
                        self.editor.selection.toggle_asset(asset.id);
                    } else {
                        self.editor.selection.select_asset(asset.id);
                    }
                }
                if response.double_clicked() {
                    self.open_asset_lab(asset.id);
                }
                response.context_menu(|ui| {
                    if automation_button(ui.button("Add to timeline"), "Add to timeline").clicked()
                    {
                        if let Err(err) = self.editor.add_asset_to_timeline(asset.id, None) {
                            self.editor.status = err;
                        }
                        ui.close();
                    }
                    if automation_button(ui.button("Duplicate"), "Duplicate").clicked() {
                        let asset_ids = if selected && self.editor.selection.asset_ids.len() > 1 {
                            self.editor.selection.asset_ids.clone()
                        } else {
                            vec![asset.id]
                        };
                        self.duplicate_assets(&asset_ids);
                        ui.close();
                    }
                    if asset.is_generative()
                        && automation_button(
                            ui.button("Extract active generation"),
                            "Extract active generation",
                        )
                        .clicked()
                    {
                        self.extract_active_generation(asset.id);
                        ui.close();
                    }
                    if automation_button(ui.button("Delete"), "Delete").clicked() {
                        let asset_ids = if selected && self.editor.selection.asset_ids.len() > 1 {
                            self.editor.selection.asset_ids.clone()
                        } else {
                            vec![asset.id]
                        };
                        self.request_delete_assets(&asset_ids);
                        ui.close();
                    }
                });
            }
            let empty_height = (ui.clip_rect().bottom() - ui.cursor().top()).max(0.0);
            if empty_height > 0.0 {
                let (_, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), empty_height),
                    Sense::click(),
                );
                if response.clicked() {
                    clear_selection = true;
                }
            }
        });
        if clear_selection {
            self.editor.selection.clear();
        }
    }

    pub(super) fn duplicate_assets(&mut self, asset_ids: &[Uuid]) {
        match self.editor.duplicate_assets(asset_ids) {
            Ok(new_asset_ids) => self.warm_asset_thumbnails(&new_asset_ids),
            Err(err) => self.editor.status = err,
        }
    }

    pub(super) fn extract_active_generation(&mut self, asset_id: Uuid) {
        match self.editor.extract_active_generation_as_asset(asset_id) {
            Ok(new_asset_id) => self.warm_asset_thumbnails(&[new_asset_id]),
            Err(err) => self.editor.status = err,
        }
    }

    pub(super) fn warm_asset_thumbnails(&mut self, asset_ids: &[Uuid]) {
        let Some(runtime) = self.generation_runtime.as_ref() else {
            return;
        };
        let thumbnailer = Arc::clone(&self.editor.thumbnailer);
        let assets: Vec<Asset> = asset_ids
            .iter()
            .filter_map(|asset_id| self.editor.project.find_asset(*asset_id).cloned())
            .filter(|asset| asset.is_visual())
            .collect();
        for asset in assets {
            let thumbnailer = Arc::clone(&thumbnailer);
            runtime.spawn(async move {
                let _ = thumbnailer.generate(&asset, true).await;
            });
        }
    }

    pub(super) fn handle_asset_file_drops(&mut self, ctx: &Context) {
        let dropped_files = ctx.input(|input| input.raw.dropped_files.clone());
        if dropped_files.is_empty() {
            return;
        }

        let paths: Vec<PathBuf> = dropped_files
            .into_iter()
            .filter_map(|file| file.path)
            .collect();
        if paths.is_empty() {
            self.editor.status = "Dropped data did not include filesystem paths.".to_string();
            return;
        }

        let pointer_pos =
            ctx.input(|input| input.pointer.interact_pos().or(input.pointer.hover_pos()));
        let drop_in_assets = self
            .asset_drop_target_rect
            .is_some_and(|rect| pointer_pos.is_some_and(|pos| rect.contains(pos)))
            || self.asset_drop_target_hovered;
        let supported_media_drop = paths
            .iter()
            .any(|path| is_supported_asset_import_path(path));
        let fallback_media_drop = pointer_pos.is_none() && supported_media_drop;
        if !drop_in_assets && !fallback_media_drop {
            return;
        }

        if self.editor.project_root().is_none() {
            self.editor.status =
                "Open or create a project before dropping media files to import.".to_string();
            return;
        }

        self.import_asset_files(paths);
        if self.editor.layout.left_collapsed {
            self.editor.layout.left_collapsed = false;
        }
    }

    pub(super) fn import_asset_files(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        if self.editor.project_root().is_none() {
            self.editor.status =
                "Open or create a project before importing media files.".to_string();
            return;
        }

        let mut imported = Vec::new();
        let mut failures = Vec::new();
        for path in paths {
            match self.editor.import_asset(&path) {
                Ok(asset_id) => imported.push(asset_id),
                Err(err) => failures.push(format!("{}: {err}", path.display())),
            }
        }

        let imported_count = imported.len();
        if imported_count > 0 {
            self.editor.selection.asset_ids = imported;
            self.editor.selection.clip_ids.clear();
            self.editor.selection.marker_ids.clear();
            self.editor.selection.track_ids.clear();
        }

        match (imported_count, failures.len()) {
            (0, 0) => {}
            (imported_count, 0) => {
                self.editor.status = if imported_count == 1 {
                    "Asset imported".to_string()
                } else {
                    format!("Imported {imported_count} assets")
                };
            }
            (0, failed_count) => {
                self.editor.status = if failed_count == 1 {
                    failures
                        .pop()
                        .unwrap_or_else(|| "Failed to import dropped file.".to_string())
                } else {
                    format!("Failed to import {failed_count} files")
                };
            }
            (imported_count, failed_count) => {
                self.editor.status =
                    format!("Imported {imported_count} assets; {failed_count} files failed");
            }
        }
    }
}
