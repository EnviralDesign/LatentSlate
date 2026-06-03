use eframe::egui::{self, FontId, Pos2, Rect, Ui, Vec2};
use uuid::Uuid;

use crate::state::{asset_display_name, Asset, Clip, ClipImageMode, Track};
use crate::ui_kit as kit;

use super::{
    TIMELINE_ADD_ROW_H, TIMELINE_CLIP_H, TIMELINE_CLIP_Y_PAD, TIMELINE_HANDLE_W,
    TIMELINE_KEYFRAME_HIT_W, TIMELINE_LABEL_W, TIMELINE_MARKER_HIT_W, TIMELINE_MARKER_LABEL_H,
    TIMELINE_MARKER_LABEL_W, TIMELINE_MAX_PX_PER_FRAME, TIMELINE_MIN_CLIP_W,
    TIMELINE_MIN_ZOOM_FLOOR, TIMELINE_RULER_H, TIMELINE_SCROLLBAR_H, TIMELINE_TRACK_H,
};

#[derive(Clone, Copy, Debug)]
pub(super) enum TimelineHit {
    Ruler,
    TrackLabel(Uuid),
    ClipBody(Uuid),
    ClipLeftEdge(Uuid),
    ClipRightEdge(Uuid),
    Marker(Uuid),
    EmptyTrack,
    Empty,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TimelineRects {
    pub(super) outer: Rect,
    pub(super) label: Rect,
    pub(super) ruler: Rect,
    pub(super) tracks: Rect,
    pub(super) add_row: Rect,
    pub(super) scrollbar: Rect,
    pub(super) track_scroll_y: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TimelineClipGeom {
    pub(super) clip_id: Uuid,
    pub(super) rect: Rect,
    pub(super) keyframe: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TimelineMarkerGeom {
    pub(super) marker_id: Uuid,
    pub(super) hit_rect: Rect,
}

pub(super) fn timeline_rects(outer: Rect, track_scroll_y: f32) -> TimelineRects {
    let ruler = Rect::from_min_max(
        Pos2::new(outer.left() + TIMELINE_LABEL_W, outer.top()),
        Pos2::new(outer.right(), outer.top() + TIMELINE_RULER_H),
    );
    let add_row = Rect::from_min_max(
        Pos2::new(outer.left(), outer.bottom() - TIMELINE_ADD_ROW_H),
        outer.right_bottom(),
    );
    let tracks = Rect::from_min_max(
        Pos2::new(outer.left() + TIMELINE_LABEL_W, ruler.bottom()),
        Pos2::new(outer.right(), add_row.top()),
    );
    let label = Rect::from_min_max(
        outer.left_top(),
        Pos2::new(outer.left() + TIMELINE_LABEL_W, add_row.top()),
    );
    let scrollbar = Rect::from_min_max(
        Pos2::new(
            outer.left() + TIMELINE_LABEL_W,
            add_row.bottom() - TIMELINE_SCROLLBAR_H,
        ),
        Pos2::new(outer.right(), add_row.bottom()),
    );
    TimelineRects {
        outer,
        label,
        ruler,
        tracks,
        add_row,
        scrollbar,
        track_scroll_y,
    }
}

pub(super) fn timeline_row_rect(rects: TimelineRects, row: usize) -> Rect {
    let top = rects.tracks.top() + row as f32 * TIMELINE_TRACK_H - rects.track_scroll_y;
    Rect::from_min_max(
        Pos2::new(rects.tracks.left(), top),
        Pos2::new(rects.tracks.right(), top + TIMELINE_TRACK_H),
    )
}

pub(super) fn timeline_header_left_width(ui: &Ui, collapsed: bool, zoom_label: &str) -> f32 {
    let title_w = measured_text_width(ui, "TIMELINE", FontId::proportional(10.5));
    if collapsed {
        return title_w + 12.0;
    }

    title_w
        + 8.0
        + kit::TIMELINE_TOOL_ICON_W
        + 4.0
        + measured_text_width(ui, zoom_label, FontId::proportional(11.0)).max(26.0)
        + 4.0
        + kit::TIMELINE_TOOL_ICON_W
        + 4.0
        + 42.0
        + 4.0
        + 58.0
        + 8.0
}

pub(super) fn timeline_header_right_width(ui: &Ui, timecode_label: &str) -> f32 {
    measured_text_width(ui, timecode_label, FontId::monospace(11.0))
        + 8.0
        + kit::TIMELINE_TRANSPORT_BUTTON_W
        + 8.0
}

fn measured_text_width(ui: &Ui, text: &str, font_id: FontId) -> f32 {
    egui::WidgetText::from(text.to_string())
        .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, font_id)
        .size()
        .x
}

pub(super) fn timeline_snapping_enabled(ui: &Ui) -> bool {
    ui.input(|input| !input.modifiers.alt)
}

pub(super) fn timeline_clip_title(clip: &Clip, asset: Option<&Asset>) -> String {
    clip.label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| asset.map(asset_display_name))
        .unwrap_or_else(|| "Clip".to_string())
}

pub(super) fn marker_label_and_rect(
    marker: &crate::state::Marker,
    row_rect: Rect,
    x: f32,
) -> Option<(&str, Rect)> {
    let label = marker
        .label
        .as_deref()
        .filter(|label| !label.trim().is_empty())?;
    Some((
        label,
        Rect::from_min_size(
            Pos2::new(x + 8.0, row_rect.top() + 7.0),
            Vec2::new(TIMELINE_MARKER_LABEL_W, TIMELINE_MARKER_LABEL_H),
        ),
    ))
}

pub(super) fn timeline_marker_hit_rect(
    marker: &crate::state::Marker,
    row_rect: Rect,
    x: f32,
) -> Rect {
    let stem_hit = Rect::from_center_size(
        Pos2::new(x, row_rect.center().y),
        Vec2::new(TIMELINE_MARKER_HIT_W, row_rect.height()),
    );
    if let Some((_, label_rect)) = marker_label_and_rect(marker, row_rect, x) {
        rect_union(stem_hit, label_rect.expand2(Vec2::new(6.0, 4.0)))
    } else {
        stem_hit
    }
}

fn rect_union(a: Rect, b: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(a.left().min(b.left()), a.top().min(b.top())),
        Pos2::new(a.right().max(b.right()), a.bottom().max(b.bottom())),
    )
}

