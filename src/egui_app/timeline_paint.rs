use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke};

use crate::core::audio::cache::PeakCache;
use crate::state::Clip;
use crate::ui_kit as kit;

use super::TimelineThumbTile;

pub(super) fn paint_clip_thumbnail_strip(
    painter: &egui::Painter,
    rect: Rect,
    tiles: &[TimelineThumbTile],
) {
    if tiles.is_empty() {
        return;
    }
    let clip_painter = painter.with_clip_rect(rect.shrink(1.0));
    let tile_w = (rect.width() / tiles.len() as f32).max(1.0);
    for (index, tile) in tiles.iter().enumerate() {
        let x = rect.left() + index as f32 * tile_w;
        if x > rect.right() {
            break;
        }
        let image_bounds = Rect::from_min_max(
            Pos2::new(x, rect.top()),
            Pos2::new((x + tile_w).min(rect.right()), rect.bottom()),
        );
        let scale = (image_bounds.width() / tile.size.x)
            .max(image_bounds.height() / tile.size.y)
            .max(0.01);
        let image_rect = Rect::from_center_size(image_bounds.center(), tile.size * scale);
        clip_painter.image(
            tile.texture_id,
            image_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::from_white_alpha(145),
        );
    }
}

pub(super) fn paint_clip_cache_buckets(painter: &egui::Painter, rect: Rect, buckets: &[bool]) {
    if buckets.is_empty() || rect.width() <= 2.0 {
        return;
    }
    let y = rect.bottom() - 4.0;
    let strip_rect = Rect::from_min_max(
        Pos2::new(rect.left(), y),
        Pos2::new(rect.right(), rect.bottom() - 1.0),
    );
    let clip_painter = painter.with_clip_rect(rect.shrink(1.0));
    clip_painter.rect_filled(
        strip_rect,
        0.0,
        Color32::from_rgba_unmultiplied(95, 58, 42, 85),
    );

    let bucket_count = buckets.len() as f32;
    let cached_color = Color32::from_rgba_unmultiplied(45, 220, 165, 175);
    let mut run_start: Option<usize> = None;
    for (index, cached) in buckets
        .iter()
        .copied()
        .chain(std::iter::once(false))
        .enumerate()
    {
        match (cached, run_start) {
            (true, None) => run_start = Some(index),
            (false, Some(start)) => {
                let x0 = rect.left() + rect.width() * start as f32 / bucket_count;
                let x1 = rect.left() + rect.width() * index as f32 / bucket_count;
                if x1 > x0 {
                    clip_painter.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(x0, y),
                            Pos2::new(x1.min(rect.right()), rect.bottom() - 1.0),
                        ),
                        0.0,
                        cached_color,
                    );
                }
                run_start = None;
            }
            _ => {}
        }
    }
}

pub(super) fn paint_clip_waveform(
    painter: &egui::Painter,
    rect: Rect,
    clip: &Clip,
    cache: &PeakCache,
) {
    let Some(level) = cache.levels.first() else {
        return;
    };
    if level.peaks.is_empty() || rect.width() <= 1.0 {
        return;
    }
    let sample_rate = cache.sample_rate.max(1) as f64;
    let start_frame = (clip.trim_in_seconds.max(0.0) * sample_rate).floor() as usize;
    let end_frame = ((clip.trim_in_seconds + clip.duration).max(0.0) * sample_rate).ceil() as usize;
    let start_index = start_frame / level.block_size.max(1);
    let end_index = (end_frame / level.block_size.max(1))
        .min(level.peaks.len())
        .max(start_index + 1);
    if start_index >= level.peaks.len() {
        return;
    }
    let peaks = &level.peaks[start_index..end_index];
    let width = rect.width().round().max(1.0) as usize;
    let step = peaks.len() as f32 / width as f32;
    let center_y = rect.center().y;
    let amp = rect.height() * 0.44;
    let clip_painter = painter.with_clip_rect(rect);
    for x in 0..width {
        let start = (x as f32 * step).floor() as usize;
        let end = ((x + 1) as f32 * step).ceil() as usize;
        let end = end.min(peaks.len()).max(start + 1);
        let mut min = i16::MAX;
        let mut max = i16::MIN;
        for peak in &peaks[start..end] {
            min = min.min(peak.min_l.min(peak.min_r));
            max = max.max(peak.max_l.max(peak.max_r));
        }
        let min = min as f32 / i16::MAX as f32;
        let max = max as f32 / i16::MAX as f32;
        let y1 = center_y - max * amp;
        let y2 = center_y - min * amp;
        let px = rect.left() + x as f32;
        clip_painter.line_segment(
            [Pos2::new(px, y1), Pos2::new(px, y2)],
            Stroke::new(1.0, Color32::from_gray(158)),
        );
    }
}

pub(super) fn paint_dashed_timeline_button(
    painter: &egui::Painter,
    rect: Rect,
    label: &str,
    color: Color32,
    hovered: bool,
) {
    painter.rect_filled(
        rect,
        4.0,
        if hovered {
            color.gamma_multiply(0.16)
        } else {
            Color32::TRANSPARENT
        },
    );
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, color.gamma_multiply(if hovered { 0.8 } else { 0.45 })),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.5),
        if hovered { color } else { kit::TEXT_DIM },
    );
}
