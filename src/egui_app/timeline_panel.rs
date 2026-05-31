use super::*;

impl NlaEguiApp {
    pub(super) fn timeline_panel(&mut self, root: &mut Ui) {
        if self.editor.layout.timeline_collapsed {
            let response = egui::Panel::bottom(self.project_panel_id("timeline_collapsed"))
                .exact_size(TIMELINE_HEADER_H + 12.0)
                .frame(kit::timeline_frame())
                .show_inside(root, |ui| {
                    self.timeline_header(ui, true);
                });
            kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Top);
            return;
        }

        let response = egui::Panel::bottom(self.project_panel_id("timeline"))
            .resizable(true)
            .default_size(self.editor.layout.timeline_height)
            .size_range(150.0..=420.0)
            .frame(kit::timeline_frame())
            .show_inside(root, |ui| {
                ui.set_min_height(150.0);
                self.timeline_header(ui, false);
                self.paint_timeline(ui);
            });
        self.editor.layout.timeline_height = response.response.rect.height().clamp(150.0, 420.0);
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Top);
    }

    pub(super) fn timeline_header(&mut self, ui: &mut Ui, collapsed: bool) {
        let duration = self.editor.project.duration().max(10.0);
        let fps = self.editor.project.settings.fps.max(1.0) as f32;
        let viewport_w = (ui.available_width() - TIMELINE_LABEL_W).max(1.0);
        let (fit_zoom, max_zoom) = timeline_zoom_bounds(duration as f32, viewport_w, fps);
        self.editor.layout.timeline_zoom =
            self.editor.layout.timeline_zoom.clamp(fit_zoom, max_zoom);
        let zoom = self.editor.layout.timeline_zoom;
        let zoom_label = if (zoom - fit_zoom).abs() <= 0.5 {
            "Fit".to_string()
        } else if (zoom - max_zoom).abs() <= 0.5 {
            "Frames".to_string()
        } else {
            format!("{zoom:.0}px/s")
        };
        let timecode_label = timecode(self.editor.current_time);

        let header_w = ui.available_width();
        let (header_rect, _) =
            ui.allocate_exact_size(Vec2::new(header_w, TIMELINE_HEADER_H), Sense::hover());
        let inner_rect = header_rect.shrink2(Vec2::new(TIMELINE_HEADER_PAD_X, 0.0));
        let right_w = timeline_header_right_width(ui, &timecode_label)
            .min(TIMELINE_HEADER_RIGHT_W)
            .min(inner_rect.width() * 0.35);
        let left_w = timeline_header_left_width(ui, collapsed, &zoom_label)
            .min(TIMELINE_HEADER_LEFT_W)
            .min((inner_rect.width() - right_w - TIMELINE_HEADER_CENTER_GAP * 2.0).max(0.0));
        let left_rect = Rect::from_min_max(
            inner_rect.left_top(),
            Pos2::new(inner_rect.left() + left_w, inner_rect.bottom()),
        );
        let right_rect = Rect::from_min_max(
            Pos2::new(inner_rect.right() - right_w, inner_rect.top()),
            inner_rect.right_bottom(),
        );
        let center_left = (left_rect.right() + TIMELINE_HEADER_CENTER_GAP).min(right_rect.left());
        let center_right = (right_rect.left() - TIMELINE_HEADER_CENTER_GAP).max(center_left);
        let center_region = Rect::from_min_max(
            Pos2::new(center_left, inner_rect.top()),
            Pos2::new(center_right, inner_rect.bottom()),
        );
        let transport_gap = 4.0;
        let transport_w = TIMELINE_TRANSPORT_BUTTON_COUNT * kit::TIMELINE_TRANSPORT_BUTTON_W
            + (TIMELINE_TRANSPORT_BUTTON_COUNT - 1.0) * transport_gap;
        let transport_rect =
            centered_child_rect(center_region, transport_w, kit::TIMELINE_TRANSPORT_BUTTON_H);

        let mut left_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(left_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        left_ui.shrink_clip_rect(left_rect);
        left_ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(kit::section_label("Timeline"));
            if !collapsed {
                ui.add_space(8.0);
                if kit::timeline_tool_icon_button(ui, "−").clicked() {
                    self.set_timeline_zoom_to_next_coarse(-1, duration, viewport_w);
                }
                ui.label(kit::caption(&zoom_label));
                if kit::timeline_tool_icon_button(ui, "+").clicked() {
                    self.set_timeline_zoom_to_next_coarse(1, duration, viewport_w);
                }
                let fit_active = (zoom - fit_zoom).abs() <= 0.5;
                let frames_active = (zoom - max_zoom).abs() <= 0.5;
                if kit::timeline_tool_text_button(ui, "Fit", 42.0, fit_active).clicked() {
                    self.set_timeline_zoom_anchored(fit_zoom, duration, viewport_w);
                }
                if kit::timeline_tool_text_button(ui, "Frames", 58.0, frames_active).clicked() {
                    self.set_timeline_zoom_anchored(max_zoom, duration, viewport_w);
                }
            }
        });

        let mut transport_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(transport_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        transport_ui.shrink_clip_rect(transport_rect);
        transport_ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = transport_gap;
            if kit::timeline_transport_icon_button(ui, kit::TimelineTransportIcon::First, false)
                .clicked()
            {
                self.seek_editor(0.0, false);
            }
            if kit::timeline_transport_icon_button(ui, kit::TimelineTransportIcon::Previous, false)
                .clicked()
            {
                self.seek_editor(
                    previous_frame_time(self.editor.current_time, self.editor.project.settings.fps),
                    false,
                );
            }
            let play_icon = if self.editor.is_playing {
                kit::TimelineTransportIcon::Pause
            } else {
                kit::TimelineTransportIcon::Play
            };
            if kit::timeline_transport_icon_button(ui, play_icon, true).clicked() {
                self.toggle_playback();
            }
            if kit::timeline_transport_icon_button(ui, kit::TimelineTransportIcon::Next, false)
                .clicked()
            {
                self.seek_editor(
                    next_frame_time(
                        self.editor.current_time,
                        self.editor.project.duration(),
                        self.editor.project.settings.fps,
                    ),
                    false,
                );
            }
            if kit::timeline_transport_icon_button(ui, kit::TimelineTransportIcon::Last, false)
                .clicked()
            {
                self.seek_editor(self.editor.project.duration(), false);
            }
        });

        let mut right_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(right_rect)
                .layout(Layout::right_to_left(Align::Center)),
        );
        right_ui.shrink_clip_rect(right_rect);
        right_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let collapse_icon = if collapsed {
                kit::TimelineTransportIcon::CaretUp
            } else {
                kit::TimelineTransportIcon::CaretDown
            };
            if kit::timeline_transport_icon_button(ui, collapse_icon, false).clicked() {
                self.editor.layout.timeline_collapsed = !collapsed;
            }
            ui.label(
                RichText::new(timecode_label)
                    .monospace()
                    .color(kit::TEXT_MUTED)
                    .size(11.0),
            );
        });
        if !collapsed {
            ui.separator();
        }
    }

    pub(super) fn paint_timeline(&mut self, ui: &mut Ui) {
        self.paint_timeline_v2(ui);
    }

    pub(super) fn paint_timeline_v2(&mut self, ui: &mut Ui) {
        let available = ui.available_size();
        let duration = self.editor.project.duration().max(10.0);
        let fps = self.editor.project.settings.fps.max(1.0) as f32;
        let track_count = self.editor.project.tracks.len().max(1) as f32;
        let min_h = TIMELINE_RULER_H + TIMELINE_TRACK_H + TIMELINE_ADD_ROW_H;
        let total_h = available.y.max(min_h);
        let (outer, response) =
            ui.allocate_exact_size(Vec2::new(available.x, total_h), Sense::click_and_drag());
        let track_content_h = track_count * TIMELINE_TRACK_H;
        let max_scroll_y =
            (track_content_h - (total_h - TIMELINE_RULER_H - TIMELINE_ADD_ROW_H).max(1.0)).max(0.0);
        self.clamp_timeline_vertical_scroll(max_scroll_y);
        let rects = timeline_rects(outer, self.editor.layout.timeline_scroll_y);
        let viewport_w = rects.tracks.width().max(1.0);
        let (fit_zoom, max_zoom) = timeline_zoom_bounds(duration as f32, viewport_w, fps);
        self.editor.layout.timeline_zoom =
            self.editor.layout.timeline_zoom.clamp(fit_zoom, max_zoom);
        self.handle_timeline_keyboard(ui, duration, viewport_w);
        let zoom = self
            .editor
            .layout
            .timeline_zoom
            .clamp(fit_zoom, max_zoom)
            .max(TIMELINE_MIN_ZOOM_FLOOR);
        self.editor.layout.timeline_zoom = zoom;
        let content_w = (duration as f32 * zoom).max(viewport_w);
        self.clamp_timeline_scroll(content_w, viewport_w);
        let content_viewport =
            Rect::from_min_max(rects.outer.left_top(), rects.tracks.right_bottom());
        self.handle_timeline_wheel(
            ui,
            content_viewport,
            duration,
            content_w,
            viewport_w,
            max_scroll_y,
        );
        let cache_buckets = if self.editor.layout.preview_stats {
            let bucket_hint_seconds = ((6.0 / zoom.max(TIMELINE_MIN_ZOOM_FLOOR)) as f64)
                .max(1.0 / self.editor.project.settings.fps.max(1.0));
            self.editor
                .previewer
                .cached_buckets_for_project(&self.editor.project, bucket_hint_seconds)
        } else {
            HashMap::new()
        };

        let painter = ui.painter_at(outer);
        let overlay_clip = Rect::from_min_max(
            rects.ruler.left_top(),
            Pos2::new(rects.outer.right(), rects.add_row.top()),
        );
        let overlay_painter = painter.with_clip_rect(overlay_clip);
        let ruler_painter = painter.with_clip_rect(rects.ruler);
        let track_painter = painter.with_clip_rect(rects.tracks);
        let label_viewport = Rect::from_min_max(
            Pos2::new(outer.left(), rects.tracks.top()),
            Pos2::new(rects.tracks.left(), rects.tracks.bottom()),
        );
        let label_painter = painter.with_clip_rect(label_viewport);
        painter.rect_filled(outer, 0.0, Color32::from_rgb(12, 13, 15));
        painter.rect_filled(rects.label, 0.0, kit::PANEL);
        painter.rect_filled(rects.ruler, 0.0, kit::CHROME);
        painter.line_segment(
            [
                Pos2::new(rects.tracks.left(), outer.top()),
                Pos2::new(rects.tracks.left(), outer.bottom()),
            ],
            Stroke::new(1.0, kit::BORDER),
        );

        let tracks = self.editor.project.tracks.clone();
        let clips = self.editor.project.clips.clone();
        let markers = self.editor.project.markers.clone();
        let assets_by_id: HashMap<Uuid, Asset> = self
            .editor
            .project
            .assets
            .iter()
            .cloned()
            .map(|asset| (asset.id, asset))
            .collect();
        let timeline_input_frozen = ui.ctx().any_popup_open();
        let dragged_asset_id = (!timeline_input_frozen)
            .then(|| {
                egui::DragAndDrop::payload::<AssetTimelineDragPayload>(ui.ctx())
                    .map(|payload| payload.asset_id)
            })
            .flatten();
        let drop_target_track_id = dragged_asset_id.and_then(|asset_id| {
            ui.ctx().pointer_hover_pos().and_then(|pos| {
                timeline_track_row_at_pos(pos, rects, &tracks)
                    .filter(|track| {
                        self.editor
                            .project
                            .asset_compatible_with_track(asset_id, track.id)
                    })
                    .map(|track| track.id)
            })
        });
        let clip_drag_target_track_id = if timeline_input_frozen {
            None
        } else {
            match self.timeline_drag {
                Some(TimelineDrag::ClipMove { clip_id, .. }) => {
                    ui.ctx().pointer_hover_pos().and_then(|pos| {
                        let clip = clips.iter().find(|clip| clip.id == clip_id)?;
                        timeline_track_row_at_pos(pos, rects, &tracks)
                            .filter(|track| {
                                self.editor
                                    .project
                                    .asset_compatible_with_track(clip.asset_id, track.id)
                            })
                            .map(|track| track.id)
                    })
                }
                _ => None,
            }
        };

        self.paint_timeline_ruler(&ruler_painter, rects.ruler, duration, zoom, fps);

        let mut clip_geoms = Vec::new();
        let mut marker_geoms = Vec::new();
        for (row, track) in tracks.iter().enumerate() {
            let row_rect = timeline_row_rect(rects, row);
            if row_rect.bottom() < rects.tracks.top() || row_rect.top() > rects.tracks.bottom() {
                continue;
            }
            let label_rect = Rect::from_min_max(
                Pos2::new(outer.left(), row_rect.top()),
                Pos2::new(rects.tracks.left(), row_rect.bottom()),
            );
            let label_hit_rect = label_rect.intersect(label_viewport);
            let selected = self.editor.selection.track_ids.contains(&track.id);
            let track_muted = track.muted && track.track_type != TrackType::Marker;
            let track_color = if track_muted {
                track_color(track.track_type).gamma_multiply(0.45)
            } else {
                track_color(track.track_type)
            };
            let track_response = ui.interact(
                label_hit_rect,
                ui.id().with(("timeline-track-label", track.id)),
                Sense::click(),
            );
            if !timeline_input_frozen && track_response.clicked() {
                self.editor.selection.select_track(track.id);
            }
            track_response.context_menu(|ui| {
                self.editor.selection.select_track(track.id);
                let can_move_up = self
                    .editor
                    .project
                    .tracks
                    .iter()
                    .position(|candidate| candidate.id == track.id)
                    .map(|index| index > 0)
                    .unwrap_or(false);
                let can_move_down = self
                    .editor
                    .project
                    .tracks
                    .iter()
                    .position(|candidate| candidate.id == track.id)
                    .map(|index| index + 1 < self.editor.project.tracks.len())
                    .unwrap_or(false);
                if automation_button(
                    ui.add_enabled(can_move_up, egui::Button::new("Move Up")),
                    "Move Up",
                )
                .clicked()
                {
                    if self.editor.project.move_track_up(track.id) {
                        self.editor.preview_dirty = true;
                        self.editor.status = format!("Moved {} up", track.name);
                    }
                    ui.close();
                }
                if automation_button(
                    ui.add_enabled(can_move_down, egui::Button::new("Move Down")),
                    "Move Down",
                )
                .clicked()
                {
                    if self.editor.project.move_track_down(track.id) {
                        self.editor.preview_dirty = true;
                        self.editor.status = format!("Moved {} down", track.name);
                    }
                    ui.close();
                }
                if track.track_type != TrackType::Marker {
                    ui.separator();
                    let label = if track.muted {
                        "Unmute Track"
                    } else {
                        "Mute Track"
                    };
                    if automation_button(ui.button(label), label).clicked() {
                        if let Some(project_track) = self
                            .editor
                            .project
                            .tracks
                            .iter_mut()
                            .find(|candidate| candidate.id == track.id)
                        {
                            project_track.muted = !project_track.muted;
                            self.editor.preview_dirty = true;
                            self.editor.status = if project_track.muted {
                                format!("Muted {}", project_track.name)
                            } else {
                                format!("Unmuted {}", project_track.name)
                            };
                        }
                        self.refresh_audio_playback_items();
                        ui.close();
                    }
                }
                ui.separator();
                if automation_button(ui.button("Delete Track..."), "Delete Track").clicked() {
                    self.request_delete_tracks(&[track.id]);
                    ui.close();
                }
            });
            label_painter.rect_filled(
                label_rect,
                0.0,
                if selected {
                    Color32::from_rgb(25, 42, 35)
                } else {
                    kit::PANEL
                },
            );
            track_painter.rect_filled(
                row_rect,
                0.0,
                if row % 2 == 0 {
                    Color32::from_rgb(14, 15, 17)
                } else {
                    Color32::from_rgb(11, 12, 14)
                },
            );
            if drop_target_track_id == Some(track.id) || clip_drag_target_track_id == Some(track.id)
            {
                track_painter.rect_filled(row_rect, 0.0, kit::BORDER_FOCUS.gamma_multiply(0.10));
                track_painter.rect_stroke(
                    row_rect.shrink(1.0),
                    3.0,
                    Stroke::new(1.0, kit::BORDER_FOCUS.gamma_multiply(0.85)),
                    egui::StrokeKind::Inside,
                );
            }
            label_painter.line_segment(
                [
                    Pos2::new(outer.left(), row_rect.bottom()),
                    Pos2::new(rects.tracks.left(), row_rect.bottom()),
                ],
                Stroke::new(1.0, kit::BORDER_SOFT),
            );
            track_painter.line_segment(
                [
                    Pos2::new(rects.tracks.left(), row_rect.bottom()),
                    Pos2::new(rects.tracks.right(), row_rect.bottom()),
                ],
                Stroke::new(1.0, kit::BORDER_SOFT),
            );
            label_painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(label_rect.left() + 12.0, row_rect.center().y - 8.0),
                    Vec2::new(3.0, 16.0),
                ),
                1.0,
                track_color,
            );
            label_painter.text(
                Pos2::new(label_rect.left() + 26.0, row_rect.center().y),
                egui::Align2::LEFT_CENTER,
                &track.name,
                FontId::proportional(12.5),
                if track_muted {
                    kit::TEXT_DIM
                } else {
                    kit::TEXT
                },
            );
            if track_muted {
                label_painter.text(
                    label_rect.right_center() - Vec2::new(14.0, 0.0),
                    egui::Align2::CENTER_CENTER,
                    "M",
                    FontId::monospace(10.0),
                    kit::TEXT_DIM,
                );
            }

            for clip in clips.iter().filter(|clip| clip.track_id == track.id) {
                let asset = assets_by_id.get(&clip.asset_id);
                let keyframe = clip_is_keyframe_image(clip, asset);
                let clip_rect = timeline_clip_rect(
                    clip,
                    asset,
                    row_rect,
                    zoom,
                    self.editor.layout.timeline_scroll_x,
                );
                clip_geoms.push(TimelineClipGeom {
                    clip_id: clip.id,
                    rect: clip_rect,
                    keyframe,
                });
                let selected = self.editor.selection.clip_ids.contains(&clip.id);
                let thumbnail_tiles = asset
                    .filter(|asset| asset.is_visual())
                    .map(|asset| {
                        self.timeline_clip_thumbnail_tiles(ui.ctx(), asset, clip, clip_rect, zoom)
                    })
                    .unwrap_or_default();
                let waveform = asset
                    .filter(|asset| asset.is_audio())
                    .and_then(|asset| self.audio_peak_cache(ui.ctx(), asset));
                let contextual_keyframe_label = !timeline_input_frozen
                    && keyframe
                    && (selected
                        || ui
                            .ctx()
                            .pointer_hover_pos()
                            .is_some_and(|pos| clip_rect.contains(pos)));
                self.paint_timeline_clip(
                    &track_painter,
                    clip,
                    asset,
                    clip_rect,
                    track_color,
                    selected,
                    &thumbnail_tiles,
                    waveform.as_ref(),
                    cache_buckets.get(&clip.id).map(Vec::as_slice),
                    contextual_keyframe_label,
                );
            }
            if track_muted {
                track_painter.rect_filled(
                    row_rect,
                    0.0,
                    Color32::from_rgba_unmultiplied(3, 4, 6, 96),
                );
            }

            if track.track_type == TrackType::Marker {
                for marker in markers.iter().filter(|marker| {
                    self.editor
                        .project
                        .marker_belongs_to_track(marker, track.id)
                }) {
                    let x = time_to_timeline_x(
                        marker.time,
                        rects.tracks.left(),
                        zoom,
                        self.editor.layout.timeline_scroll_x,
                    );
                    let hit_rect = timeline_marker_hit_rect(marker, row_rect, x);
                    marker_geoms.push(TimelineMarkerGeom {
                        marker_id: marker.id,
                        hit_rect,
                    });
                    self.paint_timeline_marker(&track_painter, marker, row_rect, x);
                }
            }
        }

        self.paint_add_track_row(ui, &painter, rects);
        self.paint_timeline_grid_overlay(&track_painter, rects, duration, zoom);
        self.paint_timeline_playhead(&overlay_painter, rects, duration, zoom);
        if let Some(time) = self.timeline_snap_preview {
            let x = time_to_timeline_x(
                time,
                rects.tracks.left(),
                zoom,
                self.editor.layout.timeline_scroll_x,
            );
            overlay_painter.line_segment(
                [
                    Pos2::new(x, rects.ruler.top()),
                    Pos2::new(x, rects.add_row.top()),
                ],
                Stroke::new(1.0, Color32::from_rgb(229, 187, 47)),
            );
        }
        self.paint_timeline_scrollbar(ui, &painter, rects, content_w, viewport_w);
        self.paint_timeline_vertical_scrollbar(ui, &painter, rects, track_content_h);
        if !timeline_input_frozen {
            if let Some(payload) = response.dnd_release_payload::<AssetTimelineDragPayload>() {
                if let Some(pos) = ui
                    .ctx()
                    .pointer_interact_pos()
                    .or_else(|| ui.ctx().pointer_hover_pos())
                {
                    self.drop_asset_on_timeline(
                        payload.asset_id,
                        pos,
                        rects,
                        &tracks,
                        duration,
                        zoom,
                    );
                }
            }
        }
        if response.secondary_clicked() {
            self.timeline_context_menu_pos = response
                .interact_pointer_pos()
                .or_else(|| ui.ctx().pointer_hover_pos());
        }
        let context_menu_response = response.context_menu(|ui| {
            let context_pos = self.timeline_context_menu_pos;
            let marker_time = context_pos
                .map(|pos| {
                    if pos.x >= rects.tracks.left() {
                        let raw_time =
                            ((pos.x - rects.tracks.left() + self.editor.layout.timeline_scroll_x)
                                / zoom)
                                .clamp(0.0, duration as f32) as f64;
                        snap_time_to_frame(raw_time, self.editor.project.settings.fps.max(1.0))
                    } else {
                        self.editor.current_time
                    }
                })
                .unwrap_or(self.editor.current_time);
            let marker_track_id = context_pos.and_then(|pos| {
                timeline_track_row_at_pos(pos, rects, &tracks)
                    .filter(|track| track.track_type == TrackType::Marker)
                    .map(|track| track.id)
            });
            let context_track = context_pos
                .and_then(|pos| timeline_track_row_at_pos(pos, rects, &tracks))
                .cloned();

            if let Some(pos) = context_pos {
                match timeline_hit(pos, rects, &tracks, &clip_geoms, &marker_geoms) {
                    TimelineHit::ClipBody(id)
                    | TimelineHit::ClipLeftEdge(id)
                    | TimelineHit::ClipRightEdge(id) => {
                        if !self.editor.selection.clip_ids.contains(&id) {
                            self.editor.selection.select_clip(id);
                        }
                    }
                    _ => {}
                }
            }

            if automation_button(ui.button("Add Marker Here"), "Add Marker Here").clicked() {
                self.editor
                    .add_marker_to_track(Some(marker_time), marker_track_id);
                ui.close();
                return;
            }

            let t2i_track_id = context_track
                .as_ref()
                .filter(|track| track.track_type == TrackType::Video)
                .map(|track| track.id);

            let mut selected_clips: Vec<Clip> = self
                .editor
                .project
                .clips
                .iter()
                .filter(|clip| self.editor.selection.clip_ids.contains(&clip.id))
                .cloned()
                .collect();
            selected_clips.sort_by(|a, b| {
                a.start_time
                    .partial_cmp(&b.start_time)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.id.cmp(&b.id))
            });
            let visual_count = selected_clips
                .iter()
                .filter(|clip| {
                    self.editor
                        .project
                        .find_asset(clip.asset_id)
                        .is_some_and(|asset| asset.is_image() || asset.is_video())
                })
                .count();
            if selected_clips.len() >= 2 {
                ui.separator();
                if automation_button(ui.button("Space Selected Clips"), "Space Selected Clips")
                    .clicked()
                {
                    self.space_selected_clips(&selected_clips);
                    ui.close();
                }
            }
            let single_clip = selected_clips
                .first()
                .filter(|_| selected_clips.len() == 1)
                .cloned();
            let single_asset = single_clip
                .as_ref()
                .and_then(|clip| self.editor.project.find_asset(clip.asset_id))
                .cloned();
            let single_is_generative = single_asset
                .as_ref()
                .is_some_and(|asset| asset.is_generative());
            let single_is_image = single_asset.as_ref().is_some_and(|asset| asset.is_image());
            let single_is_video = single_asset.as_ref().is_some_and(|asset| asset.is_video());

            if let (Some(single_clip), Some(asset)) = (single_clip.as_ref(), single_asset.as_ref())
            {
                if single_is_generative
                    && automation_button(ui.button("Open Asset Lab"), "Open Asset Lab").clicked()
                {
                    let local_time = (self.editor.current_time - single_clip.start_time
                        + single_clip.trim_in_seconds)
                        .max(0.0);
                    self.open_asset_lab_at_time(asset.id, Some(local_time));
                    ui.close();
                }
            }

            if t2i_track_id.is_some() || single_is_image || single_is_video || visual_count >= 2 {
                ui.separator();
                ui.label(kit::caption("Generate"));
            }
            if let Some(track_id) = t2i_track_id {
                ui.menu_button("Create T2I Image", |ui| {
                    if let Some(provider_id) = self.provider_choice_menu(
                        ui,
                        ProviderWorkflowKind::TextToImage,
                        "Configure provider later",
                    ) {
                        self.create_generative_image_clip_on_track(
                            track_id,
                            marker_time,
                            provider_id,
                        );
                        ui.close();
                    }
                });
            }
            if let Some(single_clip) = single_clip.as_ref() {
                if single_is_image {
                    ui.menu_button("Create I2I from Image", |ui| {
                        if let Some(provider_id) = self.provider_choice_menu(
                            ui,
                            ProviderWorkflowKind::ImageToImage,
                            "Configure provider later",
                        ) {
                            self.create_i2i_from_single_clip(single_clip.id, provider_id);
                            ui.close();
                        }
                    });
                    ui.menu_button("Create I2V from Image", |ui| {
                        if let Some(provider_id) = self.provider_choice_menu(
                            ui,
                            ProviderWorkflowKind::ImageToVideo,
                            "Configure provider later",
                        ) {
                            self.create_i2v_from_single_clip(
                                single_clip.id,
                                SingleI2VReference::Image,
                                provider_id,
                            );
                            ui.close();
                        }
                    });
                }
                if single_is_video {
                    ui.menu_button("Create I2V from First Frame", |ui| {
                        if let Some(provider_id) = self.provider_choice_menu(
                            ui,
                            ProviderWorkflowKind::ImageToVideo,
                            "Configure provider later",
                        ) {
                            self.create_i2v_from_single_clip(
                                single_clip.id,
                                SingleI2VReference::VideoFirstFrame,
                                provider_id,
                            );
                            ui.close();
                        }
                    });
                    ui.menu_button("Extend I2V from Last Frame", |ui| {
                        if let Some(provider_id) = self.provider_choice_menu(
                            ui,
                            ProviderWorkflowKind::ImageToVideo,
                            "Configure provider later",
                        ) {
                            self.create_i2v_from_single_clip(
                                single_clip.id,
                                SingleI2VReference::VideoLastFrame,
                                provider_id,
                            );
                            ui.close();
                        }
                    });
                }
            }
            if visual_count >= 2 {
                ui.menu_button("Generate Between Keyframes", |ui| {
                    if let Some(provider_id) = self.provider_choice_menu(
                        ui,
                        ProviderWorkflowKind::FirstFrameLastFrameVideo,
                        "Configure provider later",
                    ) {
                        let mut sorted = selected_clips.clone();
                        sorted.sort_by(|a, b| {
                            a.start_time
                                .partial_cmp(&b.start_time)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                        self.request_bridge_video_from_selected_clips(&sorted, provider_id);
                        ui.close();
                    }
                });
            }
            if !selected_clips.is_empty() {
                ui.separator();
            }
            if automation_button(
                ui.add_enabled(
                    !selected_clips.is_empty(),
                    egui::Button::new("Delete Clip(s)"),
                ),
                "Delete Clip(s)",
            )
            .clicked()
            {
                self.editor.delete_selected_clips();
                ui.close();
            }
        });
        if context_menu_response.is_none() && !response.context_menu_opened() {
            self.timeline_context_menu_pos = None;
        }
        self.handle_timeline_pointer(
            ui,
            &response,
            rects,
            &tracks,
            &clips,
            &clip_geoms,
            &marker_geoms,
            duration,
            zoom,
        );
    }

    pub(super) fn drop_asset_on_timeline(
        &mut self,
        asset_id: Uuid,
        pos: Pos2,
        rects: TimelineRects,
        tracks: &[crate::state::Track],
        duration: f64,
        zoom: f32,
    ) {
        let Some(track) = timeline_track_row_at_pos(pos, rects, tracks) else {
            return;
        };
        if !self
            .editor
            .project
            .asset_compatible_with_track(asset_id, track.id)
        {
            self.editor.status = "Asset cannot be placed on that track".to_string();
            return;
        }

        let raw_time = ((pos.x - rects.tracks.left() + self.editor.layout.timeline_scroll_x) / zoom)
            .clamp(0.0, duration as f32) as f64;
        let time = snap_time_to_frame(raw_time, self.editor.project.settings.fps.max(1.0));
        match self
            .editor
            .add_asset_to_timeline_track(asset_id, track.id, Some(time))
        {
            Ok(_) => {
                self.editor.status = format!("Added clip to {}", track.name);
            }
            Err(err) => {
                self.editor.status = err;
            }
        }
    }

    pub(super) fn set_timeline_zoom_anchored(&mut self, zoom: f32, duration: f64, viewport_w: f32) {
        let fps = self.editor.project.settings.fps.max(1.0) as f32;
        let (fit_zoom, max_zoom) = timeline_zoom_bounds(duration as f32, viewport_w, fps);
        let next_zoom = zoom.clamp(fit_zoom, max_zoom);
        let old_zoom = self
            .editor
            .layout
            .timeline_zoom
            .max(TIMELINE_MIN_ZOOM_FLOOR);
        if (next_zoom - old_zoom).abs() < f32::EPSILON {
            return;
        }
        let current_time = self.editor.current_time as f32;
        let anchor_x = current_time * old_zoom - self.editor.layout.timeline_scroll_x;
        self.editor.layout.timeline_scroll_x = current_time * next_zoom - anchor_x;
        self.editor.layout.timeline_zoom = next_zoom;
        let content_w = (duration as f32 * next_zoom).max(viewport_w);
        self.clamp_timeline_scroll(content_w, viewport_w);
    }

    pub(super) fn set_timeline_zoom_at_view_x(
        &mut self,
        zoom: f32,
        duration: f64,
        viewport_w: f32,
        anchor_x: f32,
    ) {
        let fps = self.editor.project.settings.fps.max(1.0) as f32;
        let (fit_zoom, max_zoom) = timeline_zoom_bounds(duration as f32, viewport_w, fps);
        let next_zoom = zoom.clamp(fit_zoom, max_zoom);
        let old_zoom = self
            .editor
            .layout
            .timeline_zoom
            .max(TIMELINE_MIN_ZOOM_FLOOR);
        if (next_zoom - old_zoom).abs() < f32::EPSILON {
            return;
        }

        let anchor_x = anchor_x.clamp(0.0, viewport_w.max(0.0));
        let anchor_time = ((self.editor.layout.timeline_scroll_x + anchor_x) / old_zoom)
            .clamp(0.0, duration as f32);
        self.editor.layout.timeline_scroll_x = anchor_time * next_zoom - anchor_x;
        self.editor.layout.timeline_zoom = next_zoom;
        let content_w = (duration as f32 * next_zoom).max(viewport_w);
        self.clamp_timeline_scroll(content_w, viewport_w);
    }

    pub(super) fn set_timeline_zoom_to_next_coarse(
        &mut self,
        direction: i32,
        duration: f64,
        viewport_w: f32,
    ) {
        let fps = self.editor.project.settings.fps.max(1.0) as f32;
        let (fit_zoom, max_zoom) = timeline_zoom_bounds(duration as f32, viewport_w, fps);
        let next_zoom = next_timeline_coarse_zoom(
            self.editor.layout.timeline_zoom,
            direction,
            fit_zoom,
            max_zoom,
        );
        self.set_timeline_zoom_anchored(next_zoom, duration, viewport_w);
    }

    pub(super) fn clamp_timeline_scroll(&mut self, content_w: f32, viewport_w: f32) {
        let max_scroll = (content_w - viewport_w).max(0.0);
        if !self.editor.layout.timeline_scroll_x.is_finite() {
            self.editor.layout.timeline_scroll_x = 0.0;
        }
        self.editor.layout.timeline_scroll_x =
            self.editor.layout.timeline_scroll_x.clamp(0.0, max_scroll);
    }

    pub(super) fn clamp_timeline_vertical_scroll(&mut self, max_scroll_y: f32) {
        if !self.editor.layout.timeline_scroll_y.is_finite() {
            self.editor.layout.timeline_scroll_y = 0.0;
        }
        self.editor.layout.timeline_scroll_y = self
            .editor
            .layout
            .timeline_scroll_y
            .clamp(0.0, max_scroll_y.max(0.0));
    }

    pub(super) fn handle_timeline_keyboard(&mut self, ui: &mut Ui, duration: f64, viewport_w: f32) {
        if self.keyboard_shortcuts_suppressed(ui.ctx()) {
            return;
        }

        let zoom_in = ui.input(|input| {
            input.key_pressed(egui::Key::Plus) || input.key_pressed(egui::Key::Equals)
        });
        let zoom_out = ui.input(|input| input.key_pressed(egui::Key::Minus));
        if zoom_in {
            self.set_timeline_zoom_to_next_coarse(1, duration, viewport_w);
        }
        if zoom_out {
            self.set_timeline_zoom_to_next_coarse(-1, duration, viewport_w);
        }
    }

    pub(super) fn handle_timeline_wheel(
        &mut self,
        ui: &mut Ui,
        viewport_rect: Rect,
        duration: f64,
        content_w: f32,
        viewport_w: f32,
        max_scroll_y: f32,
    ) {
        if self.modal_background_input_blocked() || ui.ctx().any_popup_open() {
            return;
        }
        let Some(pointer) = ui.ctx().pointer_hover_pos() else {
            return;
        };
        if !viewport_rect.contains(pointer) {
            return;
        }
        let (ctrl_down, ctrl_zoom_delta, shift, smooth_delta, wheel_delta, plain_wheel_delta) = ui
            .input(|input| {
                let mut ctrl_zoom_delta = 0.0;
                let mut shift_wheel_delta = Vec2::ZERO;
                let mut plain_wheel_delta = Vec2::ZERO;
                for event in input.events.iter() {
                    if let egui::Event::MouseWheel {
                        delta, modifiers, ..
                    } = event
                    {
                        if modifiers.command || modifiers.ctrl || modifiers.mac_cmd {
                            ctrl_zoom_delta += if delta.y.abs() > 0.0 {
                                delta.y
                            } else {
                                delta.x
                            };
                        } else if modifiers.shift {
                            shift_wheel_delta += *delta;
                        } else {
                            plain_wheel_delta += *delta;
                        }
                    }
                }
                (
                    input.modifiers.command || input.modifiers.ctrl || input.modifiers.mac_cmd,
                    ctrl_zoom_delta,
                    input.modifiers.shift,
                    input.smooth_scroll_delta,
                    shift_wheel_delta,
                    plain_wheel_delta,
                )
            });

        let ctrl_zoom_delta = if ctrl_zoom_delta.abs() > 0.0 {
            ctrl_zoom_delta
        } else if ctrl_down && smooth_delta.y.abs() > 0.0 {
            smooth_delta.y
        } else {
            0.0
        };
        if ctrl_zoom_delta.abs() > 0.0 {
            let zoom_factor = (ctrl_zoom_delta * TIMELINE_WHEEL_ZOOM_SENSITIVITY)
                .exp()
                .clamp(0.5, 2.0);
            let anchor_x = pointer.x - viewport_rect.left();
            self.set_timeline_zoom_at_view_x(
                self.editor.layout.timeline_zoom * zoom_factor,
                duration,
                viewport_w,
                anchor_x,
            );
            ui.ctx().request_repaint();
            return;
        }

        if max_scroll_y > 0.0 && !shift && !ctrl_down {
            let vertical_delta = if plain_wheel_delta.y.abs() > 0.0 {
                plain_wheel_delta.y
            } else if smooth_delta.y.abs() > 0.0 && smooth_delta.x.abs() <= smooth_delta.y.abs() {
                smooth_delta.y
            } else {
                0.0
            };
            if vertical_delta.abs() > 0.0 {
                self.editor.layout.timeline_scroll_y -= vertical_delta;
                self.clamp_timeline_vertical_scroll(max_scroll_y);
                ui.ctx().request_repaint();
                return;
            }
        }

        let content_delta = if wheel_delta.x.abs() > 0.0 {
            wheel_delta.x
        } else if wheel_delta.y.abs() > 0.0 {
            wheel_delta.y
        } else if smooth_delta.x.abs() > 0.0 {
            smooth_delta.x
        } else if shift && smooth_delta.y.abs() > 0.0 {
            smooth_delta.y
        } else {
            0.0
        };
        if content_delta.abs() > 0.0 {
            self.editor.layout.timeline_scroll_x -= content_delta;
            self.clamp_timeline_scroll(content_w, viewport_w);
            ui.ctx().request_repaint();
        }
    }

    pub(super) fn paint_timeline_ruler(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        duration: f64,
        zoom: f32,
        fps: f32,
    ) {
        let scroll_x = self.editor.layout.timeline_scroll_x;
        let visible_start = (scroll_x / zoom).max(0.0) as f64;
        let visible_end = ((scroll_x + rect.width()) / zoom).min(duration as f32) as f64;
        let target_seconds = (90.0 / zoom.max(0.1)).max(0.5) as f64;
        let major_step = nice_timeline_step(target_seconds);
        let first_tick = (visible_start / major_step).floor() as i32 - 1;
        let last_tick = (visible_end / major_step).ceil() as i32 + 1;

        if zoom >= 240.0 {
            let fps = fps.max(1.0);
            let first_frame = (visible_start * fps as f64).floor() as i64 - 1;
            let last_frame = (visible_end * fps as f64).ceil() as i64 + 1;
            let fps_i = fps.round().max(1.0) as i64;
            for frame in first_frame..=last_frame {
                if frame < 0 || frame % fps_i == 0 {
                    continue;
                }
                let t = frame as f64 / fps as f64;
                let x = time_to_timeline_x(t, rect.left(), zoom, scroll_x);
                if rect.x_range().contains(x) {
                    painter.line_segment(
                        [
                            Pos2::new(x, rect.bottom() - 4.0),
                            Pos2::new(x, rect.bottom()),
                        ],
                        Stroke::new(1.0, kit::BORDER_SOFT),
                    );
                }
            }
        }

        for tick in first_tick..=last_tick {
            if tick < 0 {
                continue;
            }
            let t = tick as f64 * major_step;
            if t > duration {
                continue;
            }
            let x = time_to_timeline_x(t, rect.left(), zoom, scroll_x);
            if x < rect.left() - 80.0 || x > rect.right() + 8.0 {
                continue;
            }
            painter.line_segment(
                [
                    Pos2::new(x, rect.bottom() - 10.0),
                    Pos2::new(x, rect.bottom()),
                ],
                Stroke::new(1.0, Color32::from_rgb(52, 55, 62)),
            );
            painter.text(
                Pos2::new(x + 4.0, rect.top() + 4.0),
                egui::Align2::LEFT_TOP,
                timeline_ruler_label(t),
                FontId::monospace(9.0),
                kit::TEXT_DIM,
            );
        }
    }

    pub(super) fn paint_timeline_grid_overlay(
        &self,
        painter: &egui::Painter,
        rects: TimelineRects,
        duration: f64,
        zoom: f32,
    ) {
        let scroll_x = self.editor.layout.timeline_scroll_x;
        let visible_start = (scroll_x / zoom).max(0.0) as f64;
        let visible_end = ((scroll_x + rects.tracks.width()) / zoom).min(duration as f32) as f64;
        let target_seconds = (90.0 / zoom.max(0.1)).max(0.5) as f64;
        let major_step = nice_timeline_step(target_seconds);
        let first_tick = (visible_start / major_step).floor() as i32 - 1;
        let last_tick = (visible_end / major_step).ceil() as i32 + 1;
        let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(86, 92, 104, 42));

        for tick in first_tick..=last_tick {
            if tick < 0 {
                continue;
            }
            let t = tick as f64 * major_step;
            if t > duration {
                continue;
            }
            let x = time_to_timeline_x(t, rects.tracks.left(), zoom, scroll_x);
            if x < rects.tracks.left() - 1.0 || x > rects.tracks.right() + 1.0 {
                continue;
            }
            painter.line_segment(
                [
                    Pos2::new(x, rects.tracks.top()),
                    Pos2::new(x, rects.tracks.bottom()),
                ],
                stroke,
            );
        }
    }

    pub(super) fn paint_timeline_clip(
        &self,
        painter: &egui::Painter,
        clip: &Clip,
        asset: Option<&Asset>,
        rect: Rect,
        accent: Color32,
        selected: bool,
        thumbnail_tiles: &[TimelineThumbTile],
        waveform: Option<&PeakCache>,
        cache_buckets: Option<&[bool]>,
        contextual_keyframe_label: bool,
    ) {
        if clip_is_keyframe_image(clip, asset) {
            self.paint_timeline_keyframe_clip(
                painter,
                clip,
                asset,
                rect,
                accent,
                selected,
                thumbnail_tiles,
                contextual_keyframe_label,
            );
            return;
        }

        let fill = if selected {
            Color32::from_rgb(18, 50, 36)
        } else {
            Color32::from_rgb(23, 25, 29)
        };
        let type_stroke = if selected {
            accent
        } else {
            accent.gamma_multiply(0.58)
        };
        let selection_stroke = if selected {
            kit::BORDER_FOCUS
        } else {
            type_stroke
        };
        painter.rect_filled(rect, 4.0, fill);
        if !thumbnail_tiles.is_empty() {
            paint_clip_thumbnail_strip(painter, rect, thumbnail_tiles);
            if selected {
                painter.rect_filled(rect, 4.0, Color32::from_rgba_unmultiplied(20, 90, 54, 44));
            }
        }
        if let Some(cache) = waveform {
            paint_clip_waveform(painter, rect.shrink2(Vec2::new(2.0, 4.0)), clip, cache);
        }
        if let Some(buckets) = cache_buckets {
            paint_clip_cache_buckets(painter, rect, buckets);
        }
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(if selected { 2.0 } else { 1.0 }, selection_stroke),
            egui::StrokeKind::Inside,
        );
        painter.rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(4.0, rect.height())),
            2.0,
            type_stroke,
        );
        let label = timeline_clip_title(clip, asset);
        painter.text(
            rect.left_center() + Vec2::new(8.0, -6.5),
            egui::Align2::LEFT_TOP,
            label,
            FontId::proportional(10.5),
            kit::TEXT_ON_ACCENT,
        );
    }

    pub(super) fn paint_timeline_keyframe_clip(
        &self,
        painter: &egui::Painter,
        clip: &Clip,
        asset: Option<&Asset>,
        rect: Rect,
        accent: Color32,
        selected: bool,
        thumbnail_tiles: &[TimelineThumbTile],
        show_label: bool,
    ) {
        let anchor_x = rect.left() + 4.0;
        let color = if selected {
            kit::BORDER_FOCUS
        } else {
            accent.gamma_multiply(0.82)
        };
        painter.line_segment(
            [
                Pos2::new(anchor_x, rect.top() + 2.0),
                Pos2::new(anchor_x, rect.bottom() - 2.0),
            ],
            Stroke::new(if selected { 2.0 } else { 1.35 }, color),
        );
        let head = [
            Pos2::new(anchor_x - 4.5, rect.top() + 1.0),
            Pos2::new(anchor_x + 4.5, rect.top() + 1.0),
            Pos2::new(anchor_x, rect.top() + 8.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            head.to_vec(),
            color,
            Stroke::NONE,
        ));

        let thumb_size = TIMELINE_KEYFRAME_THUMB.min(rect.height() - 8.0).max(12.0);
        let thumb_rect = Rect::from_min_size(
            Pos2::new(anchor_x + 6.0, rect.top() + 2.0),
            Vec2::splat(thumb_size),
        );
        if thumb_rect.right() > rect.right() {
            return;
        }

        let thumb_frame = thumb_rect.expand(2.0);
        painter.rect_filled(
            thumb_frame,
            4.0,
            if selected {
                Color32::from_rgb(18, 50, 36)
            } else {
                Color32::from_rgb(21, 23, 27)
            },
        );
        painter.rect_stroke(
            thumb_frame,
            4.0,
            Stroke::new(if selected { 1.5 } else { 1.0 }, color),
            egui::StrokeKind::Inside,
        );

        if let Some(tile) = thumbnail_tiles.first() {
            let clip_painter = painter.with_clip_rect(thumb_rect);
            let scale = (thumb_rect.width() / tile.size.x)
                .max(thumb_rect.height() / tile.size.y)
                .max(0.01);
            let image_rect = Rect::from_center_size(thumb_rect.center(), tile.size * scale);
            clip_painter.image(
                tile.texture_id,
                image_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::from_white_alpha(if selected { 255 } else { 210 }),
            );
            painter.rect_stroke(
                thumb_rect,
                3.0,
                Stroke::new(1.0, color.gamma_multiply(0.75)),
                egui::StrokeKind::Inside,
            );
        } else {
            painter.rect_filled(thumb_rect, 3.0, kit::FIELD_BG);
            painter.text(
                thumb_rect.center(),
                egui::Align2::CENTER_CENTER,
                "IMG",
                FontId::proportional(8.5),
                kit::IMAGE,
            );
        }

        if show_label {
            let label_left = thumb_frame.right() + 5.0;
            let label = timeline_clip_title(clip, asset);
            let font_id = FontId::proportional(10.0);
            let text_w = painter
                .layout_no_wrap(label.clone(), font_id.clone(), kit::TEXT)
                .size()
                .x;
            let desired_w = (text_w + 12.0).clamp(36.0, TIMELINE_KEYFRAME_LABEL_W);
            let label_w = desired_w.min((painter.clip_rect().right() - label_left).max(0.0));
            if label_w >= 36.0 {
                let label_rect = Rect::from_min_size(
                    Pos2::new(label_left, rect.center().y - TIMELINE_MARKER_LABEL_H * 0.5),
                    Vec2::new(label_w, TIMELINE_MARKER_LABEL_H),
                );
                painter.rect_filled(label_rect, 4.0, Color32::from_rgb(21, 23, 27));
                painter.rect_stroke(
                    label_rect,
                    4.0,
                    Stroke::new(1.0, color.gamma_multiply(if selected { 1.0 } else { 0.72 })),
                    egui::StrokeKind::Inside,
                );
                let text_painter = painter.with_clip_rect(label_rect.shrink2(Vec2::new(6.0, 1.0)));
                text_painter.text(
                    label_rect.left_center() + Vec2::new(6.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    font_id,
                    kit::TEXT,
                );
            }
        }
    }

    pub(super) fn paint_timeline_marker(
        &self,
        painter: &egui::Painter,
        marker: &crate::state::Marker,
        row_rect: Rect,
        x: f32,
    ) {
        let selected = self.editor.selection.marker_ids.contains(&marker.id);
        let color = marker
            .color
            .as_deref()
            .and_then(parse_hex_color)
            .unwrap_or(kit::MARKER);
        let marker_color = if selected {
            color
        } else {
            color.gamma_multiply(0.62)
        };
        painter.line_segment(
            [
                Pos2::new(x, row_rect.top() + 4.0),
                Pos2::new(x, row_rect.bottom() - 4.0),
            ],
            Stroke::new(if selected { 2.0 } else { 1.25 }, marker_color),
        );
        let points = [
            Pos2::new(x - 4.5, row_rect.bottom() - 1.0),
            Pos2::new(x + 4.5, row_rect.bottom() - 1.0),
            Pos2::new(x, row_rect.bottom() - 8.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            points.to_vec(),
            marker_color,
            Stroke::NONE,
        ));
        if let Some((label, label_rect)) = marker_label_and_rect(marker, row_rect, x) {
            painter.rect_filled(
                label_rect,
                5.0,
                if selected {
                    Color32::from_rgb(18, 50, 36)
                } else {
                    kit::PANEL_RAISED
                },
            );
            painter.rect_stroke(
                label_rect,
                5.0,
                Stroke::new(
                    if selected { 2.0 } else { 1.0 },
                    if selected {
                        kit::BORDER_FOCUS
                    } else {
                        marker_color
                    },
                ),
                egui::StrokeKind::Inside,
            );
            painter.text(
                label_rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                FontId::proportional(10.0),
                kit::TEXT,
            );
        }
    }

    pub(super) fn paint_timeline_playhead(
        &self,
        painter: &egui::Painter,
        rects: TimelineRects,
        duration: f64,
        zoom: f32,
    ) {
        let t = snap_time_to_frame(self.editor.current_time, self.editor.project.settings.fps)
            .clamp(0.0, duration);
        let x = time_to_timeline_x(
            t,
            rects.tracks.left(),
            zoom,
            self.editor.layout.timeline_scroll_x,
        );
        painter.line_segment(
            [
                Pos2::new(x, rects.ruler.top()),
                Pos2::new(x, rects.add_row.top()),
            ],
            Stroke::new(1.5, kit::PLAYHEAD),
        );
        let head = [
            Pos2::new(x - 6.0, rects.ruler.top()),
            Pos2::new(x + 6.0, rects.ruler.top()),
            Pos2::new(x, rects.ruler.top() + 8.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            head.to_vec(),
            kit::PLAYHEAD,
            Stroke::NONE,
        ));
    }

    pub(super) fn paint_add_track_row(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        rects: TimelineRects,
    ) {
        painter.rect_filled(rects.add_row, 0.0, kit::PANEL);
        painter.line_segment(
            [
                Pos2::new(rects.outer.left(), rects.add_row.top()),
                Pos2::new(rects.outer.right(), rects.add_row.top()),
            ],
            Stroke::new(1.0, kit::BORDER_SOFT),
        );
        let button_y = rects.add_row.center().y - 12.0;
        let video_rect = Rect::from_min_size(
            Pos2::new(rects.add_row.left() + 12.0, button_y),
            Vec2::new(56.0, 24.0),
        );
        let audio_rect = Rect::from_min_size(
            Pos2::new(video_rect.right() + 6.0, button_y),
            Vec2::new(56.0, 24.0),
        );
        let marker_rect = Rect::from_min_size(
            Pos2::new(audio_rect.right() + 6.0, button_y),
            Vec2::new(66.0, 24.0),
        );
        let video_resp = ui.interact(
            video_rect,
            ui.id().with("timeline-add-video"),
            Sense::click(),
        );
        let audio_resp = ui.interact(
            audio_rect,
            ui.id().with("timeline-add-audio"),
            Sense::click(),
        );
        let marker_resp = ui.interact(
            marker_rect,
            ui.id().with("timeline-add-marker"),
            Sense::click(),
        );
        let input_frozen = ui.ctx().any_popup_open();
        if !input_frozen && video_resp.clicked() {
            let track_id = self.editor.project.add_video_track();
            self.editor.selection.select_track(track_id);
            self.editor.status = "Added video track".to_string();
        }
        if !input_frozen && audio_resp.clicked() {
            let track_id = self.editor.project.add_audio_track();
            self.editor.selection.select_track(track_id);
            self.editor.status = "Added audio track".to_string();
        }
        if !input_frozen && marker_resp.clicked() {
            let track_id = self.editor.project.add_marker_track();
            self.editor.selection.select_track(track_id);
            self.editor.status = "Added marker track".to_string();
        }
        paint_dashed_timeline_button(
            painter,
            video_rect,
            "+ Video",
            kit::VIDEO,
            !input_frozen && video_resp.hovered(),
        );
        paint_dashed_timeline_button(
            painter,
            audio_rect,
            "+ Audio",
            kit::AUDIO,
            !input_frozen && audio_resp.hovered(),
        );
        paint_dashed_timeline_button(
            painter,
            marker_rect,
            "+ Marker",
            kit::MARKER,
            !input_frozen && marker_resp.hovered(),
        );
    }

    pub(super) fn paint_timeline_scrollbar(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        rects: TimelineRects,
        content_w: f32,
        viewport_w: f32,
    ) {
        if content_w <= viewport_w + 1.0 {
            return;
        }
        let max_scroll = (content_w - viewport_w).max(0.0);
        let handle_w =
            (viewport_w / content_w * rects.scrollbar.width()).clamp(42.0, rects.scrollbar.width());
        let handle_x = rects.scrollbar.left()
            + (self.editor.layout.timeline_scroll_x / max_scroll)
                * (rects.scrollbar.width() - handle_w);
        let handle = Rect::from_min_size(
            Pos2::new(handle_x, rects.scrollbar.center().y - 3.0),
            Vec2::new(handle_w, 6.0),
        );
        painter.rect_filled(
            rects.scrollbar.shrink2(Vec2::new(0.0, 4.0)),
            3.0,
            kit::FIELD_BG,
        );
        painter.rect_filled(handle, 3.0, kit::BORDER);
        let response = ui.interact(
            rects.scrollbar,
            ui.id().with("timeline-scrollbar"),
            Sense::click_and_drag(),
        );
        if ui.ctx().any_popup_open() {
            return;
        }
        if (response.dragged() || response.clicked()) && response.interact_pointer_pos().is_some() {
            let pos = response.interact_pointer_pos().unwrap();
            let ratio = ((pos.x - rects.scrollbar.left() - handle_w * 0.5)
                / (rects.scrollbar.width() - handle_w).max(1.0))
            .clamp(0.0, 1.0);
            self.editor.layout.timeline_scroll_x = ratio * max_scroll;
        }
    }

    pub(super) fn paint_timeline_vertical_scrollbar(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        rects: TimelineRects,
        content_h: f32,
    ) {
        let viewport_h = rects.tracks.height().max(1.0);
        if content_h <= viewport_h + 1.0 {
            return;
        }

        let max_scroll = (content_h - viewport_h).max(0.0);
        let rail = Rect::from_min_max(
            Pos2::new(rects.tracks.right() - 7.0, rects.tracks.top() + 4.0),
            Pos2::new(rects.tracks.right() - 3.0, rects.tracks.bottom() - 4.0),
        );
        let handle_h = (viewport_h / content_h * rail.height()).clamp(28.0, rail.height());
        let handle_y = rail.top()
            + (self.editor.layout.timeline_scroll_y / max_scroll) * (rail.height() - handle_h);
        let handle = Rect::from_min_size(
            Pos2::new(rail.left(), handle_y),
            Vec2::new(rail.width(), handle_h),
        );
        painter.rect_filled(rail, 2.0, Color32::from_rgba_unmultiplied(5, 7, 10, 110));
        painter.rect_filled(handle, 2.0, kit::BORDER.gamma_multiply(1.25));

        let response = ui.interact(
            rail.expand2(Vec2::new(6.0, 0.0)),
            ui.id().with("timeline-vertical-scrollbar"),
            Sense::click_and_drag(),
        );
        if ui.ctx().any_popup_open() {
            return;
        }
        if (response.dragged() || response.clicked()) && response.interact_pointer_pos().is_some() {
            let pos = response.interact_pointer_pos().unwrap();
            let ratio = ((pos.y - rail.top() - handle_h * 0.5)
                / (rail.height() - handle_h).max(1.0))
            .clamp(0.0, 1.0);
            self.editor.layout.timeline_scroll_y = ratio * max_scroll;
        }
    }

    pub(super) fn handle_timeline_pointer(
        &mut self,
        ui: &mut Ui,
        response: &egui::Response,
        rects: TimelineRects,
        tracks: &[crate::state::Track],
        clips: &[Clip],
        clip_geoms: &[TimelineClipGeom],
        marker_geoms: &[TimelineMarkerGeom],
        duration: f64,
        zoom: f32,
    ) {
        if ui.ctx().any_popup_open() {
            return;
        }
        if let Some(pos) = ui
            .ctx()
            .pointer_hover_pos()
            .filter(|pos| rects.outer.contains(*pos))
        {
            let cursor = if let Some(payload) =
                egui::DragAndDrop::payload::<AssetTimelineDragPayload>(ui.ctx())
            {
                timeline_track_row_at_pos(pos, rects, tracks)
                    .map(|track| {
                        if self
                            .editor
                            .project
                            .asset_compatible_with_track(payload.asset_id, track.id)
                        {
                            egui::CursorIcon::Copy
                        } else {
                            egui::CursorIcon::NoDrop
                        }
                    })
                    .unwrap_or(egui::CursorIcon::NoDrop)
            } else {
                match self.timeline_drag {
                    Some(TimelineDrag::ClipResizeLeft { .. })
                    | Some(TimelineDrag::ClipResizeRight { .. })
                    | Some(TimelineDrag::MarkerMove { .. })
                    | Some(TimelineDrag::Playhead) => egui::CursorIcon::ResizeHorizontal,
                    Some(TimelineDrag::ClipMove { .. }) => egui::CursorIcon::Grabbing,
                    None => match timeline_hit(pos, rects, tracks, clip_geoms, marker_geoms) {
                        TimelineHit::ClipLeftEdge(_)
                        | TimelineHit::ClipRightEdge(_)
                        | TimelineHit::Marker(_)
                        | TimelineHit::Ruler => egui::CursorIcon::ResizeHorizontal,
                        TimelineHit::ClipBody(_) => egui::CursorIcon::Grab,
                        TimelineHit::TrackLabel(_) => egui::CursorIcon::PointingHand,
                        TimelineHit::EmptyTrack | TimelineHit::Empty => egui::CursorIcon::Default,
                    },
                }
            };
            ui.ctx().set_cursor_icon(cursor);
        }

        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                let toggle_select = multi_select_modifier(ui);
                match timeline_hit(pos, rects, tracks, clip_geoms, marker_geoms) {
                    TimelineHit::Ruler => {
                        self.timeline_scrub_was_playing = self.editor.is_playing;
                        self.timeline_last_scrub_audio_time = None;
                        if self.editor.is_playing {
                            self.editor.is_playing = false;
                        }
                        self.timeline_drag = Some(TimelineDrag::Playhead);
                        self.seek_from_timeline_pos(
                            pos,
                            rects,
                            duration,
                            zoom,
                            true,
                            timeline_snapping_enabled(ui),
                        );
                    }
                    TimelineHit::ClipLeftEdge(id) => {
                        if let Some(clip) = clips.iter().find(|clip| clip.id == id) {
                            if !self.editor.selection.clip_ids.contains(&id) {
                                self.editor.selection.select_clip(id);
                            }
                            self.timeline_drag = Some(TimelineDrag::ClipResizeLeft {
                                clip_id: id,
                                start_time: clip.start_time,
                                duration: clip.duration,
                            });
                        }
                    }
                    TimelineHit::ClipRightEdge(id) => {
                        if let Some(clip) = clips.iter().find(|clip| clip.id == id) {
                            if !self.editor.selection.clip_ids.contains(&id) {
                                self.editor.selection.select_clip(id);
                            }
                            self.timeline_drag = Some(TimelineDrag::ClipResizeRight {
                                clip_id: id,
                                start_time: clip.start_time,
                                duration: clip.duration,
                            });
                        }
                    }
                    TimelineHit::ClipBody(id) => {
                        if let Some(clip) = clips.iter().find(|clip| clip.id == id) {
                            if toggle_select {
                                self.editor.selection.toggle_clip(id);
                            } else if !self.editor.selection.clip_ids.contains(&id) {
                                self.editor.selection.select_clip(id);
                            }
                            self.timeline_drag = Some(TimelineDrag::ClipMove {
                                clip_id: id,
                                start_time: clip.start_time,
                                duration: clip.duration,
                            });
                        }
                    }
                    TimelineHit::Marker(id) => {
                        if let Some(marker) = self
                            .editor
                            .project
                            .markers
                            .iter()
                            .find(|marker| marker.id == id)
                        {
                            self.editor.selection.select_marker(id);
                            self.timeline_drag = Some(TimelineDrag::MarkerMove {
                                marker_id: id,
                                start_time: marker.time,
                            });
                        }
                    }
                    TimelineHit::TrackLabel(id) => self.editor.selection.select_track(id),
                    TimelineHit::EmptyTrack | TimelineHit::Empty => {}
                }
            }
        }

        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let drag_delta_x = response
                    .total_drag_delta()
                    .map(|delta| delta.x)
                    .unwrap_or_else(|| response.drag_delta().x);
                self.apply_timeline_drag(
                    drag_delta_x,
                    pos,
                    rects,
                    duration,
                    zoom,
                    timeline_snapping_enabled(ui),
                );
            }
        }

        let primary_down = ui.input(|input| input.pointer.primary_down());
        if !primary_down && self.timeline_drag.is_some() {
            let was_playhead_drag = matches!(self.timeline_drag, Some(TimelineDrag::Playhead));
            self.timeline_drag = None;
            self.timeline_snap_preview = None;
            if was_playhead_drag {
                self.finish_timeline_scrub();
            }
        }

        if response.double_clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                match timeline_hit(pos, rects, tracks, clip_geoms, marker_geoms) {
                    TimelineHit::ClipLeftEdge(id)
                    | TimelineHit::ClipRightEdge(id)
                    | TimelineHit::ClipBody(id) => {
                        if let Some(clip) = clips.iter().find(|clip| clip.id == id) {
                            let local_time = (self.editor.current_time - clip.start_time
                                + clip.trim_in_seconds)
                                .max(0.0);
                            self.open_asset_lab_at_time(clip.asset_id, Some(local_time));
                        }
                    }
                    _ => {}
                }
            }
        }

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let toggle_select = multi_select_modifier(ui);
                match timeline_hit(pos, rects, tracks, clip_geoms, marker_geoms) {
                    TimelineHit::Ruler => self.seek_from_timeline_pos(
                        pos,
                        rects,
                        duration,
                        zoom,
                        false,
                        timeline_snapping_enabled(ui),
                    ),
                    TimelineHit::ClipLeftEdge(id)
                    | TimelineHit::ClipRightEdge(id)
                    | TimelineHit::ClipBody(id) => {
                        if toggle_select {
                            self.editor.selection.toggle_clip(id);
                        } else {
                            self.editor.selection.select_clip(id);
                        }
                    }
                    TimelineHit::Marker(id) => self.editor.selection.select_marker(id),
                    TimelineHit::TrackLabel(id) => self.editor.selection.select_track(id),
                    TimelineHit::EmptyTrack => self.editor.selection.clear(),
                    TimelineHit::Empty => {}
                }
            }
        }
    }

    pub(super) fn seek_from_timeline_pos(
        &mut self,
        pos: Pos2,
        rects: TimelineRects,
        duration: f64,
        zoom: f32,
        scrub_audio: bool,
        snap_enabled: bool,
    ) {
        let raw_time = ((pos.x - rects.tracks.left() + self.editor.layout.timeline_scroll_x) / zoom)
            .clamp(0.0, duration as f32) as f64;
        let fps = self.editor.project.settings.fps.max(1.0);
        let raw_frames = frames_from_seconds(raw_time, fps);
        let snap_threshold_frames =
            (TIMELINE_SNAP_THRESHOLD_PX / zoom.max(TIMELINE_MIN_ZOOM_FLOOR) as f64) * fps;
        let seek_frames = if snap_enabled {
            let targets = self.timeline_snap_targets(None, None, false);
            if let Some(hit) =
                best_snap_delta_frames(&[raw_frames], &targets, snap_threshold_frames)
            {
                self.timeline_snap_preview = if scrub_audio {
                    Some(seconds_from_frames(hit.target.frame, fps))
                } else {
                    None
                };
                raw_frames + hit.delta_frames
            } else {
                self.timeline_snap_preview = None;
                raw_frames
            }
        } else {
            self.timeline_snap_preview = None;
            raw_frames
        };
        let max_frames = frames_from_seconds(duration, fps).round();
        let time = seconds_from_frames(seek_frames.round().clamp(0.0, max_frames), fps);
        self.seek_editor(time, scrub_audio);
    }

    pub(super) fn apply_timeline_drag(
        &mut self,
        delta_x: f32,
        pos: Pos2,
        rects: TimelineRects,
        duration: f64,
        zoom: f32,
        snap_enabled: bool,
    ) {
        let Some(drag) = self.timeline_drag else {
            return;
        };
        let fps = self.editor.project.settings.fps.max(1.0);
        let delta_frames = (delta_x as f64 / zoom.max(TIMELINE_MIN_ZOOM_FLOOR) as f64) * fps;
        let min_duration_frames = (0.1 * fps).ceil().max(1.0);
        let snap_threshold_frames = (TIMELINE_SNAP_THRESHOLD_PX / zoom as f64) * fps;
        match drag {
            TimelineDrag::Playhead => {
                self.seek_from_timeline_pos(pos, rects, duration, zoom, true, snap_enabled)
            }
            TimelineDrag::ClipMove {
                clip_id,
                start_time,
                duration: clip_duration,
            } => {
                let start_frames = frames_from_seconds(start_time, fps).round();
                let duration_frames = frames_from_seconds(clip_duration, fps).round();
                let mut new_start_frames = start_frames + delta_frames;
                if snap_enabled {
                    let targets = self.timeline_snap_targets(Some(clip_id), None, true);
                    let is_keyframe_reference = self
                        .editor
                        .project
                        .clips
                        .iter()
                        .find(|clip| clip.id == clip_id)
                        .is_some_and(|clip| self.editor.project.is_keyframe_reference_clip(clip));
                    let source_frames = if is_keyframe_reference {
                        vec![new_start_frames]
                    } else {
                        vec![new_start_frames, new_start_frames + duration_frames]
                    };
                    if let Some(hit) =
                        best_snap_delta_frames(&source_frames, &targets, snap_threshold_frames)
                    {
                        new_start_frames += hit.delta_frames;
                        self.timeline_snap_preview =
                            Some(seconds_from_frames(hit.target.frame, fps));
                    } else {
                        self.timeline_snap_preview = None;
                    }
                } else {
                    self.timeline_snap_preview = None;
                }
                let new_start = seconds_from_frames(new_start_frames.round().max(0.0), fps);
                let mut changed = self.editor.project.move_clip(clip_id, new_start);
                if let Some(track_id) =
                    timeline_track_row_at_pos(pos, rects, &self.editor.project.tracks)
                        .map(|track| track.id)
                {
                    changed |= self.editor.project.move_clip_to_track(clip_id, track_id);
                }
                if changed {
                    self.editor.preview_dirty = true;
                }
            }
            TimelineDrag::ClipResizeLeft {
                clip_id,
                start_time,
                duration: clip_duration,
            } => {
                let end_frames = frames_from_seconds(start_time + clip_duration, fps).round();
                let mut new_start_frames =
                    frames_from_seconds(start_time, fps).round() + delta_frames;
                if snap_enabled {
                    let targets = self.timeline_snap_targets(Some(clip_id), None, true);
                    if let Some(hit) =
                        best_snap_delta_frames(&[new_start_frames], &targets, snap_threshold_frames)
                    {
                        new_start_frames += hit.delta_frames;
                        self.timeline_snap_preview =
                            Some(seconds_from_frames(hit.target.frame, fps));
                    } else {
                        self.timeline_snap_preview = None;
                    }
                } else {
                    self.timeline_snap_preview = None;
                }
                new_start_frames = new_start_frames.clamp(0.0, end_frames - min_duration_frames);
                let new_duration_frames = (end_frames - new_start_frames).max(min_duration_frames);
                let new_start = seconds_from_frames(new_start_frames.round(), fps);
                let new_duration = seconds_from_frames(new_duration_frames.round(), fps);
                if self
                    .editor
                    .project
                    .resize_clip(clip_id, new_start, new_duration)
                {
                    self.editor.preview_dirty = true;
                }
            }
            TimelineDrag::ClipResizeRight {
                clip_id,
                start_time,
                duration: clip_duration,
            } => {
                let start_frames = frames_from_seconds(start_time, fps).round();
                let mut new_end_frames =
                    start_frames + frames_from_seconds(clip_duration, fps).round() + delta_frames;
                if snap_enabled {
                    let targets = self.timeline_snap_targets(Some(clip_id), None, true);
                    if let Some(hit) =
                        best_snap_delta_frames(&[new_end_frames], &targets, snap_threshold_frames)
                    {
                        new_end_frames += hit.delta_frames;
                        self.timeline_snap_preview =
                            Some(seconds_from_frames(hit.target.frame, fps));
                    } else {
                        self.timeline_snap_preview = None;
                    }
                } else {
                    self.timeline_snap_preview = None;
                }
                let new_duration_frames = (new_end_frames - start_frames).max(min_duration_frames);
                let new_duration = seconds_from_frames(new_duration_frames.round(), fps);
                if self
                    .editor
                    .project
                    .resize_clip(clip_id, start_time, new_duration)
                {
                    self.editor.preview_dirty = true;
                }
            }
            TimelineDrag::MarkerMove {
                marker_id,
                start_time,
            } => {
                let mut new_frames = frames_from_seconds(start_time, fps).round() + delta_frames;
                if snap_enabled {
                    let targets = self.timeline_snap_targets(None, Some(marker_id), true);
                    if let Some(hit) =
                        best_snap_delta_frames(&[new_frames], &targets, snap_threshold_frames)
                    {
                        new_frames += hit.delta_frames;
                        self.timeline_snap_preview =
                            Some(seconds_from_frames(hit.target.frame, fps));
                    } else {
                        self.timeline_snap_preview = None;
                    }
                } else {
                    self.timeline_snap_preview = None;
                }
                let max_frames = frames_from_seconds(duration, fps).round();
                let new_time = seconds_from_frames(new_frames.round().clamp(0.0, max_frames), fps);
                if self.editor.project.move_marker(marker_id, new_time) {
                    self.editor.preview_dirty = true;
                }
            }
        }
    }

    pub(super) fn timeline_snap_targets(
        &self,
        exclude_clip: Option<Uuid>,
        exclude_marker: Option<Uuid>,
        include_playhead: bool,
    ) -> Vec<SnapTarget> {
        let fps = self.editor.project.settings.fps.max(1.0);
        let mut targets = Vec::new();
        if include_playhead {
            targets.push(SnapTarget::playhead(frames_from_seconds(
                self.editor.current_time,
                fps,
            )));
        }
        for clip in self.editor.project.clips.iter() {
            if Some(clip.id) == exclude_clip {
                continue;
            }
            targets.push(SnapTarget::clip_edge(
                frames_from_seconds(clip.start_time, fps).round(),
                clip.id,
            ));
            if !self.editor.project.is_keyframe_reference_clip(clip) {
                targets.push(SnapTarget::clip_edge(
                    frames_from_seconds(clip.end_time(), fps).round(),
                    clip.id,
                ));
            }
        }
        for marker in self.editor.project.markers.iter() {
            if Some(marker.id) == exclude_marker {
                continue;
            }
            targets.push(SnapTarget::marker(
                frames_from_seconds(marker.time, fps).round(),
                marker.id,
            ));
        }
        targets
    }

    pub(super) fn audio_peak_cache(&mut self, ctx: &Context, asset: &Asset) -> Option<PeakCache> {
        if let Some(cache) = self.audio_peak_caches.get(&asset.id) {
            return Some(cache.clone());
        }
        let project_root = self.editor.project_root()?.to_path_buf();
        let source = resolve_audio_source(&project_root, asset)?;
        let cache_path = peak_cache_path(&project_root, asset.id);
        if cache_path.exists() {
            if let Ok(cache) = load_peak_cache(&cache_path) {
                if cache_matches_source(&cache, &source).unwrap_or(false) {
                    self.audio_peak_caches.insert(asset.id, cache.clone());
                    return Some(cache);
                }
            }
        }
        if self.audio_peak_builds.insert(asset.id) {
            let ctx = ctx.clone();
            let asset_id = asset.id;
            std::thread::spawn(move || {
                let _ = build_and_store_peak_cache(
                    &project_root,
                    asset_id,
                    &source,
                    PeakBuildConfig::default(),
                );
                ctx.request_repaint();
            });
        }
        None
    }
}