pub(super) fn clip_is_keyframe_image(clip: &Clip, asset: Option<&Asset>) -> bool {
    clip.image_mode == ClipImageMode::Keyframe && asset.is_some_and(|asset| asset.is_image())
}

pub(super) fn timeline_clip_rect(
    clip: &Clip,
    asset: Option<&Asset>,
    row_rect: Rect,
    zoom: f32,
    scroll_x: f32,
) -> Rect {
    let x1 = time_to_timeline_x(clip.start_time, row_rect.left(), zoom, scroll_x);
    if clip_is_keyframe_image(clip, asset) {
        let y = row_rect.top() + TIMELINE_CLIP_Y_PAD;
        let h = TIMELINE_CLIP_H.min(row_rect.height() - TIMELINE_CLIP_Y_PAD * 2.0);
        let w = TIMELINE_KEYFRAME_HIT_W
            .min(row_rect.right() - x1 + 4.0)
            .max(0.0);
        return Rect::from_min_size(Pos2::new(x1 - 4.0, y), Vec2::new(w, h));
    }
    let x2 = time_to_timeline_x(clip.end_time(), row_rect.left(), zoom, scroll_x);
    let y = row_rect.top() + TIMELINE_CLIP_Y_PAD;
    Rect::from_min_max(
        Pos2::new(x1, y),
        Pos2::new(
            x2.max(x1 + TIMELINE_MIN_CLIP_W),
            y + TIMELINE_CLIP_H.min(row_rect.height() - TIMELINE_CLIP_Y_PAD * 2.0),
        ),
    )
}

pub(super) fn time_to_timeline_x(time: f64, left: f32, zoom: f32, scroll_x: f32) -> f32 {
    left + time as f32 * zoom - scroll_x
}

pub(super) fn timeline_zoom_bounds(duration: f32, viewport_w: f32, fps: f32) -> (f32, f32) {
    let duration = duration.max(0.01);
    let min_zoom = (viewport_w / duration).max(TIMELINE_MIN_ZOOM_FLOOR);
    let max_zoom = (fps.max(1.0) * TIMELINE_MAX_PX_PER_FRAME).max(min_zoom);
    (min_zoom, max_zoom)
}

