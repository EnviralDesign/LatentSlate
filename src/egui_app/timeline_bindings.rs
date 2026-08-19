use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Ui, Vec2};

use crate::core::media_binding::{
    binding_hover_text, bound_media_type_for_input, lookup_media_binding, resolve_media_binding,
    MediaResolveContext, MediaResolvePlan,
};
use crate::state::{
    Clip, MediaBindingRelation, MediaBindingStability, MediaFramePoint, MediaSample, MediaTimeRange,
};
use crate::ui_kit as kit;

use super::{time_to_timeline_x, LatentSlateApp, TimelineClipGeom, TimelineRects};

impl LatentSlateApp {
    pub(super) fn paint_clip_media_binding_marks(
        &self,
        painter: &egui::Painter,
        clip: &Clip,
        clip_rect: Rect,
        zoom: f32,
        scroll_x: f32,
        track_left: f32,
    ) {
        let plans = self.clip_media_binding_plans(clip);
        if plans.is_empty() {
            return;
        }
        let mut unresolved_required = false;
        let mut count = 0usize;
        for plan in &plans {
            count += 1;
            if !plan.is_ok() {
                unresolved_required = true;
            }
            let color = binding_state_color(plan.stability, plan.is_ok());
            match &plan.normalized_sample {
                MediaSample::Frame { at } => {
                    let time = plan.target_frame_time.unwrap_or_else(|| match at {
                        MediaFramePoint::OutputEnd => clip.end_time(),
                        MediaFramePoint::OutputOffset { seconds } => clip.start_time + *seconds,
                        MediaFramePoint::OutputFrame { frame } => {
                            clip.start_time
                                + (*frame as f64 / self.editor.project.settings.fps.max(1.0))
                        }
                        _ => clip.start_time,
                    });
                    let x = time_to_timeline_x(time, track_left, zoom, scroll_x)
                        .clamp(clip_rect.left() + 3.0, clip_rect.right() - 3.0);
                    paint_binding_diamond(painter, Pos2::new(x, clip_rect.center().y), 5.0, color);
                }
                MediaSample::AlignedRange
                | MediaSample::SourceRange { .. }
                | MediaSample::Whole => {
                    let rail = Rect::from_min_max(
                        Pos2::new(clip_rect.left() + 2.0, clip_rect.bottom() - 4.0),
                        Pos2::new(clip_rect.right() - 2.0, clip_rect.bottom() - 1.0),
                    );
                    painter.rect_filled(rail, 1.0, color.gamma_multiply(0.85));
                }
                MediaSample::Auto => {}
            }
        }
        if count > 1 {
            painter.text(
                Pos2::new(clip_rect.right() - 4.0, clip_rect.top() + 2.0),
                egui::Align2::RIGHT_TOP,
                count.to_string(),
                FontId::monospace(9.0),
                kit::TEXT_MUTED,
            );
        }
        if unresolved_required {
            painter.circle_filled(
                Pos2::new(clip_rect.right() - 6.0, clip_rect.top() + 6.0),
                3.5,
                kit::DANGER,
            );
        }
    }

