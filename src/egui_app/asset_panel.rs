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
    source_dimensions: Option<Vec2>,
    source_fps: Option<f64>,
) -> egui::Response {
    let accent = asset_accent(asset);
    let response = kit::draw_accent_row(ui, ASSET_ROW_H, selected, accent, |ui, content_rect| {
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
            kit::caption(asset_row_subtitle(asset, source_dimensions, source_fps)),
            11.0,
            text_width,
            kit::TEXT_MUTED,
        );
    });
    response.on_hover_ui(|ui| asset_row_details_tooltip(ui, asset, source_dimensions, source_fps))
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
        Stroke::new(1.0_f32, kit::BORDER_SOFT),
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
            Stroke::new(1.0_f32, accent.gamma_multiply(0.7)),
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

fn asset_row_subtitle(
    asset: &Asset,
    source_dimensions: Option<Vec2>,
    source_fps: Option<f64>,
) -> String {
    let mut parts = Vec::new();
    if let Some(duration) = asset.duration_seconds {
        parts.push(format_duration(duration));
    }
    if let Some(fps) = asset_row_fps(asset, source_fps) {
        parts.push(format!("{} fps", format_compact_float(fps)));
    }
    if let Some(dimensions) = source_dimensions {
        parts.push(format_dimensions(dimensions));
    }
    parts.push(asset_kind_label(&asset.kind).to_string());
    parts.join(" | ")
}

fn asset_row_details_tooltip(
    ui: &mut Ui,
    asset: &Asset,
    source_dimensions: Option<Vec2>,
    source_fps: Option<f64>,
) {
    ui.set_min_width(220.0);
    ui.label(kit::value(asset_display_name(asset)));
    ui.add_space(4.0);

    egui::Grid::new(ui.id().with(("asset-row-details", asset.id)))
        .num_columns(2)
        .spacing(Vec2::new(12.0, 4.0))
        .show(ui, |ui| {
            asset_detail_row(ui, "Type", asset_kind_label(&asset.kind));
            asset_detail_row(
                ui,
                "Duration",
                asset
                    .duration_seconds
                    .map(format_duration)
                    .unwrap_or_else(|| "Unknown".to_string()),
            );
            if let Some(fps) = asset_row_fps(asset, source_fps) {
                asset_detail_row(ui, "FPS", format!("{} fps", format_compact_float(fps)));
            }
            if let Some(dimensions) = source_dimensions {
                asset_detail_row(ui, "Resolution", format_dimensions(dimensions));
            }
        });
}

fn asset_detail_row(ui: &mut Ui, label: &str, value: impl Into<String>) {
    ui.label(kit::caption(label).color(kit::TEXT_MUTED));
    ui.label(kit::value(value.into()));
    ui.end_row();
}

fn asset_row_fps(asset: &Asset, source_fps: Option<f64>) -> Option<f64> {
    source_fps.or(match asset.kind {
        AssetKind::GenerativeVideo { fps, .. } if fps.is_finite() && fps > 0.0 => Some(fps),
        _ => None,
    })
}

fn format_dimensions(dimensions: Vec2) -> String {
    let width = dimensions.x.round().max(1.0) as u32;
    let height = dimensions.y.round().max(1.0) as u32;
    format!("{width} x {height}")
}

fn format_compact_float(value: f64) -> String {
    let mut text = format!("{value:.3}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
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

impl AssetLibraryFilter {
    const ALL: [Self; 5] = [
        Self::All,
        Self::Video,
        Self::Image,
        Self::Audio,
        Self::Generative,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All assets",
            Self::Video => "Video",
            Self::Image => "Images",
            Self::Audio => "Audio",
            Self::Generative => "Generative",
        }
    }

    fn matches(self, asset: &Asset) -> bool {
        match self {
            Self::All => true,
            Self::Video => asset.is_video(),
            Self::Image => asset.is_image(),
            Self::Audio => asset.is_audio(),
            Self::Generative => asset.is_generative(),
        }
    }
}

impl AssetLibrarySort {
    const ALL: [Self; 3] = [Self::Name, Self::RecentlyAdded, Self::TimelineUse];

    fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::RecentlyAdded => "Recently added",
            Self::TimelineUse => "Timeline use",
        }
    }
}

