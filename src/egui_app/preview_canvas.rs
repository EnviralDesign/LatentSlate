use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreviewSafeAction {
    ShowAsset(Uuid),
    OpenAssetLab(Uuid),
}

struct PreviewOperation {
    presentation: kit::OperationPresentation,
    action: Option<PreviewSafeAction>,
}

fn preview_operation(
    report: &PreviewRenderReport,
    pending: bool,
    retained_frame: bool,
    scrubbing: bool,
) -> Option<PreviewOperation> {
    use kit::{OperationPhase as Phase, OperationPresentation, OperationSeverity as Severity};

    if scrubbing || pending {
        return Some(PreviewOperation {
            presentation: OperationPresentation::new(
                Phase::Running,
                Severity::Informational,
                if scrubbing {
                    "Scrubbing preview."
                } else if retained_frame {
                    "Updating preview."
                } else {
                    "Rendering preview."
                },
            )
            .detail(if scrubbing {
                "Release the playhead to settle on the final preview frame."
            } else if retained_frame {
                "The previous frame remains visible while the update renders."
            } else {
                "You can continue working elsewhere while this frame renders."
            }),
            action: None,
        });
    }

    if report.rendered_visual_clips > 0 {
        if report.issues.is_empty() {
            return None;
        }
        let issue = &report.issues[0];
        let mut presentation = OperationPresentation::new(
            Phase::Succeeded,
            Severity::Warning,
            "Preview ready with warnings.",
        )
        .detail(format!(
            "{} rendered; {} could not be shown.",
            report.rendered_visual_clips,
            report.issues.len()
        ));
        if let Some(detail) = issue.detail.as_deref() {
            presentation = presentation.technical_detail(detail);
        }
        return Some(PreviewOperation {
            presentation: presentation.primary_action("Show Asset"),
            action: Some(PreviewSafeAction::ShowAsset(issue.asset_id)),
        });
    }

    let Some(issue) = report.issues.first() else {
        return Some(PreviewOperation {
            presentation: OperationPresentation::new(
                Phase::Idle,
                Severity::Neutral,
                "No visual content at the playhead.",
            )
            .detail("Move the playhead onto an image or video clip to preview it."),
            action: None,
        });
    };

    let (phase, severity, title, detail, action_label, action) = match issue.kind {
        PreviewRenderIssueKind::NoAvailableFrame => (
            Phase::Waiting,
            Severity::Warning,
            "No frame is available for this asset.",
            format!("{} has no active source version.", issue.asset_name),
            "Open Asset Lab",
            PreviewSafeAction::OpenAssetLab(issue.asset_id),
        ),
        PreviewRenderIssueKind::MissingMedia => (
            Phase::Blocked,
            Severity::Warning,
            "Source media is missing.",
            format!("{} cannot be shown at the playhead.", issue.asset_name),
            "Show Asset",
            PreviewSafeAction::ShowAsset(issue.asset_id),
        ),
        PreviewRenderIssueKind::DecodeFailed => (
            Phase::Failed,
            Severity::Error,
            "Preview rendering failed.",
            format!("{} could not be decoded.", issue.asset_name),
            "Show Asset",
            PreviewSafeAction::ShowAsset(issue.asset_id),
        ),
        PreviewRenderIssueKind::Unsupported => (
            Phase::Blocked,
            Severity::Warning,
            "This media cannot be previewed.",
            format!("{} uses an unsupported preview state.", issue.asset_name),
            "Show Asset",
            PreviewSafeAction::ShowAsset(issue.asset_id),
        ),
    };
    let mut presentation = OperationPresentation::new(phase, severity, title)
        .detail(detail)
        .primary_action(action_label);
    if let Some(technical_detail) = issue.detail.as_deref() {
        presentation = presentation.technical_detail(technical_detail);
    }
    Some(PreviewOperation {
        presentation,
        action: Some(action),
    })
}

