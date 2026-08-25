use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Ui, Vec2};

use crate::core::media_binding::{
    binding_hover_text, bound_media_type_for_input, lookup_media_binding, resolve_media_binding,
    MediaResolveContext, MediaResolvePlan,
};
use crate::state::{
    BoundMediaType, Clip, MediaBindingRelation, MediaBindingSource, MediaBindingStability,
    MediaFramePoint, MediaSample, MediaTimeRange,
};
use crate::ui_kit as kit;

use super::{time_to_timeline_x, LatentSlateApp, TimelineClipGeom, TimelineRects};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindingPortalDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindingLocalSource {
    ProjectAsset,
    FrozenArtifact,
}

#[derive(Clone, Debug)]
pub(super) struct TimelineBindingRoute {
    plan: MediaResolvePlan,
    target_clip_id: uuid::Uuid,
    target_anchor: Pos2,
    source_anchor: Option<Pos2>,
    source_clip_id: Option<uuid::Uuid>,
    segments: Vec<Pos2>,
    portal: Option<(Pos2, BindingPortalDirection)>,
    local_source: Option<BindingLocalSource>,
}

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
            let color = binding_port_color(plan.media_type, plan.is_ok());
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

    pub(super) fn timeline_media_binding_routes(
        &self,
        rects: TimelineRects,
        clip_geoms: &[TimelineClipGeom],
        zoom: f32,
    ) -> Vec<TimelineBindingRoute> {
        if self.editor.selection.clip_ids.len() != 1 {
            return Vec::new();
        }
        let Some(clip_id) = self.editor.selection.clip_ids.iter().copied().next() else {
            return Vec::new();
        };
        let Some(clip) = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
        else {
            return Vec::new();
        };
        let Some(asset) = self.editor.project.find_asset(clip.asset_id) else {
            return Vec::new();
        };
        if !asset.is_generative() {
            return Vec::new();
        }
        let Some(target_geom) = clip_geoms.iter().find(|geom| geom.clip_id == clip_id) else {
            return Vec::new();
        };
        if !target_geom.rect.intersects(rects.tracks) {
            return Vec::new();
        }
        let scroll_x = self.editor.layout.timeline_scroll_x;
        let track_left = rects.tracks.left();
        let mut routes = Vec::new();
        let mut allocated_risers = Vec::new();
        for plan in self.clip_media_binding_plans(&clip) {
            let target_anchor =
                target_anchor_pos(&clip, target_geom.rect, &plan, track_left, zoom, scroll_x);
            if !plan.is_ok() {
                routes.push(TimelineBindingRoute {
                    plan,
                    target_clip_id: clip.id,
                    target_anchor,
                    source_anchor: None,
                    source_clip_id: None,
                    segments: Vec::new(),
                    portal: None,
                    local_source: None,
                });
                continue;
            }

            if plan.source_clip_id.is_none() {
                let local_source = match &plan.spec.source {
                    MediaBindingSource::FrozenArtifact { .. } => {
                        Some(BindingLocalSource::FrozenArtifact)
                    }
                    MediaBindingSource::ProjectAsset { .. } => {
                        Some(BindingLocalSource::ProjectAsset)
                    }
                    _ => None,
                };
                routes.push(TimelineBindingRoute {
                    plan,
                    target_clip_id: clip.id,
                    target_anchor,
                    source_anchor: None,
                    source_clip_id: None,
                    segments: Vec::new(),
                    portal: None,
                    local_source,
                });
                continue;
            }

            let source_clip_id = plan.source_clip_id.expect("checked above");
            let Some(source_geom) = clip_geoms
                .iter()
                .find(|geom| geom.clip_id == source_clip_id)
            else {
                routes.push(TimelineBindingRoute {
                    plan,
                    target_clip_id: clip.id,
                    target_anchor,
                    source_anchor: None,
                    source_clip_id: Some(source_clip_id),
                    segments: Vec::new(),
                    portal: None,
                    local_source: None,
                });
                continue;
            };
            let resolved_source_anchor =
                source_anchor_pos(source_geom.rect, &plan, track_left, zoom, scroll_x);
            let source_visible = rects.tracks.contains(resolved_source_anchor);
            let (route_source, portal) = if source_visible {
                (resolved_source_anchor, None)
            } else {
                let (portal_pos, direction) =
                    binding_edge_portal(resolved_source_anchor, rects.tracks);
                (portal_pos, Some((portal_pos, direction)))
            };
            let segments = routed_binding_segments(
                route_source,
                target_anchor,
                source_visible.then_some(source_geom.rect),
                target_geom.rect,
                plan.relation,
                clip_geoms,
                source_clip_id,
                clip.id,
                rects.tracks,
                &allocated_risers,
            );
            if let Some(riser) = route_riser_x(&segments) {
                allocated_risers.push(riser);
            }
            routes.push(TimelineBindingRoute {
                plan,
                target_clip_id: clip.id,
                target_anchor,
                source_anchor: source_visible.then_some(resolved_source_anchor),
                source_clip_id: Some(source_clip_id),
                segments,
                portal,
                local_source: None,
            });
        }
        routes
    }

    pub(super) fn paint_media_binding_underlay(
        &self,
        painter: &egui::Painter,
        routes: &[TimelineBindingRoute],
    ) {
        for route in routes {
            if !route.plan.is_ok() || route.segments.len() < 2 {
                continue;
            }
            let color = binding_connector_color(false).gamma_multiply(0.30);
            let dashed = matches!(route.plan.stability, MediaBindingStability::Follow);
            for (index, window) in route.segments.windows(2).enumerate() {
                let source_stem = index == 0 && route.portal.is_none();
                let target_stem = index + 2 == route.segments.len();
                if source_stem || target_stem {
                    continue;
                }
                paint_binding_segment(painter, window[0], window[1], color, dashed, 1.15);
            }
        }
    }

    pub(super) fn paint_media_binding_overlay(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        rects: TimelineRects,
        routes: &[TimelineBindingRoute],
        clip_geoms: &[TimelineClipGeom],
        zoom: f32,
    ) -> bool {
        let hover = ui.ctx().pointer_hover_pos();
        let focused_index = hover.and_then(|pos| {
            routes.iter().position(|route| {
                pos.distance(route.target_anchor) <= 9.0
                    || route
                        .source_anchor
                        .is_some_and(|source| pos.distance(source) <= 9.0)
                    || route
                        .portal
                        .is_some_and(|(portal, _)| pos.distance(portal) <= 11.0)
                    || (!route.segments.is_empty()
                        && distance_to_polyline(pos, &route.segments) <= 5.0
                        && !clip_geoms.iter().any(|geom| {
                            geom.clip_id != route.target_clip_id
                                && Some(geom.clip_id) != route.source_clip_id
                                && geom.rect.contains(pos)
                        }))
            })
        });
        let mut reveal_source = None;
        let mut hover_text = None;

        for (index, route) in routes.iter().enumerate() {
            let focused = focused_index == Some(index);
            let geometry_missing = route.plan.is_ok()
                && route.source_clip_id.is_some()
                && route.source_anchor.is_none()
                && route.portal.is_none();
            let visually_valid = route.plan.is_ok() && !geometry_missing;
            let port_color = binding_port_color(route.plan.media_type, visually_valid);
            paint_ambient_route_stems(painter, route);
            paint_target_port(
                painter,
                route.target_anchor,
                &route.plan,
                port_color,
                focused,
            );

            if !visually_valid {
                paint_broken_port(painter, route.target_anchor);
            } else if let Some(local_source) = route.local_source {
                paint_local_source_terminal(
                    painter,
                    route.target_anchor,
                    local_source,
                    port_color,
                    rects.tracks,
                );
            } else if let Some(source_anchor) = route.source_anchor {
                paint_source_marker(painter, source_anchor, port_color, focused);
                if let Some(range) = route.plan.source_range {
                    if let Some(source_rect) = route
                        .source_clip_id
                        .and_then(|clip_id| self.timeline_binding_source_rect(clip_id, rects, zoom))
                    {
                        paint_source_range_highlight(
                            painter,
                            source_rect,
                            range,
                            rects.tracks.left(),
                            zoom,
                            self.editor.layout.timeline_scroll_x,
                            port_color,
                        );
                    }
                }
            }

            if let Some((portal_pos, direction)) = route.portal {
                let hit_rect = paint_edge_portal(
                    painter,
                    portal_pos,
                    direction,
                    &route.plan.field_label,
                    port_color,
                    focused,
                    rects.tracks,
                );
                let response = ui.interact(
                    hit_rect,
                    ui.id().with((
                        "timeline-binding-portal",
                        route.target_clip_id,
                        &route.plan.field_name,
                    )),
                    egui::Sense::click(),
                );
                let response = crate::core::automation::instrument_response(
                    response,
                    "timeline_binding_portal",
                    Some(format!("Reveal {} source", route.plan.field_label)),
                    true,
                    true,
                );
                if response.clicked() {
                    reveal_source = route.source_clip_id;
                }
            }

            if focused {
                if route.segments.len() >= 2 {
                    paint_focused_route(painter, route);
                }
                if let Some(pos) = hover {
                    hover_text = Some((pos, binding_hover_text(&self.editor.project, &route.plan)));
                }
            }
        }

        let revealed_source = reveal_source.is_some();
        if let Some(source_clip_id) = reveal_source {
            self.reveal_timeline_binding_source(source_clip_id, rects, zoom);
        }
        if let Some((origin, text)) = hover_text {
            paint_binding_tooltip(painter, origin, &text, rects.tracks);
        }
        revealed_source
    }

    fn timeline_binding_source_rect(
        &self,
        clip_id: uuid::Uuid,
        rects: TimelineRects,
        zoom: f32,
    ) -> Option<Rect> {
        let clip = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)?;
        let row = self
            .editor
            .project
            .tracks
            .iter()
            .position(|track| track.id == clip.track_id)?;
        let asset = self.editor.project.find_asset(clip.asset_id);
        Some(super::timeline_clip_rect(
            clip,
            asset,
            super::timeline_row_rect(rects, row),
            zoom,
            self.editor.layout.timeline_scroll_x,
        ))
    }

    fn reveal_timeline_binding_source(
        &mut self,
        clip_id: uuid::Uuid,
        rects: TimelineRects,
        zoom: f32,
    ) {
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
        let Some(row) = self
            .editor
            .project
            .tracks
            .iter()
            .position(|track| track.id == clip.track_id)
        else {
            return;
        };
        let center_time = (clip.start_time + clip.end_time()) * 0.5;
        self.editor.layout.timeline_scroll_x =
            (center_time as f32 * zoom - rects.tracks.width() * 0.5).max(0.0);
        let content_h = self.editor.project.tracks.len() as f32 * super::TIMELINE_TRACK_H;
        let max_scroll_y = (content_h - rects.tracks.height()).max(0.0);
        self.editor.layout.timeline_scroll_y = (row as f32 * super::TIMELINE_TRACK_H
            + super::TIMELINE_TRACK_H * 0.5
            - rects.tracks.height() * 0.5)
            .clamp(0.0, max_scroll_y);
        self.editor.selection.select_clip(clip_id);
        self.editor.status = "Revealed binding source on the timeline".to_string();
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

fn binding_port_color(media_type: BoundMediaType, ok: bool) -> Color32 {
    if !ok {
        return kit::DANGER;
    }
    match media_type {
        BoundMediaType::Image => kit::IMAGE,
        BoundMediaType::Video => kit::VIDEO,
        BoundMediaType::Audio => kit::AUDIO,
    }
}

fn binding_connector_color(focused: bool) -> Color32 {
    if focused {
        kit::BORDER_FOCUS
    } else {
        kit::TEXT_MUTED
    }
}

fn binding_edge_portal(source: Pos2, viewport: Rect) -> (Pos2, BindingPortalDirection) {
    let margin = 9.0;
    let x = source
        .x
        .clamp(viewport.left() + margin, viewport.right() - margin);
    let y = source
        .y
        .clamp(viewport.top() + margin, viewport.bottom() - margin);
    if source.y < viewport.top() {
        (
            Pos2::new(x, viewport.top() + 1.0),
            BindingPortalDirection::Up,
        )
    } else if source.y > viewport.bottom() {
        (
            Pos2::new(x, viewport.bottom() - 1.0),
            BindingPortalDirection::Down,
        )
    } else if source.x < viewport.left() {
        (
            Pos2::new(viewport.left() + 1.0, y),
            BindingPortalDirection::Left,
        )
    } else {
        (
            Pos2::new(viewport.right() - 1.0, y),
            BindingPortalDirection::Right,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn routed_binding_segments(
    source: Pos2,
    target: Pos2,
    source_rect: Option<Rect>,
    target_rect: Rect,
    relation: Option<MediaBindingRelation>,
    clip_geoms: &[TimelineClipGeom],
    source_clip_id: uuid::Uuid,
    target_clip_id: uuid::Uuid,
    viewport: Rect,
    allocated_risers: &[f32],
) -> Vec<Pos2> {
    if let Some(source_rect) = source_rect {
        let same_track = (source_rect.center().y - target_rect.center().y).abs() < 1.5;
        if same_track
            && matches!(
                relation,
                Some(MediaBindingRelation::TouchingPrevious | MediaBindingRelation::TouchingNext)
            )
        {
            return compact_touching_segments(source, target);
        }
        if same_track {
            let gutter_y = (source_rect.top().min(target_rect.top()) - 1.5)
                .clamp(viewport.top() + 1.0, viewport.bottom() - 1.0);
            return vec![
                source,
                Pos2::new(source.x, gutter_y),
                Pos2::new(target.x, gutter_y),
                target,
            ];
        }
    }

    let source_above = source.y <= target.y;
    let source_gutter_y = source_rect
        .map(|rect| {
            if source_above {
                rect.bottom() + 1.5
            } else {
                rect.top() - 1.5
            }
        })
        .unwrap_or(source.y)
        .clamp(viewport.top() + 1.0, viewport.bottom() - 1.0);
    let target_gutter_y = if source_above {
        target_rect.top() - 1.5
    } else {
        target_rect.bottom() + 1.5
    }
    .clamp(viewport.top() + 1.0, viewport.bottom() - 1.0);

    let min_x = source.x.min(target.x);
    let max_x = source.x.max(target.x);
    let candidates = [
        source.x,
        target.x,
        min_x - 12.0,
        max_x + 12.0,
        (source.x + target.x) * 0.5,
    ];
    let vertical_top = source_gutter_y.min(target_gutter_y);
    let vertical_bottom = source_gutter_y.max(target_gutter_y);
    let mut best = None;
    for candidate in candidates {
        let x = candidate.clamp(viewport.left() + 4.0, viewport.right() - 4.0);
        let mut score = (x - source.x).abs() * 0.05 + (x - target.x).abs() * 0.05;
        for geom in clip_geoms {
            if geom.clip_id == source_clip_id || geom.clip_id == target_clip_id {
                continue;
            }
            let rect = geom.rect.expand(2.0);
            let crosses_y = rect.bottom() >= vertical_top && rect.top() <= vertical_bottom;
            if crosses_y && x >= rect.left() && x <= rect.right() {
                score += 1_000.0;
            }
        }
        for allocated in allocated_risers {
            let distance = (x - *allocated).abs();
            if distance < 7.0 {
                score += 90.0 - distance * 8.0;
            }
        }
        if best.is_none_or(|(_, best_score)| score < best_score) {
            best = Some((x, score));
        }
    }
    let riser_x = best.map(|(x, _)| x).unwrap_or((source.x + target.x) * 0.5);
    vec![
        source,
        Pos2::new(source.x, source_gutter_y),
        Pos2::new(riser_x, source_gutter_y),
        Pos2::new(riser_x, target_gutter_y),
        Pos2::new(target.x, target_gutter_y),
        target,
    ]
}

fn route_riser_x(segments: &[Pos2]) -> Option<f32> {
    (segments.len() >= 6).then(|| segments[2].x)
}

fn paint_target_port(
    painter: &egui::Painter,
    anchor: Pos2,
    plan: &MediaResolvePlan,
    color: Color32,
    focused: bool,
) {
    painter.circle_stroke(
        anchor,
        if focused { 8.0 } else { 7.0 },
        Stroke::new(
            if focused { 2.0_f32 } else { 1.2_f32 },
            color.gamma_multiply(if focused { 0.35 } else { 0.16 }),
        ),
    );
    match plan.normalized_sample {
        MediaSample::AlignedRange | MediaSample::SourceRange { .. } | MediaSample::Whole => {
            let left = anchor - Vec2::new(7.0, 0.0);
            let right = anchor + Vec2::new(7.0, 0.0);
            painter.line_segment([left, right], Stroke::new(2.0_f32, color));
            painter.line_segment(
                [left - Vec2::new(0.0, 3.0), left + Vec2::new(0.0, 3.0)],
                Stroke::new(1.5_f32, color),
            );
            painter.line_segment(
                [right - Vec2::new(0.0, 3.0), right + Vec2::new(0.0, 3.0)],
                Stroke::new(1.5_f32, color),
            );
        }
        _ => paint_binding_diamond(painter, anchor, 4.5, color),
    }
}

fn paint_source_marker(painter: &egui::Painter, anchor: Pos2, color: Color32, focused: bool) {
    painter.circle_stroke(
        anchor,
        if focused { 8.0 } else { 7.0 },
        Stroke::new(
            if focused { 2.0_f32 } else { 1.2_f32 },
            color.gamma_multiply(if focused { 0.35 } else { 0.16 }),
        ),
    );
    painter.circle_filled(anchor, 3.25, color);
    painter.circle_stroke(anchor, 4.75, Stroke::new(1.0_f32, kit::PANEL_SUNKEN));
}

fn paint_broken_port(painter: &egui::Painter, anchor: Pos2) {
    painter.circle_stroke(anchor, 5.5, Stroke::new(1.8_f32, kit::DANGER));
    painter.line_segment(
        [anchor - Vec2::new(3.5, 3.5), anchor + Vec2::new(3.5, 3.5)],
        Stroke::new(1.5_f32, kit::DANGER),
    );
}

fn paint_local_source_terminal(
    painter: &egui::Painter,
    target: Pos2,
    source: BindingLocalSource,
    color: Color32,
    viewport: Rect,
) {
    let above = target.y - 14.0 >= viewport.top() + 4.0;
    let terminal = target + Vec2::new(0.0, if above { -13.0 } else { 13.0 });
    painter.line_segment(
        [target, terminal],
        Stroke::new(1.4_f32, color.gamma_multiply(0.75)),
    );
    let rect = Rect::from_center_size(terminal, Vec2::new(13.0, 11.0));
    painter.rect_filled(rect, 2.0, kit::PANEL_RAISED);
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.2_f32, color),
        egui::StrokeKind::Inside,
    );
    painter.text(
        terminal,
        egui::Align2::CENTER_CENTER,
        match source {
            BindingLocalSource::ProjectAsset => "A",
            BindingLocalSource::FrozenArtifact => "F",
        },
        FontId::monospace(8.0),
        color,
    );
}

fn paint_edge_portal(
    painter: &egui::Painter,
    portal: Pos2,
    direction: BindingPortalDirection,
    label: &str,
    color: Color32,
    focused: bool,
    viewport: Rect,
) -> Rect {
    let center = match direction {
        BindingPortalDirection::Left => portal + Vec2::new(7.0, 0.0),
        BindingPortalDirection::Right => portal - Vec2::new(7.0, 0.0),
        BindingPortalDirection::Up => portal + Vec2::new(0.0, 7.0),
        BindingPortalDirection::Down => portal - Vec2::new(0.0, 7.0),
    };
    let mut hit_rect = Rect::from_center_size(center, Vec2::splat(16.0));
    painter.rect_filled(hit_rect, 4.0, kit::PANEL_RAISED);
    painter.rect_stroke(
        hit_rect,
        4.0,
        Stroke::new(if focused { 1.8_f32 } else { 1.2_f32 }, color),
        egui::StrokeKind::Inside,
    );
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        match direction {
            BindingPortalDirection::Left => "<",
            BindingPortalDirection::Right => ">",
            BindingPortalDirection::Up => "^",
            BindingPortalDirection::Down => "v",
        },
        FontId::monospace(10.0),
        color,
    );
    if focused {
        let short_label: String = label.chars().take(18).collect();
        let galley = painter.layout_no_wrap(short_label, FontId::proportional(10.0), kit::TEXT);
        let label_size = galley.size() + Vec2::new(10.0, 6.0);
        let mut min = match direction {
            BindingPortalDirection::Right => Pos2::new(
                hit_rect.left() - label_size.x - 4.0,
                hit_rect.center().y - label_size.y * 0.5,
            ),
            _ => Pos2::new(
                hit_rect.right() + 4.0,
                hit_rect.center().y - label_size.y * 0.5,
            ),
        };
        min.x = min.x.clamp(
            viewport.left() + 2.0,
            (viewport.right() - label_size.x - 2.0).max(viewport.left() + 2.0),
        );
        min.y = min.y.clamp(
            viewport.top() + 2.0,
            (viewport.bottom() - label_size.y - 2.0).max(viewport.top() + 2.0),
        );
        let label_rect = Rect::from_min_size(min, label_size);
        painter.rect_filled(label_rect, 3.0, kit::PANEL_RAISED);
        painter.rect_stroke(
            label_rect,
            3.0,
            Stroke::new(1.0_f32, color.gamma_multiply(0.75)),
            egui::StrokeKind::Inside,
        );
        painter.galley(label_rect.min + Vec2::new(5.0, 3.0), galley, kit::TEXT);
        hit_rect = hit_rect.union(label_rect);
    }
    hit_rect
}

fn paint_ambient_route_stems(painter: &egui::Painter, route: &TimelineBindingRoute) {
    if route.segments.len() < 2 || !route.plan.is_ok() {
        return;
    }
    let color = binding_connector_color(false).gamma_multiply(0.55);
    let dashed = matches!(route.plan.stability, MediaBindingStability::Follow);
    if route.portal.is_none() {
        paint_binding_segment(
            painter,
            route.segments[0],
            route.segments[1],
            color,
            dashed,
            1.15,
        );
    }
    let last = route.segments.len() - 1;
    paint_binding_segment(
        painter,
        route.segments[last - 1],
        route.segments[last],
        color,
        dashed,
        1.15,
    );
}

fn paint_focused_route(painter: &egui::Painter, route: &TimelineBindingRoute) {
    for window in route.segments.windows(2) {
        paint_binding_segment(
            painter,
            window[0],
            window[1],
            kit::PANEL_SUNKEN.gamma_multiply(0.95),
            false,
            4.5,
        );
    }
    let dashed = matches!(route.plan.stability, MediaBindingStability::Follow);
    for window in route.segments.windows(2) {
        paint_binding_segment(
            painter,
            window[0],
            window[1],
            binding_connector_color(true),
            dashed,
            2.0,
        );
    }
    if matches!(route.plan.stability, MediaBindingStability::LockSource) {
        let center = route
            .segments
            .get(route.segments.len() / 2)
            .copied()
            .unwrap_or(route.target_anchor);
        paint_lock_mark(painter, center, binding_connector_color(true));
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

fn paint_lock_mark(painter: &egui::Painter, center: Pos2, color: Color32) {
    let body = Rect::from_center_size(center + Vec2::new(0.0, 1.5), Vec2::new(7.0, 5.5));
    painter.rect_filled(body, 1.0, color);
    painter.circle_stroke(
        center + Vec2::new(0.0, -2.2),
        2.4,
        Stroke::new(1.3_f32, color),
    );
}

fn paint_binding_segment(
    painter: &egui::Painter,
    a: Pos2,
    b: Pos2,
    color: Color32,
    dashed: bool,
    width: f32,
) {
    let stroke = Stroke::new(width, color);
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