impl AssetLibraryGrouping {
    const ALL: [Self; 2] = [Self::None, Self::MediaType];

    fn label(self) -> &'static str {
        match self {
            Self::None => "No grouping",
            Self::MediaType => "Group by media type",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum AssetMediaGroup {
    Video,
    Image,
    Audio,
}

impl AssetMediaGroup {
    fn label(self) -> &'static str {
        match self {
            Self::Video => "Video",
            Self::Image => "Images",
            Self::Audio => "Audio",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Self::Video => kit::VIDEO,
            Self::Image => kit::IMAGE,
            Self::Audio => kit::AUDIO,
        }
    }
}

fn asset_media_group(asset: &Asset) -> AssetMediaGroup {
    if asset.is_video() {
        AssetMediaGroup::Video
    } else if asset.is_image() {
        AssetMediaGroup::Image
    } else {
        AssetMediaGroup::Audio
    }
}

fn asset_matches_search(asset: &Asset, normalized_query: &str) -> bool {
    if normalized_query.is_empty() {
        return true;
    }
    let source = match &asset.kind {
        AssetKind::Video { path } | AssetKind::Image { path } | AssetKind::Audio { path } => {
            path.to_string_lossy()
        }
        AssetKind::GenerativeVideo { folder, .. }
        | AssetKind::GenerativeImage { folder, .. }
        | AssetKind::GenerativeAudio { folder, .. } => folder.to_string_lossy(),
    };
    asset_display_name(asset)
        .to_lowercase()
        .contains(normalized_query)
        || asset_kind_label(&asset.kind)
            .to_lowercase()
            .contains(normalized_query)
        || source.to_lowercase().contains(normalized_query)
}

fn compare_asset_rows(
    sort: AssetLibrarySort,
    a_index: usize,
    a: &Asset,
    b_index: usize,
    b: &Asset,
    timeline_use: &HashMap<Uuid, usize>,
) -> CmpOrdering {
    match sort {
        AssetLibrarySort::Name => asset_natural_cmp(a, b).then_with(|| a_index.cmp(&b_index)),
        AssetLibrarySort::RecentlyAdded => {
            b_index.cmp(&a_index).then_with(|| asset_natural_cmp(a, b))
        }
        AssetLibrarySort::TimelineUse => timeline_use
            .get(&b.id)
            .copied()
            .unwrap_or_default()
            .cmp(&timeline_use.get(&a.id).copied().unwrap_or_default())
            .then_with(|| asset_natural_cmp(a, b))
            .then_with(|| a_index.cmp(&b_index)),
    }
}

fn asset_group_header(ui: &mut Ui, group: AssetMediaGroup, count: usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(kit::section_label(group.label()).color(group.color()));
        ui.label(kit::caption(count.to_string()).color(kit::TEXT_DIM));
    });
    ui.add_space(2.0);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AssetAddAction {
    ImportMedia,
    CreateVideo,
    CreateImage,
    CreateAudio,
}

fn generative_asset_actions(ui: &mut Ui, action: &mut Option<AssetAddAction>) {
    if kit::popover_action_row(
        ui,
        "●",
        kit::VIDEO,
        "Video…",
        "Create a generative video asset",
    )
    .clicked()
    {
        *action = Some(AssetAddAction::CreateVideo);
        ui.close();
    }
    if kit::popover_action_row(
        ui,
        "●",
        kit::IMAGE,
        "Image",
        "Create a generative image asset",
    )
    .clicked()
    {
        *action = Some(AssetAddAction::CreateImage);
        ui.close();
    }
    if kit::popover_action_row(
        ui,
        "●",
        kit::AUDIO,
        "Audio",
        "Create a generative audio asset",
    )
    .clicked()
    {
        *action = Some(AssetAddAction::CreateAudio);
        ui.close();
    }
}