impl LatentSlateApp {
    pub(super) fn central_preview(&mut self, root: &mut Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(kit::PANEL_SUNKEN))
            .show_inside(root, |ui| {
                let header_h = 30.0;
                let (header_rect, _) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), header_h), Sense::hover());
                ui.painter().rect_filled(header_rect, 0.0, kit::CHROME);
                ui.painter().line_segment(
                    [header_rect.left_bottom(), header_rect.right_bottom()],
                    Stroke::new(1.0_f32, kit::BORDER),
                );
                let header_inner = header_rect.shrink2(Vec2::new(14.0, 0.0));
                let mut controls_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(header_inner)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                controls_ui.spacing_mut().item_spacing.x = kit::FORM_ROW_GAP;
                controls_ui.label(kit::section_label("Preview"));
                if kit::timeline_tool_text_button(
                    &mut controls_ui,
                    "Auto",
                    44.0,
                    self.preview_auto_fit,
                )
                .on_hover_text("Auto-fit preview canvas")
                .clicked()
                {
                    self.preview_auto_fit = !self.preview_auto_fit;
                    if self.preview_auto_fit {
                        self.preview_pan = Vec2::ZERO;
                    }
                }
                let pending = self.preview_render_busy_since.is_some();
                let retained = self.preview_layers.is_some()
                    && self.preview_render_report.rendered_visual_clips > 0;
                let scrubbing = matches!(self.timeline_drag.as_ref(), Some(TimelineDrag::Playhead));
                if let Some(operation) =
                    preview_operation(&self.preview_render_report, pending, retained, scrubbing)
                {
                    self.preview_operation_status(
                        &mut controls_ui,
                        operation,
                        header_inner.width(),
                    );
                }
                let s = &self.editor.project.settings;
                ui.painter().text(
                    header_inner.right_center(),
                    egui::Align2::RIGHT_CENTER,
                    format!(
                        "{} x {} @ {}",
                        s.width,
                        s.height,
                        format_preview_frame_rate(s.fps)
                    ),
                    FontId::monospace(11.0),
                    kit::TEXT_DIM,
                );
                let available = ui.available_size();
                let preview_height = available.y.max(160.0);
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(available.x, preview_height),
                    Sense::click_and_drag(),
                );
                self.paint_preview(ui, rect.shrink(8.0), &response);
            });
    }

    pub(super) fn paint_preview(&mut self, ui: &mut Ui, rect: Rect, response: &egui::Response) {
        let painter = ui.painter().with_clip_rect(rect);
        if let Some(layers) = self.preview_layers.clone() {
            let fit_scale = preview_fit_scale(rect, &layers);
            self.handle_preview_view_input(ui, response, rect, &layers, fit_scale);
            let scale = self.preview_canvas_screen_scale(fit_scale);
            let canvas_size = Vec2::new(
                layers.canvas_width as f32 * scale,
                layers.canvas_height as f32 * scale,
            );
            let canvas_rect = Rect::from_center_size(rect.center() + self.preview_pan, canvas_size);
            let layer_painter = painter.with_clip_rect(canvas_rect.intersect(rect));
            for layer in layers.layers.iter() {
                let Some(texture) = self.preview_layer_textures.get(&layer.texture_key) else {
                    continue;
                };
                let placement = layer.placement;
                let layer_rect = Rect::from_min_size(
                    canvas_rect.min
                        + Vec2::new(placement.offset_x * scale, placement.offset_y * scale),
                    Vec2::new(placement.scaled_w * scale, placement.scaled_h * scale),
                );
                let alpha = (placement.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
                let tint = Color32::from_white_alpha(alpha);
                if placement.rotation_deg.abs() <= 0.01 {
                    layer_painter.image(
                        texture.texture.id(),
                        layer_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        tint,
                    );
                } else {
                    paint_rotated_texture(
                        &layer_painter,
                        texture.texture.id(),
                        layer_rect,
                        placement.rotation_deg,
                        tint,
                    );
                }
            }
            let mut object_geometries = self.preview_object_geometries(&layers, canvas_rect, scale);
            object_geometries.extend(self.paint_preview_keyframe_reference_ghosts(
                ui,
                &layer_painter,
                canvas_rect,
                &layers,
                scale,
            ));
            self.paint_preview_transform_overlay(
                ui,
                rect,
                canvas_rect,
                &layers,
                &object_geometries,
            );
        } else {
            painter.rect_filled(rect, 0.0, kit::PANEL_SUNKEN);
        }

        if self.editor.layout.preview_stats {
            if let Some(stats) = &self.preview_stats {
                let cache = self.editor.previewer.cache_stats();
                let cache_mb = cache.total_bytes as f64 / (1024.0 * 1024.0);
                let cache_max_mb = cache.max_bytes as f64 / (1024.0 * 1024.0);
                let render_state = if self.preview_render_in_flight.load(Ordering::Relaxed) {
                    format!(
                        "busy {:.1}ms",
                        self.preview_render_busy_since
                            .map(|start| start.elapsed().as_secs_f64() * 1000.0)
                            .unwrap_or_default()
                    )
                } else {
                    "idle".to_string()
                };
                let hit_total = stats.cache_hits + stats.cache_misses;
                let hit_rate = if hit_total > 0 {
                    stats.cache_hits as f64 / hit_total as f64 * 100.0
                } else {
                    0.0
                };
                let text = format!(
                    concat!(
                        "async {}\n",
                        "worker {:.1}ms  delivery {:.1}ms\n",
                        "total {:.1}ms  upload {:.1}ms\n",
                        "scan {:.1}ms  comp {:.1}ms\n",
                        "vdec {:.1}ms  wait {:.1}ms\n",
                        "still {:.1}ms\n",
                        "  seek {:.1}  pkt {:.1}\n",
                        "  xfer {:.1}  scale {:.1}  copy {:.1}\n",
                        "cache {:.0}/{:.0}MB  entries {}\n",
                        "hit {} miss {} ({:.0}%)\n",
                        "indexed assets {} frames {}\n",
                        "layers {}  stale {}"
                    ),
                    render_state,
                    self.preview_render_last_worker_ms.unwrap_or_default(),
                    self.preview_render_last_delivery_ms.unwrap_or_default(),
                    stats.total_ms,
                    stats.encode_ms,
                    stats.collect_ms,
                    stats.composite_ms,
                    stats.video_decode_ms,
                    stats.video_decode_queue_ms,
                    stats.still_load_ms,
                    stats.video_decode_seek_ms,
                    stats.video_decode_packet_ms,
                    stats.video_decode_transfer_ms,
                    stats.video_decode_scale_ms,
                    stats.video_decode_copy_ms,
                    cache_mb,
                    cache_max_mb,
                    cache.entry_count,
                    stats.cache_hits,
                    stats.cache_misses,
                    hit_rate,
                    cache.indexed_asset_count,
                    cache.indexed_frame_count,
                    stats.layers,
                    self.preview_render_stale_count,
                );
                let stats_rect = Rect::from_min_size(
                    rect.right_top() + Vec2::new(-274.0, 12.0),
                    Vec2::new(258.0, 188.0),
                );
                ui.painter().rect_filled(
                    stats_rect,
                    6.0,
                    Color32::from_rgba_unmultiplied(13, 14, 16, 220),
                );
                ui.painter().rect_stroke(
                    stats_rect,
                    6.0,
                    Stroke::new(1.0_f32, kit::BORDER_SOFT),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    stats_rect.min + Vec2::new(10.0, 8.0),
                    egui::Align2::LEFT_TOP,
                    text,
                    FontId::monospace(11.0),
                    kit::TEXT_MUTED,
                );
            }
        }
    }

    fn preview_operation_status(
        &mut self,
        ui: &mut Ui,
        operation: PreviewOperation,
        header_width: f32,
    ) {
        let pill = kit::operation_status_pill(
            ui,
            operation.presentation.phase,
            operation.presentation.severity,
        );
        let mut trigger_rect = pill.rect;
        let title_width = (header_width - 330.0).clamp(0.0, 190.0);
        if title_width >= 48.0 {
            let title = ui.add_sized(
                [title_width, 20.0],
                egui::Label::new(
                    RichText::new(&operation.presentation.title)
                        .color(kit::TEXT_MUTED)
                        .size(10.5),
                )
                .truncate(),
            );
            trigger_rect = trigger_rect.union(title.rect);
        }

        let hover_text = operation
            .presentation
            .detail
            .as_deref()
            .unwrap_or(&operation.presentation.title);
        let trigger = crate::core::automation::instrument_response(
            ui.interact(
                trigger_rect,
                ui.make_persistent_id("preview_operation_status"),
                Sense::click(),
            ),
            "preview_status",
            Some(operation.presentation.title.clone()),
            true,
            false,
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hover_text);
        let mut copied_details = false;
        let mut requested_action = None;
        egui::Popup::menu(&trigger)
            .id(trigger.id.with("details"))
            .align(egui::RectAlign::BOTTOM_START)
            .gap(4.0)
            .width(360.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_max_width(360.0);
                let response = kit::operation_banner(
                    ui,
                    "preview_header_operation_details",
                    &operation.presentation,
                );
                copied_details = response.technical_detail_copied;
                if response.primary_clicked {
                    requested_action = operation.action;
                    ui.close();
                }
            });

        if copied_details {
            self.editor.status = "Copied preview details.".to_string();
        }
        if let Some(action) = requested_action {
            self.apply_preview_safe_action(action);
        }
    }

    fn apply_preview_safe_action(&mut self, action: PreviewSafeAction) {
        match action {
            PreviewSafeAction::ShowAsset(asset_id) => {
                self.editor.selection.asset_ids = vec![asset_id];
                self.asset_reveal_override = Some(asset_id);
                self.asset_reveal_scroll_target = Some(asset_id);
                self.editor.layout.left_collapsed = false;
                self.editor.status = "Revealed preview source in Assets.".to_string();
            }
            PreviewSafeAction::OpenAssetLab(asset_id) => {
                self.open_asset_lab(asset_id);
            }
        }
    }

    pub(super) fn preview_canvas_screen_scale(&mut self, fit_scale: f32) -> f32 {
        if self.preview_auto_fit {
            self.preview_zoom = fit_scale;
            self.preview_pan = Vec2::ZERO;
            fit_scale
        } else {
            if !self.preview_zoom.is_finite() || self.preview_zoom <= 0.0 {
                self.preview_zoom = fit_scale;
            }
            self.preview_zoom
                .clamp(PREVIEW_ZOOM_MIN.max(fit_scale * 0.1), PREVIEW_ZOOM_MAX)
        }
    }

    pub(super) fn handle_preview_view_input(
        &mut self,
        ui: &mut Ui,
        _response: &egui::Response,
        rect: Rect,
        layers: &PreviewLayerStack,
        fit_scale: f32,
    ) {
        if self.modal_background_input_blocked() {
            return;
        }
        let pointer = ui
            .ctx()
            .pointer_interact_pos()
            .or_else(|| ui.ctx().pointer_hover_pos());
        let pointer_in_preview = pointer.map(|point| rect.contains(point)).unwrap_or(false);
        let secondary_pressed_in_preview =
            ui.input(|input| input.pointer.secondary_pressed()) && pointer_in_preview;
        if secondary_pressed_in_preview {
            if let Some(start_pointer) = pointer {
                self.preview_auto_fit = false;
                if !self.preview_zoom.is_finite() || self.preview_zoom <= 0.0 {
                    self.preview_zoom = fit_scale;
                }
                self.preview_drag = Some(PreviewTransformDrag::Pan {
                    start_pan: self.preview_pan,
                    start_pointer,
                });
            }
        }
        if let Some(PreviewTransformDrag::Pan {
            start_pan,
            start_pointer,
        }) = self.preview_drag
        {
            if ui.input(|input| input.pointer.secondary_down()) {
                if let Some(pointer) = pointer {
                    self.preview_pan = start_pan + (pointer - start_pointer);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::AllScroll);
                    ui.ctx().request_repaint();
                }
            } else {
                self.preview_drag = None;
            }
        }

        let scroll_delta = preview_scroll_delta(ui, rect);
        if scroll_delta.abs() <= 0.0 {
            return;
        }

        let old_scale = if self.preview_auto_fit {
            fit_scale
        } else {
            self.preview_zoom
        }
        .clamp(PREVIEW_ZOOM_MIN, PREVIEW_ZOOM_MAX);
        let old_canvas_rect = Rect::from_center_size(
            rect.center() + self.preview_pan,
            Vec2::new(
                layers.canvas_width as f32 * old_scale,
                layers.canvas_height as f32 * old_scale,
            ),
        );
        let pointer = ui.ctx().pointer_hover_pos().unwrap_or(rect.center());
        let canvas_point = (pointer - old_canvas_rect.min) / old_scale.max(0.0001);
        let zoom_factor = (1.0
            + scroll_delta * PREVIEW_SCROLL_ZOOM_SENSITIVITY * PREVIEW_WHEEL_ZOOM_MULTIPLIER)
            .clamp(0.5, 2.0);

        self.preview_auto_fit = false;
        self.preview_zoom = (old_scale * zoom_factor).clamp(PREVIEW_ZOOM_MIN, PREVIEW_ZOOM_MAX);
        let new_size = Vec2::new(
            layers.canvas_width as f32 * self.preview_zoom,
            layers.canvas_height as f32 * self.preview_zoom,
        );
        let new_min = pointer - canvas_point * self.preview_zoom;
        let new_center = new_min + new_size * 0.5;
        self.preview_pan = new_center - rect.center();
        ui.ctx().request_repaint();
    }

    pub(super) fn preview_object_geometries(
        &self,
        layers: &PreviewLayerStack,
        canvas_rect: Rect,
        canvas_scale: f32,
    ) -> Vec<PreviewObjectGeometry> {
        let project_w = self.editor.project.settings.width.max(1) as f32;
        let project_h = self.editor.project.settings.height.max(1) as f32;
        let preview_scale = layers.canvas_width.max(1) as f32 / project_w;
        let project_to_screen = (preview_scale * canvas_scale).max(0.0001);
        let project_center = Pos2::new(project_w * 0.5, project_h * 0.5);
        let mut geometries = Vec::new();

        for layer in layers.layers.iter() {
            let Some(clip_id) = layer.clip_id else {
                continue;
            };
            let Some(clip) = self
                .editor
                .project
                .clips
                .iter()
                .find(|clip| clip.id == clip_id)
            else {
                continue;
            };
            let half_size = Vec2::new(
                (layer.placement.scaled_w / preview_scale).max(1.0) * 0.5,
                (layer.placement.scaled_h / preview_scale).max(1.0) * 0.5,
            );
            let center =
                project_center + Vec2::new(clip.transform.position_x, clip.transform.position_y);
            let project_rect = Rect::from_center_size(center, half_size * 2.0);
            let corners_project = [
                project_rect.left_top(),
                project_rect.right_top(),
                project_rect.right_bottom(),
                project_rect.left_bottom(),
            ];
            let screen_corners = corners_project.map(|point| {
                let rotated = rotate_point(point, center, clip.transform.rotation_deg);
                preview_project_to_screen(rotated, canvas_rect, preview_scale, canvas_scale)
            });
            geometries.push(PreviewObjectGeometry {
                clip_id,
                project_rect,
                screen_corners,
                screen_center: preview_project_to_screen(
                    center,
                    canvas_rect,
                    preview_scale,
                    canvas_scale,
                ),
                project_to_screen,
            });
        }

        geometries
    }

    pub(super) fn paint_preview_keyframe_reference_ghosts(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        canvas_rect: Rect,
        layers: &PreviewLayerStack,
        canvas_scale: f32,
    ) -> Vec<PreviewObjectGeometry> {
        let active_clip_ids: HashSet<Uuid> = layers
            .layers
            .iter()
            .filter_map(|layer| layer.clip_id)
            .collect();
        let fps = self.editor.project.settings.fps.max(1.0);
        let current_frame = timeline_floor_frame(self.editor.current_time, fps);
        let candidates: Vec<(Clip, Asset)> = self
            .editor
            .project
            .clips
            .iter()
            .filter(|clip| self.editor.selection.clip_ids.contains(&clip.id))
            .filter(|clip| !active_clip_ids.contains(&clip.id))
            .filter(|clip| timeline_floor_frame(clip.start_time, fps) != current_frame)
            .filter(|clip| {
                self.editor
                    .project
                    .find_track(clip.track_id)
                    .is_some_and(|track| !track.muted)
            })
            .filter_map(|clip| {
                let asset = self.editor.project.find_asset(clip.asset_id)?;
                if !clip_is_keyframe_image(clip, Some(asset)) {
                    return None;
                }
                Some((clip.clone(), asset.clone()))
            })
            .collect();
        let mut geometries = Vec::new();

        for (clip, asset) in candidates {
            let Some((texture_id, fallback_size)) = self.asset_thumbnail(ui.ctx(), &asset) else {
                continue;
            };
            let source_size = self
                .asset_source_dimensions(&asset)
                .unwrap_or(fallback_size)
                .max(Vec2::splat(1.0));
            let geometry = preview_geometry_for_clip(
                &self.editor.project,
                &clip,
                source_size,
                canvas_rect,
                layers,
                canvas_scale,
            );
            let screen_rect = Rect::from_center_size(
                geometry.screen_center,
                Vec2::new(
                    source_size.x
                        * preview_project_scale(layers, self.editor.project.settings.width)
                        * canvas_scale
                        * clip.transform.scale_x.max(0.01),
                    source_size.y
                        * preview_project_scale(layers, self.editor.project.settings.width)
                        * canvas_scale
                        * clip.transform.scale_y.max(0.01),
                ),
            );
            let tint = Color32::from_white_alpha(82);
            if clip.transform.rotation_deg.abs() <= 0.01 {
                painter.image(
                    texture_id,
                    screen_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    tint,
                );
            } else {
                paint_rotated_texture(
                    painter,
                    texture_id,
                    screen_rect,
                    clip.transform.rotation_deg,
                    tint,
                );
            }
            for index in 0..4 {
                painter.line_segment(
                    [
                        geometry.screen_corners[index],
                        geometry.screen_corners[(index + 1) % 4],
                    ],
                    Stroke::new(1.0_f32, kit::BORDER_FOCUS.gamma_multiply(0.42)),
                );
            }
            geometries.push(geometry);
        }

        geometries
    }

    pub(super) fn paint_preview_transform_overlay(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        canvas_rect: Rect,
        layers: &PreviewLayerStack,
        objects: &[PreviewObjectGeometry],
    ) {
        let painter = ui.painter().with_clip_rect(rect);
        for guide in self.preview_snap_guides.iter() {
            painter.line_segment(
                [guide.start, guide.end],
                Stroke::new(1.0_f32, Color32::from_rgb(229, 187, 47)),
            );
        }

        let Some(selected_clip_id) = self.editor.selection.primary_clip() else {
            if !matches!(self.preview_drag, Some(PreviewTransformDrag::Pan { .. })) {
                self.preview_drag = None;
                self.preview_snap_guides.clear();
            }
            return;
        };
        let Some(selected) = objects
            .iter()
            .find(|object| object.clip_id == selected_clip_id)
            .cloned()
        else {
            if !matches!(self.preview_drag, Some(PreviewTransformDrag::Pan { .. })) {
                self.preview_drag = None;
                self.preview_snap_guides.clear();
            }
            return;
        };

        self.apply_preview_transform_drag(ui, canvas_rect, layers, objects, &selected);

        let stroke = Stroke::new(1.0_f32, kit::BORDER_FOCUS);
        for index in 0..4 {
            painter.line_segment(
                [
                    selected.screen_corners[index],
                    selected.screen_corners[(index + 1) % 4],
                ],
                stroke,
            );
        }

        let body_rect = rect_from_points(&selected.screen_corners).expand(4.0);
        let body_response = ui.interact(
            body_rect,
            ui.id().with(("preview-transform-body", selected.clip_id)),
            Sense::click_and_drag(),
        );
        if body_response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if body_response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(pointer) = body_response.interact_pointer_pos() {
                if let Some(transform) = self.clip_transform(selected.clip_id) {
                    let project_point = preview_screen_to_project(
                        pointer,
                        canvas_rect,
                        layers,
                        self.editor.project.settings.width,
                    );
                    self.preview_auto_fit = false;
                    self.preview_drag = Some(PreviewTransformDrag::Move {
                        clip_id: selected.clip_id,
                        start_transform: transform,
                        start_pointer_project: project_point,
                        start_half_size: selected.project_rect.size() * 0.5,
                    });
                }
            }
        }

        for (handle, point) in preview_scale_handle_points(&selected) {
            let handle_rect = Rect::from_center_size(point, Vec2::splat(PREVIEW_HANDLE_SIZE));
            let response = ui.interact(
                handle_rect,
                ui.id()
                    .with(("preview-scale-handle", selected.clip_id, handle as u8)),
                Sense::click_and_drag(),
            );
            painter.rect_filled(handle_rect, 2.0, kit::FIELD_BG);
            painter.rect_stroke(
                handle_rect,
                2.0,
                Stroke::new(
                    1.0_f32,
                    if response.hovered() {
                        kit::TEXT
                    } else {
                        kit::BORDER_FOCUS
                    },
                ),
                egui::StrokeKind::Inside,
            );
            if response.hovered() {
                ui.ctx().set_cursor_icon(preview_scale_cursor(handle));
            }
            if response.drag_started_by(egui::PointerButton::Primary) {
                if let Some(transform) = self.clip_transform(selected.clip_id) {
                    self.preview_auto_fit = false;
                    self.preview_drag = Some(PreviewTransformDrag::Scale {
                        clip_id: selected.clip_id,
                        handle,
                        start_transform: transform,
                        start_center_project: selected.project_rect.center(),
                        start_half_size: selected.project_rect.size() * 0.5,
                    });
                }
            }
        }

        let rotate_point = preview_rotate_handle_point(&selected);
        painter.line_segment(
            [selected.screen_center, rotate_point],
            Stroke::new(1.0_f32, kit::BORDER_FOCUS.gamma_multiply(0.65)),
        );
        let rotate_rect =
            Rect::from_center_size(rotate_point, Vec2::splat(PREVIEW_HANDLE_SIZE + 2.0));
        let rotate_response = ui.interact(
            rotate_rect,
            ui.id().with(("preview-rotate-handle", selected.clip_id)),
            Sense::click_and_drag(),
        );
        painter.circle_filled(
            rotate_point,
            (PREVIEW_HANDLE_SIZE + 1.0) * 0.5,
            kit::FIELD_BG,
        );
        painter.circle_stroke(
            rotate_point,
            (PREVIEW_HANDLE_SIZE + 1.0) * 0.5,
            Stroke::new(
                1.0_f32,
                if rotate_response.hovered() {
                    kit::TEXT
                } else {
                    kit::BORDER_FOCUS
                },
            ),
        );
        if rotate_response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if rotate_response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(pointer) = rotate_response.interact_pointer_pos() {
                if let Some(transform) = self.clip_transform(selected.clip_id) {
                    let project_point = preview_screen_to_project(
                        pointer,
                        canvas_rect,
                        layers,
                        self.editor.project.settings.width,
                    );
                    let center = selected.project_rect.center();
                    self.preview_auto_fit = false;
                    self.preview_drag = Some(PreviewTransformDrag::Rotate {
                        clip_id: selected.clip_id,
                        start_transform: transform,
                        start_center_project: center,
                        start_pointer_angle: vector_angle_deg(project_point - center),
                    });
                }
            }
        }
    }

    pub(super) fn apply_preview_transform_drag(
        &mut self,
        ui: &mut Ui,
        canvas_rect: Rect,
        layers: &PreviewLayerStack,
        objects: &[PreviewObjectGeometry],
        selected: &PreviewObjectGeometry,
    ) {
        let primary_down = ui.input(|input| input.pointer.primary_down());
        if !primary_down {
            if !matches!(self.preview_drag, Some(PreviewTransformDrag::Pan { .. })) {
                self.preview_drag = None;
                self.preview_snap_guides.clear();
            }
            return;
        }

        let Some(pointer) = ui.ctx().pointer_interact_pos() else {
            return;
        };
        let pointer_project = preview_screen_to_project(
            pointer,
            canvas_rect,
            layers,
            self.editor.project.settings.width,
        );
        let alt_down = ui.input(|input| input.modifiers.alt);
        let shift_down = ui.input(|input| input.modifiers.shift);
        let Some(drag) = self.preview_drag else {
            return;
        };

        let mut next_transform = match drag {
            PreviewTransformDrag::Move {
                clip_id,
                start_transform,
                start_pointer_project,
                start_half_size,
            } if clip_id == selected.clip_id => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                let delta = pointer_project - start_pointer_project;
                let mut transform = start_transform;
                transform.position_x = start_transform.position_x + delta.x;
                transform.position_y = start_transform.position_y + delta.y;
                if !alt_down {
                    let snapped = self.snap_preview_transform_position(
                        clip_id,
                        transform,
                        start_half_size,
                        objects,
                        canvas_rect,
                        layers,
                        selected.project_to_screen,
                        selected.project_to_screen
                            / preview_project_scale(layers, self.editor.project.settings.width)
                                .max(0.0001),
                    );
                    transform = snapped.0;
                    self.preview_snap_guides = snapped.1;
                } else {
                    self.preview_snap_guides.clear();
                }
                transform
            }
            PreviewTransformDrag::Scale {
                clip_id,
                handle,
                start_transform,
                start_center_project,
                start_half_size,
            } if clip_id == selected.clip_id => {
                let constrain_aspect = !shift_down;
                let (scale_pointer, snap_axis, guides) = if alt_down {
                    (pointer_project, None, Vec::new())
                } else {
                    self.snap_preview_scale_pointer(
                        clip_id,
                        pointer_project,
                        handle,
                        objects,
                        canvas_rect,
                        layers,
                        selected.project_to_screen,
                        selected.project_to_screen
                            / preview_project_scale(layers, self.editor.project.settings.width)
                                .max(0.0001),
                    )
                };
                let transform = preview_scaled_transform(
                    start_transform,
                    start_center_project,
                    scale_pointer,
                    handle,
                    start_half_size,
                    constrain_aspect,
                    snap_axis,
                    selected.project_to_screen,
                );
                self.preview_snap_guides = guides;
                transform
            }
            PreviewTransformDrag::Rotate {
                clip_id,
                start_transform,
                start_center_project,
                start_pointer_angle,
            } if clip_id == selected.clip_id => {
                let current_angle = vector_angle_deg(pointer_project - start_center_project);
                let mut rotation =
                    start_transform.rotation_deg + current_angle - start_pointer_angle;
                if !shift_down {
                    rotation = (rotation / 15.0).round() * 15.0;
                }
                let mut transform = start_transform;
                transform.rotation_deg = rotation;
                self.preview_snap_guides.clear();
                transform
            }
            _ => return,
        };

        next_transform.scale_x = next_transform.scale_x.clamp(0.01, 100.0);
        next_transform.scale_y = next_transform.scale_y.clamp(0.01, 100.0);
        next_transform.opacity = next_transform.opacity.clamp(0.0, 1.0);
        if self
            .editor
            .project
            .set_clip_transform(selected.clip_id, next_transform)
        {
            self.editor.preview_dirty = true;
            ui.ctx().request_repaint();
        }
    }

    pub(super) fn snap_preview_transform_position(
        &self,
        clip_id: Uuid,
        transform: ClipTransform,
        half_size: Vec2,
        objects: &[PreviewObjectGeometry],
        canvas_rect: Rect,
        layers: &PreviewLayerStack,
        project_to_screen: f32,
        canvas_scale: f32,
    ) -> (ClipTransform, Vec<PreviewSnapGuide>) {
        let project_w = self.editor.project.settings.width.max(1) as f32;
        let project_h = self.editor.project.settings.height.max(1) as f32;
        let project_center = Pos2::new(project_w * 0.5, project_h * 0.5);
        let center = project_center + Vec2::new(transform.position_x, transform.position_y);
        let rect = Rect::from_center_size(center, half_size * 2.0);
        let threshold = (PREVIEW_SNAP_THRESHOLD_PX / project_to_screen.max(0.0001)).max(0.5);

        let mut x_targets = vec![0.0, project_w * 0.5, project_w];
        let mut y_targets = vec![0.0, project_h * 0.5, project_h];
        for object in objects.iter().filter(|object| object.clip_id != clip_id) {
            x_targets.extend([
                object.project_rect.left(),
                object.project_rect.center().x,
                object.project_rect.right(),
            ]);
            y_targets.extend([
                object.project_rect.top(),
                object.project_rect.center().y,
                object.project_rect.bottom(),
            ]);
        }

        let mut transform = transform;
        let mut guides = Vec::new();
        let preview_scale = preview_project_scale(layers, self.editor.project.settings.width);
        if let Some((delta, target)) = nearest_snap_delta(
            [rect.left(), rect.center().x, rect.right()],
            &x_targets,
            threshold,
        ) {
            transform.position_x += delta;
            let p0 = preview_project_to_screen(
                Pos2::new(target, 0.0),
                canvas_rect,
                preview_scale,
                canvas_scale,
            );
            let p1 = preview_project_to_screen(
                Pos2::new(target, project_h),
                canvas_rect,
                preview_scale,
                canvas_scale,
            );
            guides.push(PreviewSnapGuide { start: p0, end: p1 });
        }
        if let Some((delta, target)) = nearest_snap_delta(
            [rect.top(), rect.center().y, rect.bottom()],
            &y_targets,
            threshold,
        ) {
            transform.position_y += delta;
            let p0 = preview_project_to_screen(
                Pos2::new(0.0, target),
                canvas_rect,
                preview_scale,
                canvas_scale,
            );
            let p1 = preview_project_to_screen(
                Pos2::new(project_w, target),
                canvas_rect,
                preview_scale,
                canvas_scale,
            );
            guides.push(PreviewSnapGuide { start: p0, end: p1 });
        }

        (transform, guides)
    }

    pub(super) fn snap_preview_scale_pointer(
        &self,
        clip_id: Uuid,
        pointer_project: Pos2,
        handle: PreviewScaleHandle,
        objects: &[PreviewObjectGeometry],
        canvas_rect: Rect,
        layers: &PreviewLayerStack,
        project_to_screen: f32,
        canvas_scale: f32,
    ) -> (Pos2, Option<PreviewScaleSnapAxis>, Vec<PreviewSnapGuide>) {
        let (sx, sy) = preview_scale_handle_signs(handle);
        let project_w = self.editor.project.settings.width.max(1) as f32;
        let project_h = self.editor.project.settings.height.max(1) as f32;
        let threshold = (PREVIEW_SNAP_THRESHOLD_PX / project_to_screen.max(0.0001)).max(0.5);
        let preview_scale = preview_project_scale(layers, self.editor.project.settings.width);

        let mut x_targets = vec![0.0, project_w * 0.5, project_w];
        let mut y_targets = vec![0.0, project_h * 0.5, project_h];
        for object in objects.iter().filter(|object| object.clip_id != clip_id) {
            x_targets.extend([
                object.project_rect.left(),
                object.project_rect.center().x,
                object.project_rect.right(),
            ]);
            y_targets.extend([
                object.project_rect.top(),
                object.project_rect.center().y,
                object.project_rect.bottom(),
            ]);
        }

        let x_snap = if sx != 0.0 {
            nearest_snap_delta([pointer_project.x], &x_targets, threshold).map(|(delta, target)| {
                let p0 = preview_project_to_screen(
                    Pos2::new(target, 0.0),
                    canvas_rect,
                    preview_scale,
                    canvas_scale,
                );
                let p1 = preview_project_to_screen(
                    Pos2::new(target, project_h),
                    canvas_rect,
                    preview_scale,
                    canvas_scale,
                );
                (
                    delta,
                    PreviewSnapGuide { start: p0, end: p1 },
                    PreviewScaleSnapAxis::X,
                )
            })
        } else {
            None
        };
        let y_snap = if sy != 0.0 {
            nearest_snap_delta([pointer_project.y], &y_targets, threshold).map(|(delta, target)| {
                let p0 = preview_project_to_screen(
                    Pos2::new(0.0, target),
                    canvas_rect,
                    preview_scale,
                    canvas_scale,
                );
                let p1 = preview_project_to_screen(
                    Pos2::new(project_w, target),
                    canvas_rect,
                    preview_scale,
                    canvas_scale,
                );
                (
                    delta,
                    PreviewSnapGuide { start: p0, end: p1 },
                    PreviewScaleSnapAxis::Y,
                )
            })
        } else {
            None
        };

        let selected_snap = match (x_snap, y_snap) {
            (Some(x), Some(y)) if x.0.abs() <= y.0.abs() => Some(x),
            (Some(_), Some(y)) => Some(y),
            (Some(x), None) => Some(x),
            (None, Some(y)) => Some(y),
            (None, None) => None,
        };

        let mut pointer = pointer_project;
        let mut guides = Vec::new();
        if let Some((delta, guide, axis)) = selected_snap {
            match axis {
                PreviewScaleSnapAxis::X => pointer.x += delta,
                PreviewScaleSnapAxis::Y => pointer.y += delta,
            }
            guides.push(guide);
            (pointer, Some(axis), guides)
        } else {
            (pointer, None, guides)
        }
    }

    pub(super) fn clip_transform(&self, clip_id: Uuid) -> Option<ClipTransform> {
        self.editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .map(|clip| clip.transform)
    }
}