    pub(super) fn paint_media_binding_overlay(
        &self,
        ui: &mut Ui,
        painter: &egui::Painter,
        rects: TimelineRects,
        clip_geoms: &[TimelineClipGeom],
        zoom: f32,
    ) {
        if self.editor.selection.clip_ids.len() != 1 {
            return;
        }
        let Some(clip_id) = self.editor.selection.clip_ids.iter().copied().next() else {
            return;
        };
        let Some(clip) = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
        else {
            return;
        };
        let Some(asset) = self.editor.project.find_asset(clip.asset_id) else {
            return;
        };
        if !asset.is_generative() {
            return;
        }
        let Some(target_geom) = clip_geoms.iter().find(|geom| geom.clip_id == clip_id) else {
            return;
        };
        let scroll_x = self.editor.layout.timeline_scroll_x;
        let track_left = rects.tracks.left();
        let hover = ui.ctx().pointer_hover_pos();
        let mut hover_text: Option<(Pos2, String)> = None;
        for plan in self.clip_media_binding_plans(&clip) {
            let color = binding_state_color(plan.stability, plan.is_ok());
            let target_anchor =
                target_anchor_pos(&clip, target_geom.rect, &plan, track_left, zoom, scroll_x);
            if matches!(plan.stability, MediaBindingStability::FreezeInput)
                || plan.source_clip_id.is_none()
            {
                painter.circle_stroke(
                    target_anchor,
                    4.5,
                    Stroke::new(1.5_f32, color.gamma_multiply(0.7)),
                );
                if !plan.is_ok() {
                    painter.circle_filled(target_anchor, 3.0, kit::DANGER);
                }
                if hover.is_some_and(|pos| pos.distance(target_anchor) <= 8.0) {
                    hover_text = Some((
                        target_anchor,
                        binding_hover_text(&self.editor.project, &plan),
                    ));
                }
                continue;
            }
            if !plan.is_ok() {
                painter.circle_filled(target_anchor, 3.5, kit::DANGER);
                if hover.is_some_and(|pos| pos.distance(target_anchor) <= 8.0) {
                    hover_text = Some((
                        target_anchor,
                        binding_hover_text(&self.editor.project, &plan),
                    ));
                }
                continue;
            }
            let Some(source_clip_id) = plan.source_clip_id else {
                continue;
            };
            let Some(source_geom) = clip_geoms
                .iter()
                .find(|geom| geom.clip_id == source_clip_id)
            else {
                painter.circle_filled(target_anchor, 3.5, kit::DANGER);
                continue;
            };
            if let Some(range) = plan.source_range {
                paint_source_range_highlight(
                    painter,
                    source_geom.rect,
                    range,
                    track_left,
                    zoom,
                    scroll_x,
                    color,
                );
            }
            let source_anchor =
                source_anchor_pos(source_geom.rect, &plan, track_left, zoom, scroll_x);
            let same_track =
                (source_geom.rect.center().y - target_geom.rect.center().y).abs() < 1.5;
            let touching = matches!(
                plan.relation,
                Some(MediaBindingRelation::TouchingPrevious | MediaBindingRelation::TouchingNext)
            );
            let segments = if same_track && touching {
                compact_touching_segments(source_anchor, target_anchor)
            } else {
                elbow_segments(source_anchor, target_anchor)
            };
            let dashed = matches!(plan.stability, MediaBindingStability::Follow);
            for window in segments.windows(2) {
                paint_binding_segment(painter, window[0], window[1], color, dashed);
            }
            painter.circle_filled(source_anchor, 3.0, color);
            painter.circle_filled(target_anchor, 2.5, color);
            if matches!(plan.stability, MediaBindingStability::LockSource) {
                let mid = segments
                    .get(segments.len() / 2)
                    .copied()
                    .unwrap_or(target_anchor);
                paint_lock_mark(painter, mid, color);
            }
            if let Some(pos) = hover {
                if distance_to_polyline(pos, &segments) <= 6.0 {
                    hover_text = Some((pos, binding_hover_text(&self.editor.project, &plan)));
                }
            }
        }
        if let Some((origin, text)) = hover_text {
            paint_binding_tooltip(painter, origin, &text, rects.tracks);
        }
    }

    fn clip_media_binding_plans(&self, clip: &Clip) -> Vec<MediaResolvePlan> {
        let Some(config) = self.editor.project.generative_config(clip.asset_id) else {
            return Vec::new();
        };
        let Some(provider_id) = config.provider_id else {
            return Vec::new();
        };
        let Some(provider) = self
            .editor
            .provider_entries
            .iter()
            .find(|provider| provider.id == provider_id)
        else {
            return Vec::new();
        };
        let mut plans = Vec::new();
        for input in provider.inputs.iter() {
            if bound_media_type_for_input(input).is_none() {
                continue;
            }
            let Some(spec) = lookup_media_binding(config, input, &self.editor.project) else {
                continue;
            };
            plans.push(resolve_media_binding(
                MediaResolveContext {
                    project: &self.editor.project,
                    target_asset_id: Some(clip.asset_id),
                    context_clip_id: Some(clip.id),
                    field: input,
                    provider: Some(provider),
                    config: Some(config),
                },
                &spec,
            ));
        }
        plans
    }
}

fn binding_state_color(stability: MediaBindingStability, ok: bool) -> Color32 {
    if !ok {
        return kit::DANGER;
    }
    match stability {
        MediaBindingStability::Follow => kit::IMAGE,
        MediaBindingStability::LockSource => kit::PRIMARY,
        MediaBindingStability::FreezeInput => kit::AUDIO.gamma_multiply(0.85),
    }
}

fn paint_binding_diamond(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let h = size;
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(center.x, center.y - h),
            Pos2::new(center.x + h, center.y),
            Pos2::new(center.x, center.y + h),
            Pos2::new(center.x - h, center.y),
        ],
        color,
        Stroke::NONE,
    ));
}

fn target_anchor_pos(
    clip: &Clip,
    rect: Rect,
    plan: &MediaResolvePlan,
    track_left: f32,
    zoom: f32,
    scroll_x: f32,
) -> Pos2 {
    let time = plan.target_frame_time.unwrap_or_else(|| {
        plan.target_range
            .map(|range| (range.start_seconds + range.end_seconds) * 0.5)
            .unwrap_or(clip.start_time)
    });
    let x = time_to_timeline_x(time, track_left, zoom, scroll_x)
        .clamp(rect.left() + 2.0, rect.right() - 2.0);
    let y = if matches!(
        plan.normalized_sample,
        MediaSample::AlignedRange | MediaSample::SourceRange { .. } | MediaSample::Whole
    ) {
        rect.bottom() - 2.0
    } else {
        rect.center().y
    };
    Pos2::new(x, y)
}