fn asset_add_popover(trigger: &egui::Response, include_import: bool) -> Option<AssetAddAction> {
    const POPOVER_W: f32 = 264.0;

    let mut action = None;
    egui::Popup::menu(trigger)
        .id(trigger.id.with("asset_add_popover"))
        .align(egui::RectAlign::BOTTOM_END)
        .gap(4.0)
        .width(POPOVER_W)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(POPOVER_W);
            if include_import {
                kit::field_label(ui, "Add Asset");
                ui.add_space(4.0);
                if kit::popover_action_row(
                    ui,
                    "⇧",
                    kit::TEXT_MUTED,
                    "Import media…",
                    "Video, image, or audio files",
                )
                .clicked()
                {
                    action = Some(AssetAddAction::ImportMedia);
                    ui.close();
                }
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            }
            kit::field_label(ui, "Create Generative");
            ui.add_space(4.0);
            generative_asset_actions(ui, &mut action);
        });
    action
}

fn empty_asset_library(ui: &mut Ui, action: &mut Option<AssetAddAction>) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("No assets yet")
                    .color(kit::TEXT_MUTED)
                    .strong(),
            );
            ui.add_space(5.0);
            ui.label(
                RichText::new("Drop media here, import files,\nor create a generative asset.")
                    .color(kit::TEXT_DIM)
                    .size(11.0),
            );
            ui.add_space(14.0);

            let available_width = ui.available_width().min(280.0);
            if available_width >= 250.0 {
                ui.allocate_ui_with_layout(
                    Vec2::new(available_width, kit::STANDALONE_BUTTON_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = kit::FORM_ROW_GAP;
                        let button_width = (available_width - kit::FORM_ROW_GAP) * 0.5;
                        if kit::secondary_button(ui, "Import Media", button_width).clicked() {
                            *action = Some(AssetAddAction::ImportMedia);
                        }
                        let trigger = kit::secondary_button(ui, "New Generative ▾", button_width);
                        if let Some(selected) = asset_add_popover(&trigger, false) {
                            *action = Some(selected);
                        }
                    },
                );
            } else {
                if kit::secondary_button(ui, "Import Media", available_width).clicked() {
                    *action = Some(AssetAddAction::ImportMedia);
                }
                ui.add_space(kit::FORM_ROW_GAP);
                let trigger = kit::secondary_button(ui, "New Generative ▾", available_width);
                if let Some(selected) = asset_add_popover(&trigger, false) {
                    *action = Some(selected);
                }
            }
        });
    });
}

