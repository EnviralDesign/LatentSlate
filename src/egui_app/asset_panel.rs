use std::cmp::Ordering as CmpOrdering;

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