fn source_anchor_pos(
    rect: Rect,
    plan: &MediaResolvePlan,
    track_left: f32,
    zoom: f32,
    scroll_x: f32,
) -> Pos2 {
    let time = plan.source_frame_time.or_else(|| {
        plan.source_range
            .map(|range| (range.start_seconds + range.end_seconds) * 0.5)
    });
    let x = time
        .map(|time| time_to_timeline_x(time, track_left, zoom, scroll_x))
        .unwrap_or(rect.center().x)
        .clamp(rect.left() + 2.0, rect.right() - 2.0);
    Pos2::new(x, rect.center().y)
}

fn paint_source_range_highlight(
    painter: &egui::Painter,
    source_rect: Rect,
    range: MediaTimeRange,
    track_left: f32,
    zoom: f32,
    scroll_x: f32,
    color: Color32,
) {
    let x1 = time_to_timeline_x(range.start_seconds, track_left, zoom, scroll_x);
    let x2 = time_to_timeline_x(range.end_seconds, track_left, zoom, scroll_x);
    let highlight = Rect::from_min_max(
        Pos2::new(x1.min(x2), source_rect.top()),
        Pos2::new(x1.max(x2), source_rect.bottom()),
    )
    .intersect(source_rect);
    if highlight.width() > 1.0 {
        painter.rect_filled(
            highlight,
            2.0,
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 48),
        );
    }
}

fn compact_touching_segments(source: Pos2, target: Pos2) -> Vec<Pos2> {
    let apex_y = source.y.min(target.y) - 8.0;
    vec![
        source,
        Pos2::new(source.x, apex_y),
        Pos2::new(target.x, apex_y),
        target,
    ]
}

fn elbow_segments(source: Pos2, target: Pos2) -> Vec<Pos2> {
    let mid_y = (source.y + target.y) * 0.5;
    vec![
        source,
        Pos2::new(source.x, mid_y),
        Pos2::new(target.x, mid_y),
        target,
    ]
}

fn paint_lock_mark(painter: &egui::Painter, center: Pos2, color: Color32) {
    let body = Rect::from_center_size(center + Vec2::new(0.0, 1.5), Vec2::new(7.0, 5.5));
    painter.rect_filled(body, 1.0, color);
    painter.circle_stroke(center + Vec2::new(0.0, -2.2), 2.4, Stroke::new(1.3_f32, color));
}

fn paint_binding_segment(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32, dashed: bool) {
    let stroke = Stroke::new(2.0_f32, color);
    if !dashed {
        painter.line_segment([a, b], stroke);
        return;
    }
    let delta = b - a;
    let len = delta.length();
    if len <= 0.5 {
        return;
    }
    let unit = delta / len;
    let dash = 5.0;
    let gap = 4.0;
    let mut d = 0.0;
    while d < len {
        let start = a + unit * d;
        let end = a + unit * (d + dash).min(len);
        painter.line_segment([start, end], stroke);
        d += dash + gap;
    }
}

fn distance_to_polyline(pos: Pos2, points: &[Pos2]) -> f32 {
    points
        .windows(2)
        .map(|pair| distance_to_segment(pos, pair[0], pair[1]))
        .fold(f32::MAX, f32::min)
}

fn distance_to_segment(pos: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.x * ab.x + ab.y * ab.y;
    if len_sq <= f32::EPSILON {
        return pos.distance(a);
    }
    let t = ((pos - a).x * ab.x + (pos - a).y * ab.y) / len_sq;
    let t = t.clamp(0.0, 1.0);
    pos.distance(a + ab * t)
}

fn paint_binding_tooltip(painter: &egui::Painter, origin: Pos2, text: &str, clip: Rect) {
    let font = FontId::proportional(11.0);
    let galley = painter.layout(text.to_string(), font, kit::TEXT, 280.0);
    let size = galley.size() + Vec2::new(14.0, 10.0);
    let mut min = origin + Vec2::new(10.0, -size.y - 8.0);
    if min.x + size.x > clip.right() {
        min.x = (clip.right() - size.x - 4.0).max(clip.left() + 4.0);
    }
    if min.y < clip.top() {
        min.y = origin.y + 10.0;
    }
    let rect = Rect::from_min_size(min, size);
    painter.rect_filled(rect, 4.0, kit::PANEL_RAISED);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, kit::BORDER),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + Vec2::new(7.0, 5.0), galley, kit::TEXT);
}