fn asset_drop_state(ui: &mut Ui) {
    let height = (ui.clip_rect().bottom() - ui.cursor().top()).max(120.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let target_rect = rect.shrink(6.0);
    ui.painter().rect_filled(
        target_rect,
        kit::field_radius(),
        kit::FIELD_BG_ACTIVE.gamma_multiply(0.9),
    );
    ui.painter().rect_stroke(
        target_rect,
        kit::field_radius(),
        Stroke::new(1.5_f32, kit::BORDER_FOCUS),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        target_rect.center() - Vec2::new(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        "Drop media to import",
        FontId::proportional(13.0),
        kit::TEXT,
    );
    ui.painter().text(
        target_rect.center() + Vec2::new(0.0, 11.0),
        egui::Align2::CENTER_CENTER,
        "Video, image, or audio files",
        FontId::proportional(10.5),
        kit::TEXT_MUTED,
    );
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
        let mut add_action = None;
        let mut collapse_panel = false;
        kit::panel_header_with_actions(ui, "ASSETS", |ui| {
            if kit::icon_button(ui, "◀")
                .on_hover_text("Collapse Assets panel")
                .clicked()
            {
                collapse_panel = true;
            }
            let trigger = kit::icon_button(ui, "+").on_hover_text("Add asset");
            add_action = asset_add_popover(&trigger, true);
        });
        if collapse_panel {
            self.editor.layout.left_collapsed = true;
        }
        ui.add_space(8.0);
        let asset_files_hovered =
            ui.ctx().input(|input| {
                !input.raw.hovered_files.is_empty()
                    && input
                        .pointer
                        .hover_pos()
                        .is_some_and(|pointer| ui.max_rect().contains(pointer))
            }) || (!ui.ctx().input(|input| input.raw.hovered_files.is_empty())
                && self.asset_drop_target_hovered);
        if self.editor.project.assets.is_empty() {
            kit::scroll_body(ui, |ui| {
                if asset_files_hovered {
                    asset_drop_state(ui);
                } else {
                    empty_asset_library(ui, &mut add_action);
                }
            });
            if let Some(action) = add_action {
                self.perform_asset_add_action(ui, action);
            }
            return;
        }

        if kit::search_field(
            ui,
            &mut self.asset_search,
            ui.available_width(),
            "Search assets...",
            "Asset search",
        )
        .on_hover_text("Search asset names, media types, and project-relative source paths. Escape clears the query.")
        .changed()
        {
            self.asset_reveal_override = None;
            self.asset_reveal_scroll_target = None;
        }
        ui.add_space(6.0);
        kit::field_grid_row_with_height(ui, &[1.0, 1.0], kit::FIELD_H, 6.0, |ui, index| {
            let width = ui.available_width();
            if index == 0 {
                kit::combo_field(
                    ui,
                    "asset_library_filter",
                    self.asset_filter.label(),
                    width,
                    |ui| {
                        for option in AssetLibraryFilter::ALL {
                            if automation_selectable_value(
                                ui,
                                &mut self.asset_filter,
                                option,
                                option.label(),
                            )
                            .clicked()
                            {
                                self.asset_reveal_override = None;
                                self.asset_reveal_scroll_target = None;
                                ui.close();
                            }
                        }
                    },
                )
                .on_hover_text("Filter the library by media or generative assets.");
            } else {
                kit::combo_field(
                    ui,
                    "asset_library_sort",
                    self.asset_sort.label(),
                    width,
                    |ui| {
                        for option in AssetLibrarySort::ALL {
                            if automation_selectable_value(
                                ui,
                                &mut self.asset_sort,
                                option,
                                option.label(),
                            )
                            .clicked()
                            {
                                self.asset_reveal_override = None;
                                self.asset_reveal_scroll_target = None;
                                ui.close();
                            }
                        }
                    },
                )
                .on_hover_text("Sort assets by name, insertion order, or timeline usage.");
            }
        });
        ui.add_space(6.0);
        kit::combo_field(
            ui,
            "asset_library_grouping",
            self.asset_grouping.label(),
            ui.available_width(),
            |ui| {
                for option in AssetLibraryGrouping::ALL {
                    if automation_selectable_value(
                        ui,
                        &mut self.asset_grouping,
                        option,
                        option.label(),
                    )
                    .clicked()
                    {
                        self.asset_reveal_override = None;
                        self.asset_reveal_scroll_target = None;
                        ui.close();
                    }
                }
            },
        )
        .on_hover_text("Optionally group the current view into Video, Images, and Audio.");

        let normalized_query = self.asset_search.trim().to_lowercase();
        let timeline_use: HashMap<Uuid, usize> =
            self.editor
                .project
                .clips
                .iter()
                .fold(HashMap::new(), |mut counts, clip| {
                    *counts.entry(clip.asset_id).or_default() += 1;
                    counts
                });
        let mut assets: Vec<(usize, Asset)> = self
            .editor
            .project
            .assets
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, asset)| {
                self.asset_reveal_override == Some(asset.id)
                    || (self.asset_filter.matches(asset)
                        && asset_matches_search(asset, &normalized_query))
            })
            .collect();
        assets.sort_by(|(a_index, a), (b_index, b)| {
            let group_order = if self.asset_grouping == AssetLibraryGrouping::MediaType {
                asset_media_group(a).cmp(&asset_media_group(b))
            } else {
                CmpOrdering::Equal
            };
            group_order.then_with(|| {
                compare_asset_rows(self.asset_sort, *a_index, a, *b_index, b, &timeline_use)
            })
        });

        let total_assets = self.editor.project.assets.len();
        let visible_ids: HashSet<Uuid> = assets.iter().map(|(_, asset)| asset.id).collect();
        let hidden_selected = self
            .editor
            .selection
            .asset_ids
            .iter()
            .filter(|asset_id| !visible_ids.contains(asset_id))
            .count();
        if let Some(asset_id) = self.asset_reveal_override {
            if self.editor.project.find_asset(asset_id).is_some() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        kit::caption("Showing bound source outside the current filters")
                            .color(kit::MARKER),
                    );
                    if ui.small_button("Clear").clicked() {
                        self.asset_reveal_override = None;
                        self.asset_reveal_scroll_target = None;
                    }
                });
            } else {
                self.asset_reveal_override = None;
                self.asset_reveal_scroll_target = None;
            }
        }
        ui.add_space(4.0);
        let summary = if hidden_selected > 0 {
            format!(
                "{} of {total_assets} assets · {hidden_selected} selected hidden",
                assets.len()
            )
        } else if assets.len() != total_assets {
            format!("{} of {total_assets} assets", assets.len())
        } else if total_assets == 1 {
            "1 asset".to_string()
        } else {
            format!("{total_assets} assets")
        };
        let color = if hidden_selected > 0 {
            kit::MARKER
        } else {
            kit::TEXT_DIM
        };
        ui.label(kit::caption(summary).color(color));

        let mut clear_selection = false;
        let visible_assets_empty = assets.is_empty();
        kit::scroll_body(ui, |ui| {
            ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
            if asset_files_hovered {
                asset_drop_state(ui);
                return;
            }
            if visible_assets_empty {
                kit::empty_state(
                    ui,
                    "No matching assets",
                    "Adjust the search or filter controls above.",
                );
                ui.add_space(kit::FORM_ROW_GAP);
            }
            let mut current_group = None;
            let mut group_counts = HashMap::new();
            if self.asset_grouping == AssetLibraryGrouping::MediaType {
                for (_, asset) in &assets {
                    *group_counts.entry(asset_media_group(asset)).or_default() += 1;
                }
            }
            for (_, asset) in assets {
                if self.asset_grouping == AssetLibraryGrouping::MediaType {
                    let group = asset_media_group(&asset);
                    if current_group != Some(group) {
                        if current_group.is_some() {
                            ui.add_space(4.0);
                        }
                        asset_group_header(
                            ui,
                            group,
                            group_counts.get(&group).copied().unwrap_or_default(),
                        );
                        current_group = Some(group);
                    }
                }
                let selected = self.editor.selection.asset_ids.contains(&asset.id);
                let thumbnail = self.asset_thumbnail(ui.ctx(), &asset);
                let source_dimensions = self.asset_source_dimensions(&asset);
                let source_fps = self.asset_source_fps(&asset);
                let response = asset_row(
                    ui,
                    &asset,
                    selected,
                    thumbnail,
                    source_dimensions,
                    source_fps,
                );
                if self.asset_reveal_scroll_target == Some(asset.id) {
                    ui.scroll_to_rect(response.rect, Some(egui::Align::Center));
                    self.asset_reveal_scroll_target = None;
                }
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
        if let Some(action) = add_action {
            self.perform_asset_add_action(ui, action);
        }
    }

    fn perform_asset_add_action(&mut self, ui: &Ui, action: AssetAddAction) {
        match action {
            AssetAddAction::ImportMedia => {
                let initial_dir = self
                    .editor
                    .project
                    .project_path
                    .clone()
                    .unwrap_or_else(default_projects_dir);
                let options = kit::BrowseFileOptions::new()
                    .id_salt("asset_import_files")
                    .initial_dir(initial_dir.as_path())
                    .remember_last_dir()
                    .filters(ASSET_IMPORT_FILTERS);
                let paths = kit::pick_files_dialog(ui, options);
                self.import_asset_files(paths);
            }
            AssetAddAction::CreateVideo => {
                self.editor.overlays.generative_video = true;
            }
            AssetAddAction::CreateImage => {
                if let Err(err) = self.editor.create_generative_image() {
                    self.editor.status = err;
                }
            }
            AssetAddAction::CreateAudio => {
                if let Err(err) = self.editor.create_generative_audio() {
                    self.editor.status = err;
                }
            }
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
