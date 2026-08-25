use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Ui, Vec2};

use crate::core::media_binding::{
    binding_hover_text, bound_media_type_for_input, default_sample_for_field, lookup_media_binding,
    resolve_media_binding, MediaBindingError, MediaResolveContext, MediaResolvePlan,
};
use crate::state::{
    BoundMediaType, Clip, MediaBindingRelation, MediaBindingSource, MediaBindingSpec,
    MediaBindingStability, MediaFramePoint, MediaSample, MediaTimeRange,
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

#[derive(Clone, Copy, Debug)]
struct BindingPortalVisual {
    direction: BindingPortalDirection,
    icon_rect: Rect,
}

#[derive(Clone, Copy, Debug)]
struct BindingTerminalVisual {
    rect: Rect,
    anchor: Pos2,
}

#[derive(Clone, Debug)]
struct TimelineBindingVisualPlan {
    plan: MediaResolvePlan,
    required_unbound: bool,
}

#[derive(Clone, Copy, Debug)]
enum BindingOccupiedSegment {
    Vertical { x: f32, y0: f32, y1: f32 },
    Horizontal { y: f32, x0: f32, x1: f32 },
}

struct TimelineBindingVisualMetrics;

impl TimelineBindingVisualMetrics {
    const ANCHOR_INSET: f32 = 3.0;
    const PORTAL_SIZE: f32 = 16.0;
    const PORTAL_EDGE_INSET: f32 = 10.0;
    const PORTAL_SCROLLBAR_RESERVE: f32 = 18.0;
    const PORTAL_STACK_GAP: f32 = 3.0;
    const ROUTE_HOVER_RADIUS: f32 = 5.0;
    const RISER_SEPARATION: f32 = 7.0;
    const TERMINAL_GAP: f32 = 6.0;
    const FOCUS_LATCH: Duration = Duration::from_millis(280);
    const AMBIENT_ROUTE_LIMIT: usize = 6;
}

#[derive(Clone, Debug)]
pub(super) struct TimelineBindingRoute {
    plan: MediaResolvePlan,
    required_unbound: bool,
    target_clip_id: uuid::Uuid,
    target_rect: Rect,
    target_anchor: Pos2,
    target_continuation: Option<BindingPortalVisual>,
    source_anchor: Option<Pos2>,
    source_rect: Option<Rect>,
    source_focus_time: Option<f64>,
    source_timeline_range: Option<MediaTimeRange>,
    source_clip_id: Option<uuid::Uuid>,
    segments: Vec<Pos2>,
    portal: Option<BindingPortalVisual>,
    local_source: Option<BindingLocalSource>,
    terminal: Option<BindingTerminalVisual>,
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
        for visual in &plans {
            let plan = &visual.plan;
            count += 1;
            if !plan.is_ok() {
                unresolved_required = true;
            }
            if clip_rect.width() < 16.0 {
                continue;
            }
            let color = binding_port_color(plan.media_type, plan.is_ok());
            match &plan.normalized_sample {
                MediaSample::Frame { .. } => {
                    let anchor = target_anchor_pos(
                        clip,
                        clip_rect,
                        plan,
                        track_left,
                        zoom,
                        scroll_x,
                        self.editor.project.settings.fps,
                    );
                    paint_binding_diamond(
                        painter,
                        anchor,
                        binding_mark_scale(clip_rect) * 5.0,
                        color,
                    );
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
        if clip_rect.width() < 16.0 {
            let color = if unresolved_required {
                kit::DANGER
            } else {
                binding_port_color(plans[0].plan.media_type, true)
            };
            painter.circle_filled(
                Pos2::new(
                    safe_axis_in_rect(
                        clip_rect.right() - 2.0,
                        clip_rect.left(),
                        clip_rect.right(),
                        1.0,
                    ),
                    clip_rect.top() + clip_rect.height().min(6.0) * 0.5,
                ),
                2.0,
                color,
            );
        }
        if count > 1 && clip_rect.width() >= 24.0 {
            painter.text(
                Pos2::new(clip_rect.right() - 4.0, clip_rect.top() + 2.0),
                egui::Align2::RIGHT_TOP,
                count.to_string(),
                FontId::monospace(9.0),
                kit::TEXT_MUTED,
            );
        }
        if unresolved_required && clip_rect.width() >= 16.0 {
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
        let mut occupied_segments = Vec::new();
        let mut allocated_portals = Vec::new();
        let mut visuals = self.clip_media_binding_plans(&clip);
        visuals.sort_by(|left, right| {
            semantic_target_time(&clip, &left.plan, self.editor.project.settings.fps)
                .total_cmp(&semantic_target_time(
                    &clip,
                    &right.plan,
                    self.editor.project.settings.fps,
                ))
                .then_with(|| left.plan.field_name.cmp(&right.plan.field_name))
        });
        for visual in visuals {
            let plan = visual.plan;
            let semantic_target_anchor = target_anchor_pos(
                &clip,
                target_geom.rect,
                &plan,
                track_left,
                zoom,
                scroll_x,
                self.editor.project.settings.fps,
            );
            let target_continuation = (!rects.tracks.contains(semantic_target_anchor)).then(|| {
                let continuation = binding_edge_portal(
                    semantic_target_anchor,
                    rects.tracks.center(),
                    rects.tracks,
                    &allocated_portals,
                );
                allocated_portals.push(continuation);
                continuation
            });
            let target_anchor = target_continuation
                .map(|continuation| continuation.icon_rect.center())
                .unwrap_or(semantic_target_anchor);
            let local_source = match &plan.spec.source {
                MediaBindingSource::FrozenArtifact { .. } => {
                    Some(BindingLocalSource::FrozenArtifact)
                }
                MediaBindingSource::ProjectAsset { .. } => Some(BindingLocalSource::ProjectAsset),
                _ => None,
            };
            if let Some(local_source) = local_source {
                let terminal = local_source_terminal_visual(
                    target_anchor,
                    target_geom.rect,
                    local_source,
                    rects.tracks,
                );
                routes.push(TimelineBindingRoute {
                    plan,
                    required_unbound: visual.required_unbound,
                    target_clip_id: clip.id,
                    target_rect: target_geom.rect,
                    target_anchor,
                    target_continuation,
                    source_anchor: None,
                    source_rect: None,
                    source_focus_time: None,
                    source_timeline_range: None,
                    source_clip_id: None,
                    segments: Vec::new(),
                    portal: None,
                    local_source: Some(local_source),
                    terminal: Some(terminal),
                });
                continue;
            }
            if !plan.is_ok() {
                routes.push(TimelineBindingRoute {
                    plan,
                    required_unbound: visual.required_unbound,
                    target_clip_id: clip.id,
                    target_rect: target_geom.rect,
                    target_anchor,
                    target_continuation,
                    source_anchor: None,
                    source_rect: None,
                    source_focus_time: None,
                    source_timeline_range: None,
                    source_clip_id: None,
                    segments: Vec::new(),
                    portal: None,
                    local_source: None,
                    terminal: None,
                });
                continue;
            }

            let source_clip_id = match &plan.spec.source {
                MediaBindingSource::TimelineClip { clip_id, .. } => Some(*clip_id),
                MediaBindingSource::FollowTimeline { .. } => plan.source_clip_id,
                MediaBindingSource::ProjectAsset { .. }
                | MediaBindingSource::FrozenArtifact { .. } => None,
            };
            let Some(source_clip_id) = source_clip_id else {
                routes.push(TimelineBindingRoute {
                    plan,
                    required_unbound: visual.required_unbound,
                    target_clip_id: clip.id,
                    target_rect: target_geom.rect,
                    target_anchor,
                    target_continuation,
                    source_anchor: None,
                    source_rect: None,
                    source_focus_time: None,
                    source_timeline_range: None,
                    source_clip_id: None,
                    segments: Vec::new(),
                    portal: None,
                    local_source: None,
                    terminal: None,
                });
                continue;
            };
            let Some(source_geom) = clip_geoms
                .iter()
                .find(|geom| geom.clip_id == source_clip_id)
            else {
                routes.push(TimelineBindingRoute {
                    plan,
                    required_unbound: visual.required_unbound,
                    target_clip_id: clip.id,
                    target_rect: target_geom.rect,
                    target_anchor,
                    target_continuation,
                    source_anchor: None,
                    source_rect: None,
                    source_focus_time: None,
                    source_timeline_range: None,
                    source_clip_id: Some(source_clip_id),
                    segments: Vec::new(),
                    portal: None,
                    local_source: None,
                    terminal: None,
                });
                continue;
            };
            let source_clip = self
                .editor
                .project
                .clips
                .iter()
                .find(|candidate| candidate.id == source_clip_id);
            let source_duration = source_clip.and_then(|source_clip| {
                self.editor
                    .project
                    .find_asset(source_clip.asset_id)
                    .and_then(|asset| asset.duration_seconds)
            });
            let source_timeline_frame = source_clip.and_then(|source_clip| {
                plan.source_frame_time.map(|source_time| {
                    source_clip.timeline_time_for_source(source_time, source_duration)
                })
            });
            let source_timeline_range = source_clip.and_then(|source_clip| {
                plan.source_range.map(|range| {
                    let start =
                        source_clip.timeline_time_for_source(range.start_seconds, source_duration);
                    let end =
                        source_clip.timeline_time_for_source(range.end_seconds, source_duration);
                    MediaTimeRange::new(start.min(end), start.max(end))
                })
            });
            let source_focus_time = source_timeline_frame.or_else(|| {
                source_timeline_range.map(|range| (range.start_seconds + range.end_seconds) * 0.5)
            });
            let resolved_source_anchor = source_anchor_pos(
                source_geom.rect,
                source_timeline_frame,
                source_timeline_range,
                track_left,
                zoom,
                scroll_x,
            );
            let source_visible = rects.tracks.contains(resolved_source_anchor);
            let (route_source, portal) = if source_visible {
                (resolved_source_anchor, None)
            } else {
                let portal = binding_edge_portal(
                    resolved_source_anchor,
                    target_anchor,
                    rects.tracks,
                    &allocated_portals,
                );
                allocated_portals.push(portal);
                (portal.icon_rect.center(), Some(portal))
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
                &occupied_segments,
            );
            occupied_segments.extend(binding_occupied_segments(&segments));
            routes.push(TimelineBindingRoute {
                plan,
                required_unbound: visual.required_unbound,
                target_clip_id: clip.id,
                target_rect: target_geom.rect,
                target_anchor,
                target_continuation,
                source_anchor: source_visible.then_some(resolved_source_anchor),
                source_rect: Some(source_geom.rect),
                source_focus_time,
                source_timeline_range,
                source_clip_id: Some(source_clip_id),
                segments,
                portal,
                local_source: None,
                terminal: None,
            });
        }
        separate_coincident_route_targets(&mut routes, rects.tracks);
        routes
    }

    pub(super) fn paint_media_binding_underlay(
        &self,
        painter: &egui::Painter,
        routes: &[TimelineBindingRoute],
    ) {
        if routes.len() > TimelineBindingVisualMetrics::AMBIENT_ROUTE_LIMIT {
            return;
        }
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
        let now = Instant::now();
        let hover = ui.ctx().pointer_hover_pos();
        let hovered_index = hover.and_then(|pos| {
            routes
                .iter()
                .position(|route| binding_route_contains(route, pos, clip_geoms))
        });
        if let Some(index) = hovered_index {
            let route = &routes[index];
            self.timeline_binding_focus = Some(super::TimelineBindingFocus {
                target_clip_id: route.target_clip_id,
                field_name: route.plan.field_name.clone(),
                last_seen: now,
            });
        } else if let Some(focus) = self.timeline_binding_focus.as_ref() {
            let elapsed = now.saturating_duration_since(focus.last_seen);
            if elapsed >= TimelineBindingVisualMetrics::FOCUS_LATCH {
                self.timeline_binding_focus = None;
            } else {
                ui.ctx().request_repaint_after(
                    TimelineBindingVisualMetrics::FOCUS_LATCH.saturating_sub(elapsed),
                );
            }
        }
        let focused_index = self.timeline_binding_focus.as_ref().and_then(|focus| {
            routes.iter().position(|route| {
                route.target_clip_id == focus.target_clip_id
                    && route.plan.field_name == focus.field_name
            })
        });
        let mut reveal_source = None;
        let mut reveal_project_asset = None;
        let mut reveal_frozen = None;
        let mut hover_text = None;
        let mut binding_pointer_captured = false;

        if routes.len() <= TimelineBindingVisualMetrics::AMBIENT_ROUTE_LIMIT {
            for route in routes {
                paint_ambient_route_stems(painter, route);
            }
        }
        if let Some(index) = focused_index {
            if let Some(route) = routes.get(index).filter(|route| route.segments.len() >= 2) {
                paint_focused_route(painter, route);
            }
        }

        for (index, route) in routes.iter().enumerate() {
            let focused = focused_index == Some(index);
            let geometry_missing = route.plan.is_ok()
                && route.source_clip_id.is_some()
                && route.source_anchor.is_none()
                && route.portal.is_none();
            let visually_valid = route.plan.is_ok() && !geometry_missing;
            let port_color = binding_port_color(route.plan.media_type, visually_valid);
            if let Some(continuation) = route.target_continuation {
                paint_target_continuation(painter, continuation, port_color, focused);
            } else {
                paint_target_port(
                    painter,
                    route.target_anchor,
                    &route.plan,
                    port_color,
                    focused,
                    binding_mark_scale(route.target_rect),
                );
            }

            if !visually_valid {
                paint_broken_port(
                    painter,
                    route.target_anchor,
                    binding_mark_scale(route.target_rect),
                );
            }
            if let (Some(local_source), Some(terminal)) = (route.local_source, route.terminal) {
                paint_local_source_terminal(
                    painter,
                    route.target_anchor,
                    local_source,
                    terminal,
                    port_color,
                    visually_valid,
                    focused,
                );
                let response = ui.interact(
                    terminal.rect.expand(2.0),
                    ui.id().with((
                        "timeline-binding-terminal",
                        route.target_clip_id,
                        &route.plan.field_name,
                    )),
                    egui::Sense::click(),
                );
                let response = crate::core::automation::instrument_response(
                    response,
                    "timeline_binding_terminal",
                    Some(format!("Reveal {} source", route.plan.field_label)),
                    true,
                    true,
                );
                binding_pointer_captured |= response.hovered()
                    || response.is_pointer_button_down_on()
                    || response.dragged();
                if response.clicked() {
                    match &route.plan.spec.source {
                        MediaBindingSource::ProjectAsset { asset_id, version } => {
                            reveal_project_asset = Some((
                                *asset_id,
                                version.clone(),
                                route.target_clip_id,
                                route.plan.field_name.clone(),
                            ));
                        }
                        MediaBindingSource::FrozenArtifact { path, origin, .. } => {
                            let absolute = route.plan.source_path_absolute.clone().or_else(|| {
                                if path.is_absolute() {
                                    Some(path.clone())
                                } else {
                                    self.editor
                                        .project
                                        .project_path
                                        .as_ref()
                                        .map(|root| root.join(path))
                                }
                            });
                            let original_clip =
                                origin.as_ref().and_then(|origin| origin.source_clip_id);
                            let source_focus_time = origin.as_ref().and_then(|origin| {
                                original_clip.and_then(|source_clip_id| {
                                    self.timeline_binding_source_focus_time(
                                        source_clip_id,
                                        origin.source_frame_time,
                                        origin.source_range,
                                    )
                                })
                            });
                            let go_to_original = ui.input(|input| input.modifiers.shift);
                            reveal_frozen = Some((
                                absolute,
                                original_clip,
                                source_focus_time,
                                go_to_original,
                                route.target_clip_id,
                                route.plan.field_name.clone(),
                            ));
                        }
                        _ => {}
                    }
                }
            } else if visually_valid {
                if let Some(source_anchor) = route.source_anchor {
                    paint_source_marker(
                        painter,
                        source_anchor,
                        port_color,
                        focused,
                        route.source_rect.map(binding_mark_scale).unwrap_or(1.0),
                    );
                    if let Some(range) = route.source_timeline_range {
                        if let Some(source_rect) = route.source_clip_id.and_then(|clip_id| {
                            self.timeline_binding_source_rect(clip_id, rects, zoom)
                        }) {
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
            }

            if let Some(portal) = route.portal {
                paint_edge_portal(painter, portal, port_color, focused);
                let response = ui.interact(
                    portal.icon_rect,
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
                binding_pointer_captured |= response.hovered()
                    || response.is_pointer_button_down_on()
                    || response.dragged();
                if response.clicked() {
                    reveal_source = route.source_clip_id.map(|source_clip_id| {
                        (
                            source_clip_id,
                            route.target_clip_id,
                            route.plan.field_name.clone(),
                            route.source_focus_time,
                        )
                    });
                }
            }

            if focused && hovered_index == Some(index) {
                if let Some(pos) = hover {
                    let text = if route.required_unbound {
                        format!(
                            "{}\nRequired input has no source.\nChoose a source in the inspector.",
                            route.plan.field_label
                        )
                    } else {
                        let mut text = binding_hover_text(&self.editor.project, &route.plan);
                        match route.local_source {
                            Some(BindingLocalSource::ProjectAsset) => {
                                text.push_str(
                                    "\nClick to reveal this asset or exact Asset Lab output.",
                                );
                            }
                            Some(BindingLocalSource::FrozenArtifact) => {
                                text.push_str(
                                    "\nClick to reveal the frozen file. Shift-click to go to its original timeline source.",
                                );
                            }
                            None => {}
                        }
                        text
                    };
                    hover_text = Some((pos, text));
                }
            }
        }

        if let Some((source_clip_id, consumer_clip_id, field_name, source_focus_time)) =
            reveal_source
        {
            self.reveal_timeline_binding_source(
                source_clip_id,
                consumer_clip_id,
                field_name,
                source_focus_time,
                rects,
                zoom,
            );
        }
        if let Some((asset_id, version, consumer_clip_id, field_name)) = reveal_project_asset {
            self.reveal_timeline_binding_project_asset(
                asset_id,
                version.as_deref(),
                consumer_clip_id,
                field_name,
            );
        }
        if let Some((
            path,
            original_clip,
            source_focus_time,
            go_to_original,
            consumer_clip_id,
            field_name,
        )) = reveal_frozen
        {
            if go_to_original {
                if let Some(source_clip_id) = original_clip {
                    self.reveal_timeline_binding_source(
                        source_clip_id,
                        consumer_clip_id,
                        field_name,
                        source_focus_time,
                        rects,
                        zoom,
                    );
                } else {
                    self.editor.status =
                        "This frozen input has no recorded original timeline clip.".to_string();
                }
            } else if let Some(path) = path {
                if !path.exists() {
                    self.editor.status =
                        format!("Frozen input file is missing: {}", path.display());
                } else if let Err(err) = super::reveal_path_in_file_manager(&path) {
                    self.editor.status = err;
                } else {
                    self.editor.status = "Revealed frozen input in File Explorer".to_string();
                }
            } else {
                self.editor.status =
                    "The project folder is unavailable, so this frozen input cannot be revealed."
                        .to_string();
            }
        }
        if let Some((origin, text)) = hover_text {
            paint_binding_tooltip(painter, origin, &text, rects.tracks);
        }
        binding_pointer_captured
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
        consumer_clip_id: uuid::Uuid,
        field_name: String,
        source_focus_time: Option<f64>,
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
        let center_time = source_focus_time
            .filter(|time| time.is_finite())
            .unwrap_or_else(|| (clip.start_time + clip.end_time()) * 0.5)
            .clamp(clip.start_time, clip.end_time());
        self.timeline_binding_navigation_origin = Some(super::TimelineBindingNavigationOrigin {
            consumer_clip_id,
            field_name,
            prior_clip_selection: self.editor.selection.clip_ids.clone(),
            prior_scroll_x: self.editor.layout.timeline_scroll_x,
            prior_scroll_y: self.editor.layout.timeline_scroll_y,
        });
        let max_scroll_x =
            (self.editor.project.duration() as f32 * zoom - rects.tracks.width()).max(0.0);
        self.editor.layout.timeline_scroll_x =
            (center_time as f32 * zoom - rects.tracks.width() * 0.5).clamp(0.0, max_scroll_x);
        let content_h = self.editor.project.tracks.len() as f32 * super::TIMELINE_TRACK_H;
        let max_scroll_y = (content_h - rects.tracks.height()).max(0.0);
        self.editor.layout.timeline_scroll_y = (row as f32 * super::TIMELINE_TRACK_H
            + super::TIMELINE_TRACK_H * 0.5
            - rects.tracks.height() * 0.5)
            .clamp(0.0, max_scroll_y);
        self.editor.selection.select_clip(clip_id);
        self.editor.status = "Revealed binding source on the timeline".to_string();
    }

    pub(super) fn return_to_timeline_binding_consumer(&mut self) {
        let Some(origin) = self.timeline_binding_navigation_origin.take() else {
            return;
        };
        let clip_exists = |clip_id| {
            self.editor
                .project
                .clips
                .iter()
                .any(|clip| clip.id == clip_id)
        };
        if !clip_exists(origin.consumer_clip_id)
            || origin
                .prior_clip_selection
                .iter()
                .copied()
                .any(|clip_id| !clip_exists(clip_id))
        {
            self.timeline_binding_focus = None;
            self.editor.status =
                "The prior timeline selection changed, so Back to Consumer is no longer available."
                    .to_string();
            return;
        }
        self.editor.layout.timeline_scroll_x = origin.prior_scroll_x.max(0.0);
        self.editor.layout.timeline_scroll_y = origin.prior_scroll_y.max(0.0);
        self.editor.selection.clear();
        self.editor.selection.clip_ids = if origin.prior_clip_selection.is_empty() {
            vec![origin.consumer_clip_id]
        } else {
            origin.prior_clip_selection
        };
        self.timeline_binding_focus = None;
        self.editor.status = format!("Returned to binding consumer · {}", origin.field_name);
    }

    fn reveal_timeline_binding_project_asset(
        &mut self,
        asset_id: uuid::Uuid,
        version: Option<&str>,
        consumer_clip_id: uuid::Uuid,
        field_name: String,
    ) {
        let Some(asset) = self.editor.project.find_asset(asset_id) else {
            self.editor.status = "The bound project asset no longer exists.".to_string();
            return;
        };
        let navigation_origin = super::TimelineBindingNavigationOrigin {
            consumer_clip_id,
            field_name,
            prior_clip_selection: self.editor.selection.clip_ids.clone(),
            prior_scroll_x: self.editor.layout.timeline_scroll_x,
            prior_scroll_y: self.editor.layout.timeline_scroll_y,
        };
        if asset.is_generative() {
            if let Some(version) = version {
                match self.open_asset_lab_version(asset_id, version) {
                    Ok(()) => {
                        self.timeline_binding_navigation_origin = Some(navigation_origin);
                        self.editor.status = format!("Opened bound output {version} in Asset Lab");
                    }
                    Err(err) => self.editor.status = err,
                }
                return;
            }
        }
        self.timeline_binding_navigation_origin = Some(navigation_origin);
        self.editor.layout.left_collapsed = false;
        self.asset_reveal_override = Some(asset_id);
        self.asset_reveal_scroll_target = Some(asset_id);
        self.editor.selection.select_asset(asset_id);
        self.editor.status = "Revealed bound project asset".to_string();
    }

    fn timeline_binding_source_focus_time(
        &self,
        clip_id: uuid::Uuid,
        source_frame_time: Option<f64>,
        source_range: Option<MediaTimeRange>,
    ) -> Option<f64> {
        let clip = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)?;
        let source_duration = self
            .editor
            .project
            .find_asset(clip.asset_id)
            .and_then(|asset| asset.duration_seconds);
        source_frame_time
            .map(|time| clip.timeline_time_for_source(time, source_duration))
            .or_else(|| {
                source_range.map(|range| {
                    let start = clip.timeline_time_for_source(range.start_seconds, source_duration);
                    let end = clip.timeline_time_for_source(range.end_seconds, source_duration);
                    (start + end) * 0.5
                })
            })
    }

    fn clip_media_binding_plans(&self, clip: &Clip) -> Vec<TimelineBindingVisualPlan> {
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
            if let Some(spec) = lookup_media_binding(config, input, &self.editor.project) {
                plans.push(TimelineBindingVisualPlan {
                    plan: resolve_media_binding(
                        MediaResolveContext {
                            project: &self.editor.project,
                            target_asset_id: Some(clip.asset_id),
                            context_clip_id: Some(clip.id),
                            field: input,
                            provider: Some(provider),
                            config: Some(config),
                        },
                        &spec,
                    ),
                    required_unbound: false,
                });
            } else if input.required {
                let media_type =
                    bound_media_type_for_input(input).expect("media inputs were filtered above");
                plans.push(TimelineBindingVisualPlan {
                    plan: MediaResolvePlan {
                        field_name: input.name.clone(),
                        field_label: input.label.clone(),
                        spec: MediaBindingSpec::default(),
                        normalized_sample: default_sample_for_field(input),
                        media_type,
                        source_media_type: None,
                        stability: MediaBindingStability::Follow,
                        relation: None,
                        source_asset_id: None,
                        source_clip_id: None,
                        source_version: None,
                        source_path: None,
                        source_path_absolute: None,
                        target_range: None,
                        source_range: None,
                        target_frame_time: None,
                        source_frame_time: None,
                        retime_to_duration: None,
                        uses_original_source: false,
                        candidate_count: 0,
                        ranking_explanation: None,
                        diagnostics: Vec::new(),
                        errors: vec![MediaBindingError::SourceMissing {
                            detail: "choose a source".to_string(),
                        }],
                    },
                    required_unbound: true,
                });
            }
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

fn binding_edge_portal(
    source: Pos2,
    target: Pos2,
    viewport: Rect,
    allocated: &[BindingPortalVisual],
) -> BindingPortalVisual {
    let half = TimelineBindingVisualMetrics::PORTAL_SIZE * 0.5;
    let left = safe_axis_in_rect(
        viewport.left() + TimelineBindingVisualMetrics::PORTAL_EDGE_INSET + half,
        viewport.left(),
        viewport.right(),
        half,
    );
    let right = safe_axis_in_rect(
        viewport.right() - TimelineBindingVisualMetrics::PORTAL_SCROLLBAR_RESERVE - half,
        viewport.left(),
        viewport.right(),
        half,
    );
    let top = safe_axis_in_rect(
        viewport.top() + TimelineBindingVisualMetrics::PORTAL_EDGE_INSET + half,
        viewport.top(),
        viewport.bottom(),
        half,
    );
    let bottom = safe_axis_in_rect(
        viewport.bottom() - TimelineBindingVisualMetrics::PORTAL_EDGE_INSET - half,
        viewport.top(),
        viewport.bottom(),
        half,
    );
    let safe = Rect::from_min_max(
        Pos2::new(left.min(right), top.min(bottom)),
        Pos2::new(left.max(right), top.max(bottom)),
    );
    let origin = Pos2::new(
        safe_axis_in_rect(target.x, safe.left(), safe.right(), 0.0),
        safe_axis_in_rect(target.y, safe.top(), safe.bottom(), 0.0),
    );
    let delta = source - origin;
    let mut candidates = Vec::new();
    if delta.x < -f32::EPSILON {
        candidates.push((
            (safe.left() - origin.x) / delta.x,
            BindingPortalDirection::Left,
        ));
    } else if delta.x > f32::EPSILON {
        candidates.push((
            (safe.right() - origin.x) / delta.x,
            BindingPortalDirection::Right,
        ));
    }
    if delta.y < -f32::EPSILON {
        candidates.push((
            (safe.top() - origin.y) / delta.y,
            BindingPortalDirection::Up,
        ));
    } else if delta.y > f32::EPSILON {
        candidates.push((
            (safe.bottom() - origin.y) / delta.y,
            BindingPortalDirection::Down,
        ));
    }
    let (t, direction) = candidates
        .into_iter()
        .filter(|(t, _)| t.is_finite() && *t >= 0.0)
        .min_by(|(left, _), (right, _)| left.total_cmp(right))
        .unwrap_or((0.0, BindingPortalDirection::Right));
    let raw_center = origin + delta * t;
    let center = stack_portal_center(raw_center, direction, safe, allocated);
    BindingPortalVisual {
        direction,
        icon_rect: Rect::from_center_size(
            center,
            Vec2::splat(TimelineBindingVisualMetrics::PORTAL_SIZE),
        ),
    }
}

fn stack_portal_center(
    raw: Pos2,
    direction: BindingPortalDirection,
    safe: Rect,
    allocated: &[BindingPortalVisual],
) -> Pos2 {
    let step =
        TimelineBindingVisualMetrics::PORTAL_SIZE + TimelineBindingVisualMetrics::PORTAL_STACK_GAP;
    for attempt in 0..=allocated.len().min(12) {
        let lane = if attempt == 0 {
            0.0
        } else {
            let magnitude = attempt.div_ceil(2) as f32;
            if attempt % 2 == 1 {
                magnitude
            } else {
                -magnitude
            }
        };
        let mut center = raw;
        match direction {
            BindingPortalDirection::Left | BindingPortalDirection::Right => {
                center.y = safe_axis_in_rect(raw.y + lane * step, safe.top(), safe.bottom(), 0.0);
            }
            BindingPortalDirection::Up | BindingPortalDirection::Down => {
                center.x = safe_axis_in_rect(raw.x + lane * step, safe.left(), safe.right(), 0.0);
            }
        }
        let rect = Rect::from_center_size(
            center,
            Vec2::splat(TimelineBindingVisualMetrics::PORTAL_SIZE),
        );
        if !allocated
            .iter()
            .any(|portal| portal.icon_rect.intersects(rect))
        {
            return center;
        }
    }
    raw
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
    occupied_segments: &[BindingOccupiedSegment],
) -> Vec<Pos2> {
    if let Some(source_rect) = source_rect {
        let same_track = (source_rect.center().y - target_rect.center().y).abs() < 1.5;
        if same_track
            && matches!(
                relation,
                Some(MediaBindingRelation::TouchingPrevious | MediaBindingRelation::TouchingNext)
            )
        {
            return compact_touching_segments(source, target, source_rect, target_rect, viewport);
        }
        if same_track {
            let gutter_y = safe_axis_in_rect(
                source_rect.top().min(target_rect.top()) - 1.5,
                viewport.top(),
                viewport.bottom(),
                1.0,
            );
            return vec![
                source,
                Pos2::new(source.x, gutter_y),
                Pos2::new(target.x, gutter_y),
                target,
            ];
        }
    }

    let source_above = source.y <= target.y;
    let source_gutter_y = safe_axis_in_rect(
        source_rect
            .map(|rect| {
                if source_above {
                    rect.bottom() + 1.5
                } else {
                    rect.top() - 1.5
                }
            })
            .unwrap_or(source.y),
        viewport.top(),
        viewport.bottom(),
        1.0,
    );
    let target_gutter_y = safe_axis_in_rect(
        if source_above {
            target_rect.top() - 1.5
        } else {
            target_rect.bottom() + 1.5
        },
        viewport.top(),
        viewport.bottom(),
        1.0,
    );

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
        let x = safe_axis_in_rect(
            candidate,
            viewport.left(),
            viewport.right() - TimelineBindingVisualMetrics::PORTAL_SCROLLBAR_RESERVE,
            4.0,
        );
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
        score += score_candidate_occupancy(
            x,
            source,
            target,
            source_gutter_y,
            target_gutter_y,
            occupied_segments,
        );
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

fn binding_occupied_segments(points: &[Pos2]) -> Vec<BindingOccupiedSegment> {
    points
        .windows(2)
        .filter_map(|segment| {
            let a = segment[0];
            let b = segment[1];
            if (a.x - b.x).abs() <= 0.5 {
                Some(BindingOccupiedSegment::Vertical {
                    x: (a.x + b.x) * 0.5,
                    y0: a.y.min(b.y),
                    y1: a.y.max(b.y),
                })
            } else if (a.y - b.y).abs() <= 0.5 {
                Some(BindingOccupiedSegment::Horizontal {
                    y: (a.y + b.y) * 0.5,
                    x0: a.x.min(b.x),
                    x1: a.x.max(b.x),
                })
            } else {
                None
            }
        })
        .collect()
}

fn score_candidate_occupancy(
    riser_x: f32,
    source: Pos2,
    target: Pos2,
    source_gutter_y: f32,
    target_gutter_y: f32,
    occupied: &[BindingOccupiedSegment],
) -> f32 {
    let vertical_y0 = source_gutter_y.min(target_gutter_y);
    let vertical_y1 = source_gutter_y.max(target_gutter_y);
    let source_x0 = source.x.min(riser_x);
    let source_x1 = source.x.max(riser_x);
    let target_x0 = target.x.min(riser_x);
    let target_x1 = target.x.max(riser_x);
    occupied.iter().fold(0.0, |mut score, segment| {
        match *segment {
            BindingOccupiedSegment::Vertical { x, y0, y1 } => {
                let vertical_overlap = ranges_overlap(vertical_y0, vertical_y1, y0, y1);
                let distance = (riser_x - x).abs();
                if vertical_overlap && distance < TimelineBindingVisualMetrics::RISER_SEPARATION {
                    score += 90.0 - distance * 8.0;
                }
                if (x >= source_x0
                    && x <= source_x1
                    && source_gutter_y >= y0
                    && source_gutter_y <= y1)
                    || (x >= target_x0
                        && x <= target_x1
                        && target_gutter_y >= y0
                        && target_gutter_y <= y1)
                {
                    score += 28.0;
                }
            }
            BindingOccupiedSegment::Horizontal { y, x0, x1 } => {
                if riser_x >= x0 && riser_x <= x1 && y >= vertical_y0 && y <= vertical_y1 {
                    score += 38.0;
                }
                if (y - source_gutter_y).abs() < 2.0 && ranges_overlap(source_x0, source_x1, x0, x1)
                {
                    score += 65.0;
                }
                if (y - target_gutter_y).abs() < 2.0 && ranges_overlap(target_x0, target_x1, x0, x1)
                {
                    score += 65.0;
                }
            }
        }
        score
    })
}

fn ranges_overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> bool {
    a1 >= b0 && b1 >= a0
}

fn separate_coincident_route_targets(routes: &mut [TimelineBindingRoute], viewport: Rect) {
    let mut visited = vec![false; routes.len()];
    for start in 0..routes.len() {
        if visited[start] {
            continue;
        }
        if routes[start].target_continuation.is_some() {
            visited[start] = true;
            continue;
        }
        let origin = routes[start].target_anchor;
        let mut group = Vec::new();
        for (index, route) in routes.iter().enumerate().skip(start) {
            if !visited[index]
                && route.target_continuation.is_none()
                && (route.target_anchor.x - origin.x).abs() <= 1.5
                && (route.target_anchor.y - origin.y).abs() <= 1.5
            {
                group.push(index);
            }
        }
        if group.len() <= 1 {
            visited[start] = true;
            continue;
        }
        group.sort_by(|left, right| {
            routes[*left]
                .plan
                .field_name
                .cmp(&routes[*right].plan.field_name)
        });
        let usable_height = (routes[start].target_rect.height() - 10.0).max(0.0);
        let step = (usable_height / (group.len() - 1) as f32).min(7.0);
        let center = (group.len() - 1) as f32 * 0.5;
        for (position, index) in group.into_iter().enumerate() {
            visited[index] = true;
            let route = &mut routes[index];
            route.target_anchor.y = safe_axis_in_rect(
                origin.y + (position as f32 - center) * step,
                route.target_rect.top(),
                route.target_rect.bottom(),
                TimelineBindingVisualMetrics::ANCHOR_INSET,
            );
            if let Some(last) = route.segments.last_mut() {
                *last = route.target_anchor;
            }
            if let Some(local_source) = route.local_source {
                route.terminal = Some(local_source_terminal_visual(
                    route.target_anchor,
                    route.target_rect,
                    local_source,
                    viewport,
                ));
            }
        }
    }
}

fn paint_target_port(
    painter: &egui::Painter,
    anchor: Pos2,
    plan: &MediaResolvePlan,
    color: Color32,
    focused: bool,
    scale: f32,
) {
    painter.circle_stroke(
        anchor,
        (if focused { 8.0 } else { 7.0 }) * scale,
        Stroke::new(
            if focused { 2.0_f32 } else { 1.2_f32 },
            color.gamma_multiply(if focused { 0.35 } else { 0.16 }),
        ),
    );
    match plan.normalized_sample {
        MediaSample::AlignedRange | MediaSample::SourceRange { .. } | MediaSample::Whole => {
            let left = anchor - Vec2::new(7.0 * scale, 0.0);
            let right = anchor + Vec2::new(7.0 * scale, 0.0);
            painter.line_segment([left, right], Stroke::new(2.0_f32, color));
            painter.line_segment(
                [
                    left - Vec2::new(0.0, 3.0 * scale),
                    left + Vec2::new(0.0, 3.0 * scale),
                ],
                Stroke::new(1.5_f32, color),
            );
            painter.line_segment(
                [
                    right - Vec2::new(0.0, 3.0 * scale),
                    right + Vec2::new(0.0, 3.0 * scale),
                ],
                Stroke::new(1.5_f32, color),
            );
        }
        _ => paint_binding_diamond(painter, anchor, 4.5 * scale, color),
    }
}

fn paint_source_marker(
    painter: &egui::Painter,
    anchor: Pos2,
    color: Color32,
    focused: bool,
    scale: f32,
) {
    painter.circle_stroke(
        anchor,
        (if focused { 8.0 } else { 7.0 }) * scale,
        Stroke::new(
            if focused { 2.0_f32 } else { 1.2_f32 },
            color.gamma_multiply(if focused { 0.35 } else { 0.16 }),
        ),
    );
    painter.circle_filled(anchor, 3.25 * scale, color);
    painter.circle_stroke(
        anchor,
        4.75 * scale,
        Stroke::new(1.0_f32, kit::PANEL_SUNKEN),
    );
}

fn paint_broken_port(painter: &egui::Painter, anchor: Pos2, scale: f32) {
    painter.circle_stroke(anchor, 5.5 * scale, Stroke::new(1.8_f32, kit::DANGER));
    painter.line_segment(
        [
            anchor - Vec2::splat(3.5 * scale),
            anchor + Vec2::splat(3.5 * scale),
        ],
        Stroke::new(1.5_f32, kit::DANGER),
    );
}

fn paint_local_source_terminal(
    painter: &egui::Painter,
    target: Pos2,
    source: BindingLocalSource,
    terminal: BindingTerminalVisual,
    color: Color32,
    valid: bool,
    focused: bool,
) {
    let connector = Stroke::new(
        1.4_f32,
        color.gamma_multiply(if valid { 0.75 } else { 0.45 }),
    );
    let delta = terminal.anchor - target;
    let distance = delta.length();
    if distance <= 28.0 {
        painter.line_segment([target, terminal.anchor], connector);
    } else if distance > f32::EPSILON {
        let unit = delta / distance;
        painter.line_segment([target, target + unit * 8.0], connector);
        painter.line_segment([terminal.anchor - unit * 8.0, terminal.anchor], connector);
    }
    painter.rect_filled(terminal.rect, 3.0, kit::PANEL_RAISED);
    painter.rect_stroke(
        terminal.rect,
        3.0,
        Stroke::new(if focused { 1.8_f32 } else { 1.2_f32 }, color),
        egui::StrokeKind::Inside,
    );
    painter.text(
        terminal.rect.center(),
        egui::Align2::CENTER_CENTER,
        match source {
            BindingLocalSource::ProjectAsset => "ASSET",
            BindingLocalSource::FrozenArtifact => "FROZEN",
        },
        FontId::monospace(7.5),
        color,
    );
}

fn paint_edge_portal(
    painter: &egui::Painter,
    portal: BindingPortalVisual,
    color: Color32,
    focused: bool,
) {
    painter.rect_filled(portal.icon_rect, 4.0, kit::PANEL_RAISED);
    painter.rect_stroke(
        portal.icon_rect,
        4.0,
        Stroke::new(if focused { 1.8_f32 } else { 1.2_f32 }, color),
        egui::StrokeKind::Inside,
    );
    painter.text(
        portal.icon_rect.center(),
        egui::Align2::CENTER_CENTER,
        match portal.direction {
            BindingPortalDirection::Left => "<",
            BindingPortalDirection::Right => ">",
            BindingPortalDirection::Up => "^",
            BindingPortalDirection::Down => "v",
        },
        FontId::monospace(10.0),
        color,
    );
}

fn paint_target_continuation(
    painter: &egui::Painter,
    continuation: BindingPortalVisual,
    color: Color32,
    focused: bool,
) {
    let rect = continuation.icon_rect;
    painter.rect_filled(rect, 4.0, kit::PANEL_SUNKEN.gamma_multiply(0.96));
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(if focused { 1.8_f32 } else { 1.2_f32 }, color),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        match continuation.direction {
            BindingPortalDirection::Left => "<|",
            BindingPortalDirection::Right => "|>",
            BindingPortalDirection::Up => "^|",
            BindingPortalDirection::Down => "v|",
        },
        FontId::monospace(8.0),
        color,
    );
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
    fps: f64,
) -> Pos2 {
    let time = semantic_target_time(clip, plan, fps);
    let x = safe_axis_in_rect(
        time_to_timeline_x(time, track_left, zoom, scroll_x),
        rect.left(),
        rect.right(),
        TimelineBindingVisualMetrics::ANCHOR_INSET,
    );
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
    source_timeline_frame: Option<f64>,
    source_timeline_range: Option<MediaTimeRange>,
    track_left: f32,
    zoom: f32,
    scroll_x: f32,
) -> Pos2 {
    let time = source_timeline_frame.or_else(|| {
        source_timeline_range.map(|range| (range.start_seconds + range.end_seconds) * 0.5)
    });
    let x = safe_axis_in_rect(
        time.map(|time| time_to_timeline_x(time, track_left, zoom, scroll_x))
            .unwrap_or(rect.center().x),
        rect.left(),
        rect.right(),
        TimelineBindingVisualMetrics::ANCHOR_INSET,
    );
    Pos2::new(x, rect.center().y)
}

fn semantic_target_time(clip: &Clip, plan: &MediaResolvePlan, fps: f64) -> f64 {
    if let Some(time) = plan.target_frame_time {
        return time;
    }
    if let Some(range) = plan.target_range {
        return (range.start_seconds + range.end_seconds) * 0.5;
    }
    match &plan.normalized_sample {
        MediaSample::Frame { at } => match at {
            MediaFramePoint::OutputEnd => clip.end_time(),
            MediaFramePoint::OutputOffset { seconds } => clip.start_time + *seconds,
            MediaFramePoint::OutputFrame { frame } => {
                clip.start_time + (*frame as f64 / fps.max(1.0))
            }
            MediaFramePoint::OutputStart
            | MediaFramePoint::SourceStart
            | MediaFramePoint::SourceEnd
            | MediaFramePoint::SourceTime { .. } => clip.start_time,
        },
        MediaSample::AlignedRange | MediaSample::SourceRange { .. } | MediaSample::Whole => {
            (clip.start_time + clip.end_time()) * 0.5
        }
        MediaSample::Auto => clip.start_time,
    }
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

fn compact_touching_segments(
    source: Pos2,
    target: Pos2,
    source_rect: Rect,
    target_rect: Rect,
    viewport: Rect,
) -> Vec<Pos2> {
    let top = source_rect.top().min(target_rect.top()) - 1.5;
    let bottom = source_rect.bottom().max(target_rect.bottom()) + 1.5;
    let use_top = top >= viewport.top() + 1.0
        || (viewport.bottom() - bottom).abs() > (viewport.top() - top).abs();
    let apex_y = safe_axis_in_rect(
        if use_top { top } else { bottom },
        viewport.top(),
        viewport.bottom(),
        1.0,
    );
    vec![
        source,
        Pos2::new(source.x, apex_y),
        Pos2::new(target.x, apex_y),
        target,
    ]
}

fn safe_axis_in_rect(value: f32, start: f32, end: f32, requested_inset: f32) -> f32 {
    let (low, high) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    if !low.is_finite() || !high.is_finite() {
        return 0.0;
    }
    let width = (high - low).max(0.0);
    let inset = requested_inset.max(0.0).min(width * 0.5);
    let min = low + inset;
    let max = high - inset;
    if !value.is_finite() || max - min <= f32::EPSILON {
        (low + high) * 0.5
    } else {
        value.clamp(min, max)
    }
}

fn binding_mark_scale(rect: Rect) -> f32 {
    if rect.width() < 12.0 {
        0.5
    } else if rect.width() < 28.0 {
        0.72
    } else {
        1.0
    }
}

fn local_source_terminal_visual(
    target: Pos2,
    target_rect: Rect,
    source: BindingLocalSource,
    viewport: Rect,
) -> BindingTerminalVisual {
    let preferred_width: f32 = match source {
        BindingLocalSource::ProjectAsset => 38.0,
        BindingLocalSource::FrozenArtifact => 46.0,
    };
    let scrollbar_reserve = TimelineBindingVisualMetrics::PORTAL_SCROLLBAR_RESERVE
        .min((viewport.width() - 4.0).max(0.0));
    let visible_right = (viewport.right() - scrollbar_reserve).max(viewport.left());
    let visible_width = (visible_right - viewport.left()).max(0.0);
    let horizontal_padding = 2.0_f32.min(visible_width * 0.5);
    let width = preferred_width.min((visible_width - horizontal_padding * 2.0).max(0.0));
    let height = 14.0_f32.min(viewport.height().max(0.0));
    let size = Vec2::new(width, height);
    let gap = TimelineBindingVisualMetrics::TERMINAL_GAP;
    let left_center = Pos2::new(target_rect.left() - gap - width * 0.5, target.y);
    let right_center = Pos2::new(target_rect.right() + gap + width * 0.5, target.y);
    let left_rect = Rect::from_center_size(left_center, size);
    let right_rect = Rect::from_center_size(right_center, size);
    let terminal_viewport = Rect::from_min_max(
        viewport.left_top(),
        Pos2::new(visible_right, viewport.bottom()),
    );
    let center = if terminal_viewport.contains(left_rect.left_top())
        && terminal_viewport.contains(left_rect.right_bottom())
    {
        left_center
    } else if terminal_viewport.contains(right_rect.left_top())
        && terminal_viewport.contains(right_rect.right_bottom())
    {
        right_center
    } else {
        let on_left_edge = target_rect.center().x >= terminal_viewport.center().x;
        Pos2::new(
            if on_left_edge {
                terminal_viewport.left() + width * 0.5 + horizontal_padding
            } else {
                terminal_viewport.right() - width * 0.5 - horizontal_padding
            },
            safe_axis_in_rect(
                target.y,
                terminal_viewport.top(),
                terminal_viewport.bottom(),
                size.y * 0.5,
            ),
        )
    };
    let rect = Rect::from_center_size(center, size);
    let anchor = if center.x < target.x {
        rect.right_center()
    } else {
        rect.left_center()
    };
    BindingTerminalVisual { rect, anchor }
}

fn paint_lock_mark(painter: &egui::Painter, center: Pos2, color: Color32) {
    let body = Rect::from_center_size(center + Vec2::new(0.0, 1.5), Vec2::new(7.0, 5.5));
    let outline = kit::PANEL_SUNKEN.gamma_multiply(0.95);
    painter.circle_stroke(
        center + Vec2::new(0.0, -2.2),
        2.4,
        Stroke::new(3.1_f32, outline),
    );
    painter.circle_stroke(
        center + Vec2::new(0.0, -2.2),
        2.4,
        Stroke::new(1.3_f32, color),
    );
    painter.rect_filled(body.expand(1.0), 1.5, outline);
    painter.rect_filled(body, 1.0, color);
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

fn binding_route_contains(
    route: &TimelineBindingRoute,
    pos: Pos2,
    clip_geoms: &[TimelineClipGeom],
) -> bool {
    let target_radius = 9.0 * binding_mark_scale(route.target_rect);
    route
        .target_continuation
        .is_some_and(|continuation| continuation.icon_rect.expand(2.0).contains(pos))
        || pos.distance(route.target_anchor) <= target_radius.max(5.0)
        || route
            .source_anchor
            .is_some_and(|source| pos.distance(source) <= 9.0)
        || route
            .portal
            .is_some_and(|portal| portal.icon_rect.expand(2.0).contains(pos))
        || route
            .terminal
            .is_some_and(|terminal| terminal.rect.expand(2.0).contains(pos))
        || (!route.segments.is_empty()
            && distance_to_polyline(pos, &route.segments)
                <= TimelineBindingVisualMetrics::ROUTE_HOVER_RADIUS
            && !clip_geoms.iter().any(|geom| {
                geom.clip_id != route.target_clip_id
                    && Some(geom.clip_id) != route.source_clip_id
                    && geom.rect.contains(pos)
            }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inset_clamp_collapses_safely_for_zero_and_narrow_rects() {
        assert_eq!(safe_axis_in_rect(12.0, 5.0, 5.0, 3.0), 5.0);
        assert_eq!(safe_axis_in_rect(20.0, 5.0, 7.0, 3.0), 6.0);
        assert_eq!(safe_axis_in_rect(f32::NAN, 5.0, 7.0, 1.0), 6.0);
    }

    #[test]
    fn portal_geometry_is_safe_in_tiny_viewports() {
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(2.0, 2.0));
        let portal = binding_edge_portal(Pos2::new(100.0, 1.0), Pos2::new(1.0, 1.0), viewport, &[]);
        assert!(portal.icon_rect.center().x.is_finite());
        assert!(portal.icon_rect.center().y.is_finite());
    }

    #[test]
    fn local_terminal_fallback_stays_inside_the_painted_viewport() {
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 80.0));
        let target_rect = Rect::from_min_max(Pos2::new(0.0, 20.0), Pos2::new(100.0, 50.0));
        let terminal = local_source_terminal_visual(
            target_rect.center(),
            target_rect,
            BindingLocalSource::FrozenArtifact,
            viewport,
        );

        assert!(viewport.contains(terminal.rect.left_top()));
        assert!(viewport.contains(terminal.rect.right_bottom()));
        assert!(terminal.rect.right() <= 82.0);
    }
}