#[cfg(test)]
mod operation_mapping_tests {
    use super::*;
    use crate::core::preview::PreviewRenderIssue;
    use crate::ui_kit::{OperationPhase as Phase, OperationSeverity as Severity};

    fn issue(kind: PreviewRenderIssueKind) -> PreviewRenderIssue {
        PreviewRenderIssue {
            kind,
            clip_id: Uuid::new_v4(),
            asset_id: Uuid::new_v4(),
            asset_name: "Stargazers V07".to_string(),
            source_path: Some(PathBuf::from("missing.mp4")),
            detail: Some("decoder detail".to_string()),
        }
    }

    #[test]
    fn preview_pending_distinguishes_initial_render_from_retained_update() {
        let report = PreviewRenderReport::default();
        let initial = preview_operation(&report, true, false, false).unwrap();
        assert_eq!(initial.presentation.phase, Phase::Running);
        assert_eq!(initial.presentation.severity, Severity::Informational);

        let updating = preview_operation(&report, true, true, false).unwrap();
        assert_eq!(updating.presentation.phase, Phase::Running);
        assert!(updating.presentation.title.contains("Updating"));
    }

    #[test]
    fn active_scrub_has_one_stable_running_state_instead_of_empty_idle() {
        let report = PreviewRenderReport::default();
        let scrubbing = preview_operation(&report, false, false, true).unwrap();
        assert_eq!(scrubbing.presentation.phase, Phase::Running);
        assert_eq!(scrubbing.presentation.severity, Severity::Informational);
        assert!(scrubbing.presentation.title.contains("Scrubbing"));

        let settled = preview_operation(&report, false, false, false).unwrap();
        assert_eq!(settled.presentation.phase, Phase::Idle);
    }