pub(super) fn next_timeline_coarse_zoom(
    current: f32,
    direction: i32,
    fit_zoom: f32,
    max_zoom: f32,
) -> f32 {
    let fit_zoom = fit_zoom.max(TIMELINE_MIN_ZOOM_FLOOR);
    let max_zoom = max_zoom.max(fit_zoom);
    if (max_zoom - fit_zoom).abs() <= f32::EPSILON {
        return fit_zoom;
    }

    let current = current.clamp(fit_zoom, max_zoom);
    let mut stops = vec![fit_zoom, max_zoom];
    const MANTISSAS: &[f32] = &[1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0];
    let min_exp = fit_zoom.log10().floor() as i32 - 1;
    let max_exp = max_zoom.log10().ceil() as i32 + 1;
    for exp in min_exp..=max_exp {
        let decade = 10_f32.powi(exp);
        for mantissa in MANTISSAS {
            let stop = mantissa * decade;
            if stop >= fit_zoom * 0.999 && stop <= max_zoom * 1.001 {
                stops.push(stop.clamp(fit_zoom, max_zoom));
            }
        }
    }
    stops.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    stops.dedup_by(|a, b| (*a - *b).abs() <= ((*a).max(*b) * 0.002).max(0.25));

    let epsilon = (current.abs() * 0.00001).max(0.001);
    if direction >= 0 {
        stops
            .into_iter()
            .find(|stop| *stop > current + epsilon)
            .unwrap_or(max_zoom)
    } else {
        stops
            .into_iter()
            .rev()
            .find(|stop| *stop < current - epsilon)
            .unwrap_or(fit_zoom)
    }
}

pub(super) fn nice_timeline_step(target_seconds: f64) -> f64 {
    const STEPS: &[f64] = &[0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0];
    STEPS
        .iter()
        .copied()
        .find(|step| *step >= target_seconds)
        .unwrap_or(*STEPS.last().unwrap())
}

pub(super) fn timeline_ruler_label(seconds: f64) -> String {
    let total_seconds = seconds.round().max(0.0) as u64;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

pub(super) fn timeline_hit(
    pos: Pos2,
    rects: TimelineRects,
    tracks: &[Track],
    clip_geoms: &[TimelineClipGeom],
    marker_geoms: &[TimelineMarkerGeom],
) -> TimelineHit {
    if rects.ruler.contains(pos) {
        return TimelineHit::Ruler;
    }
    if pos.x < rects.tracks.left() && pos.y >= rects.tracks.top() && pos.y < rects.add_row.top() {
        let row = ((pos.y - rects.tracks.top() + rects.track_scroll_y) / TIMELINE_TRACK_H)
            .floor()
            .max(0.0) as usize;
        return tracks
            .get(row)
            .map(|track| TimelineHit::TrackLabel(track.id))
            .unwrap_or(TimelineHit::Empty);
    }
    for geom in clip_geoms.iter().rev() {
        let hit_rect = geom.rect.expand2(Vec2::new(TIMELINE_HANDLE_W, 0.0));
        if !hit_rect.contains(pos) {
            continue;
        }
        if geom.keyframe {
            return TimelineHit::ClipBody(geom.clip_id);
        }
        if (pos.x - geom.rect.left()).abs() <= TIMELINE_HANDLE_W {
            return TimelineHit::ClipLeftEdge(geom.clip_id);
        }
        if (pos.x - geom.rect.right()).abs() <= TIMELINE_HANDLE_W {
            return TimelineHit::ClipRightEdge(geom.clip_id);
        }
        if geom.rect.contains(pos) {
            return TimelineHit::ClipBody(geom.clip_id);
        }
    }
    for geom in marker_geoms.iter().rev() {
        if geom.hit_rect.contains(pos) {
            return TimelineHit::Marker(geom.marker_id);
        }
    }
    if rects.tracks.contains(pos) {
        TimelineHit::EmptyTrack
    } else {
        TimelineHit::Empty
    }
}

pub(super) fn timeline_track_row_at_pos<'a>(
    pos: Pos2,
    rects: TimelineRects,
    tracks: &'a [Track],
) -> Option<&'a Track> {
    if pos.y < rects.tracks.top() || pos.y >= rects.add_row.top() {
        return None;
    }
    let row = ((pos.y - rects.tracks.top() + rects.track_scroll_y) / TIMELINE_TRACK_H)
        .floor()
        .max(0.0) as usize;
    tracks.get(row)
}

pub(super) fn timeline_track_insert_index_at_pos(
    pos: Pos2,
    rects: TimelineRects,
    track_count: usize,
) -> usize {
    let local_y = pos.y - rects.tracks.top() + rects.track_scroll_y;
    (local_y / TIMELINE_TRACK_H)
        .round()
        .clamp(0.0, track_count as f32) as usize
}

pub(super) fn timeline_track_divider_y(rects: TimelineRects, insertion_index: usize) -> f32 {
    rects.tracks.top() + insertion_index as f32 * TIMELINE_TRACK_H - rects.track_scroll_y
}