    #[test]
    fn preview_missing_decode_and_ready_states_map_truthfully() {
        let mut missing = PreviewRenderReport::default();
        missing.active_visual_clips = 1;
        missing
            .issues
            .push(issue(PreviewRenderIssueKind::MissingMedia));
        let missing = preview_operation(&missing, false, false, false).unwrap();
        assert_eq!(missing.presentation.phase, Phase::Blocked);
        assert_eq!(missing.presentation.severity, Severity::Warning);
        assert!(matches!(
            missing.action,
            Some(PreviewSafeAction::ShowAsset(_))
        ));

        let mut failed = PreviewRenderReport::default();
        failed.active_visual_clips = 1;
        failed
            .issues
            .push(issue(PreviewRenderIssueKind::DecodeFailed));
        let failed = preview_operation(&failed, false, false, false).unwrap();
        assert_eq!(failed.presentation.phase, Phase::Failed);
        assert_eq!(failed.presentation.severity, Severity::Error);
        assert_eq!(
            failed.presentation.technical_detail.as_deref(),
            Some("decoder detail")
        );

        let ready = PreviewRenderReport {
            active_visual_clips: 1,
            rendered_visual_clips: 1,
            issues: Vec::new(),
        };
        assert!(preview_operation(&ready, false, true, false).is_none());
    }

    #[test]
    fn preview_partial_result_is_success_with_warning() {
        let report = PreviewRenderReport {
            active_visual_clips: 2,
            rendered_visual_clips: 1,
            issues: vec![issue(PreviewRenderIssueKind::Unsupported)],
        };
        let presentation = preview_operation(&report, false, true, false)
            .unwrap()
            .presentation;
        assert_eq!(presentation.phase, Phase::Succeeded);
        assert_eq!(presentation.severity, Severity::Warning);
    }
}

fn format_preview_frame_rate(fps: f64) -> String {
    if (fps - fps.round()).abs() < 0.0001 {
        format!("{fps:.0}")
    } else {
        format!("{fps:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}
