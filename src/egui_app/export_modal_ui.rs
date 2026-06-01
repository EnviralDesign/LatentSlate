use super::*;

impl LatentSlateApp {
    pub(super) fn open_export_modal(&mut self) {
        if self.export_cancel.is_none() {
            self.export_modal = ExportModalState::for_project(&self.editor.project);
            self.export_preview_texture = None;
        }
        self.editor.overlays.export_video = true;
    }

    pub(super) fn close_or_cancel_export_modal(&mut self) {
        if let Some(cancel) = &self.export_cancel {
            cancel.store(true, Ordering::Relaxed);
            self.export_modal.message = "Cancelling export...".to_string();
            self.export_modal.status = ExportRunStatus::Running;
        } else {
            self.editor.overlays.export_video = false;
        }
    }

    pub(super) fn export_video_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let size = modal_size(ctx, EXPORT_MODAL_SIZE, [580.0, 500.0]);
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "export_video", true);
        egui::Window::new("Export Video")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "Export Video",
                    Some("Render the current timeline to an MP4 file."),
                    true,
                );
                kit::modal_body(ui, |ui| self.export_video_modal_contents(ui));
            });

        if close_clicked || outside_clicked || !open {
            self.close_or_cancel_export_modal();
        }
    }

    pub(super) fn export_video_modal_contents(&mut self, ui: &mut Ui) {
        let footer_h = kit::PRIMARY_BUTTON_H;
        let full_rect = ui.available_rect_before_wrap();
        let (rect, _) = ui.allocate_exact_size(full_rect.size(), Sense::hover());
        let footer_rect = Rect::from_min_max(
            Pos2::new(rect.left(), rect.bottom() - footer_h),
            rect.right_bottom(),
        );
        let body_rect =
            Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), footer_rect.top()));

        let mut body_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body_rect)
                .layout(Layout::left_to_right(Align::Min)),
        );
        body_ui.shrink_clip_rect(body_rect);
        let available_w = body_ui.available_width();
        let gap = 12.0;
        let left_w = ((available_w - gap) * 0.58).max(300.0);
        let right_w = (available_w - gap - left_w).max(220.0);
        body_ui.spacing_mut().item_spacing.x = gap;
        body_ui.allocate_ui_with_layout(
            Vec2::new(left_w, body_rect.height()),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(left_w);
                self.export_settings_card(ui);
            },
        );
        body_ui.allocate_ui_with_layout(
            Vec2::new(right_w, body_rect.height()),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(right_w);
                self.export_progress_card(ui);
            },
        );

        let mut footer_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(footer_rect)
                .layout(Layout::right_to_left(Align::Center)),
        );
        footer_ui.shrink_clip_rect(footer_rect);
        self.export_footer(&mut footer_ui);
    }

    pub(super) fn export_settings_card(&mut self, ui: &mut Ui) {
        let running = self.export_cancel.is_some();
        kit::card_panel(ui, ui.available_height(), |ui| {
            ui.add_enabled_ui(!running, |ui| {
                kit::scroll_body(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                    kit::field_label(ui, "Output");
                    let initial_dir = self
                        .editor
                        .project
                        .project_path
                        .as_ref()
                        .map(|root| root.join("exports"))
                        .unwrap_or_else(|| default_projects_dir().join("exports"));
                    let options = kit::BrowseFileOptions::new()
                        .button_label("Browse")
                        .initial_dir(initial_dir.as_path())
                        .remember_last_dir()
                        .id_salt("export_output_file")
                        .filters(MP4_FILE_FILTERS);
                    if let Some(path) = kit::labeled_save_file_field(
                        ui,
                        "Output File",
                        &mut self.export_modal.output_path,
                        options,
                    ) {
                        self.export_modal.output_path =
                            ensure_mp4_extension(path).display().to_string();
                    }
                    ui.add_space(kit::ACTION_GAP);

                    kit::field_label(ui, "Video");
                    kit::field_grid_row(ui, &[1.0, 1.0, 1.0], |ui, index| match index {
                        0 => {
                            kit::labeled_text_field(ui, "Width", &mut self.export_modal.width);
                        }
                        1 => {
                            kit::labeled_text_field(ui, "Height", &mut self.export_modal.height);
                        }
                        _ => {
                            kit::labeled_text_field(ui, "FPS", &mut self.export_modal.fps);
                        }
                    });
                    ui.add_space(kit::FORM_ROW_GAP);
                    kit::field_grid_row(ui, &[1.0, 2.0], |ui, index| match index {
                        0 => {
                            kit::labeled_combo_field(
                                ui,
                                "Codec",
                                "export_codec",
                                self.export_modal.codec.label(),
                                |ui| {
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.codec,
                                        VideoExportCodec::H264,
                                        "H.264",
                                    );
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.codec,
                                        VideoExportCodec::H265,
                                        "H.265",
                                    );
                                },
                            );
                        }
                        _ => {
                            kit::labeled_combo_field(
                                ui,
                                "Perceptual Quality",
                                "export_quality",
                                self.export_modal.quality.label(),
                                |ui| {
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.quality,
                                        VideoExportQuality::Compact,
                                        "Compact",
                                    );
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.quality,
                                        VideoExportQuality::Balanced,
                                        "Balanced",
                                    );
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.quality,
                                        VideoExportQuality::High,
                                        "High Quality",
                                    );
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.quality,
                                        VideoExportQuality::NearLossless,
                                        "Near Lossless",
                                    );
                                },
                            );
                        }
                    });
                    ui.add_space(kit::FORM_ROW_GAP);
                    kit::field_grid_row(ui, &[1.0], |ui, _index| {
                        kit::labeled_combo_field(
                            ui,
                            "Intermediate Format",
                            "export_frame_format",
                            self.export_modal.frame_format.label(),
                            |ui| {
                                automation_selectable_value(
                                    ui,
                                    &mut self.export_modal.frame_format,
                                    VideoExportFrameFormat::Png,
                                    "PNG",
                                );
                                automation_selectable_value(
                                    ui,
                                    &mut self.export_modal.frame_format,
                                    VideoExportFrameFormat::Bmp,
                                    "BMP (Fast)",
                                );
                            },
                        );
                    });
                    ui.add_space(kit::ACTION_GAP);

                    kit::field_label(ui, "Range");
                    kit::field_grid_row(ui, &[1.0, 1.0], |ui, index| match index {
                        0 => {
                            kit::labeled_text_field(
                                ui,
                                "Start Seconds",
                                &mut self.export_modal.start_seconds,
                            );
                        }
                        _ => {
                            kit::labeled_text_field(
                                ui,
                                "Duration Seconds",
                                &mut self.export_modal.duration_seconds,
                            );
                        }
                    });
                    ui.add_space(kit::FORM_ROW_GAP);
                    automation_checkbox(ui, &mut self.export_modal.include_audio, "Include audio");
                    ui.add_space(kit::ACTION_GAP);

                    kit::field_label(ui, "Burn In");
                    automation_checkbox(
                        ui,
                        &mut self.export_modal.timestamp_overlay_enabled,
                        "Timestamp overlay",
                    );
                    ui.add_enabled_ui(self.export_modal.timestamp_overlay_enabled, |ui| {
                        ui.add_space(kit::FORM_ROW_GAP);
                        kit::labeled_combo_field(
                            ui,
                            "Timestamp Position",
                            "export_timestamp_position",
                            self.export_modal.timestamp_overlay_position.label(),
                            |ui| {
                                automation_selectable_value(
                                    ui,
                                    &mut self.export_modal.timestamp_overlay_position,
                                    TimestampOverlayPosition::TopCenter,
                                    "Top Center",
                                );
                                automation_selectable_value(
                                    ui,
                                    &mut self.export_modal.timestamp_overlay_position,
                                    TimestampOverlayPosition::BottomCenter,
                                    "Bottom Center",
                                );
                            },
                        );
                    });
                });
            });
        });
    }

    pub(super) fn export_progress_card(&mut self, ui: &mut Ui) {
        kit::card_frame().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
            kit::field_label(ui, "Progress");
            ui.label(kit::body(&self.export_modal.message));
            ui.add(
                egui::ProgressBar::new(self.export_modal.progress.clamp(0.0, 1.0))
                    .show_percentage()
                    .animate(self.export_cancel.is_some())
                    .desired_width(ui.available_width()),
            );
            if !self.export_modal.frame_label.is_empty() {
                ui.label(kit::caption(&self.export_modal.frame_label));
            } else {
                ui.label(kit::caption(&self.export_modal.stage));
            }
            ui.add_space(kit::FORM_ROW_GAP);
            self.export_preview(ui);
            if let Some(error) = &self.export_modal.error {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(RichText::new(error).color(kit::DANGER).size(11.0));
            }
            if let Some(summary) = &self.export_modal.summary {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(format!(
                    "{} {}, {}, {} frames, {:.2}s{}",
                    summary.codec.label(),
                    self.export_modal.quality.label(),
                    summary.frame_format.label(),
                    summary.frame_count,
                    summary.duration_seconds,
                    if summary.audio_included {
                        ", audio"
                    } else {
                        ""
                    }
                )));
                ui.label(kit::caption(path_label(&summary.output_path)));
            }
            if !self.export_modal.warnings.is_empty() {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(format!(
                    "{} warning{}",
                    self.export_modal.warnings.len(),
                    if self.export_modal.warnings.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                )));
            }
        });
    }

    pub(super) fn export_preview(&mut self, ui: &mut Ui) {
        let size = Vec2::new(ui.available_width(), 150.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter()
            .rect_filled(rect, kit::field_radius(), kit::FIELD_BG);
        ui.painter().rect_stroke(
            rect,
            kit::field_radius(),
            Stroke::new(1.0, kit::BORDER_SOFT),
            egui::StrokeKind::Inside,
        );
        let Some(texture) = &self.export_preview_texture else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Preview appears during export",
                FontId::proportional(11.0),
                kit::TEXT_DIM,
            );
            return;
        };
        let texture_size = texture.size_vec2();
        let scale = (rect.width() / texture_size.x.max(1.0))
            .min(rect.height() / texture_size.y.max(1.0))
            .min(1.0);
        let image_size = texture_size * scale;
        let image_rect = Rect::from_center_size(rect.center(), image_size);
        ui.painter().image(
            texture.id(),
            image_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    pub(super) fn export_footer(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if self.export_cancel.is_some() {
                if kit::danger_button(ui, "Cancel Export", 150.0).clicked() {
                    self.close_or_cancel_export_modal();
                }
                return;
            }
            if matches!(self.export_modal.status, ExportRunStatus::Finished) {
                if kit::primary_button(ui, "Close", 120.0).clicked() {
                    self.editor.overlays.export_video = false;
                }
                if kit::secondary_button(ui, "Export Again", 130.0).clicked() {
                    self.start_export_video();
                }
                return;
            }
            if kit::primary_button(ui, "Export Video", 150.0).clicked() {
                self.start_export_video();
            }
            if kit::secondary_button(ui, "Cancel", 110.0).clicked() {
                self.editor.overlays.export_video = false;
            }
        });
    }

    pub(super) fn start_export_video(&mut self) {
        let settings = match self.export_modal.to_settings() {
            Ok(settings) => settings,
            Err(err) => {
                self.export_modal.status = ExportRunStatus::Failed;
                self.export_modal.error = Some(err);
                self.export_modal.message = "Export settings need attention.".to_string();
                self.export_modal.progress = 0.0;
                return;
            }
        };
        let project = self.editor.project.clone();
        let job = VideoExportJob { project, settings };
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);
        let events = self.export_events_tx.clone();

        self.export_modal.status = ExportRunStatus::Running;
        self.export_modal.progress = 0.0;
        self.export_modal.stage = "preparing".to_string();
        self.export_modal.message = "Preparing export".to_string();
        self.export_modal.frame_label.clear();
        self.export_modal.error = None;
        self.export_modal.summary = None;
        self.export_modal.warnings.clear();
        self.export_preview_texture = None;
        self.export_cancel = Some(cancel);
        self.editor.status = "Export started".to_string();

        std::thread::spawn(move || {
            export_video(job, cancel_for_thread, |event| {
                let _ = events.send(event);
            });
        });
    }

    pub(super) fn service_export_events(&mut self, ctx: &Context) {
        while let Ok(event) = self.export_events_rx.try_recv() {
            match event {
                VideoExportEvent::Progress {
                    stage,
                    message,
                    progress,
                    frame_index,
                    total_frames,
                    preview,
                } => {
                    self.export_modal.stage = stage.to_string();
                    self.export_modal.message = message;
                    self.export_modal.progress = progress.clamp(0.0, 1.0);
                    self.export_modal.frame_label = match (frame_index, total_frames) {
                        (Some(frame), Some(total)) => format!("Frame {frame} of {total}"),
                        _ => self.export_modal.stage.clone(),
                    };
                    if let Some(preview) = preview {
                        self.update_export_preview_texture(ctx, preview);
                    }
                }
                VideoExportEvent::Finished(summary) => {
                    self.export_cancel = None;
                    self.export_modal.status = ExportRunStatus::Finished;
                    self.export_modal.progress = 1.0;
                    self.export_modal.stage = "complete".to_string();
                    self.export_modal.message = "Export complete".to_string();
                    self.export_modal.frame_label.clear();
                    self.export_modal.warnings = summary.warnings.clone();
                    self.editor.status = format!("Exported {}", path_label(&summary.output_path));
                    self.export_modal.summary = Some(summary);
                }
                VideoExportEvent::Cancelled => {
                    self.export_cancel = None;
                    self.export_modal.status = ExportRunStatus::Cancelled;
                    self.export_modal.stage = "cancelled".to_string();
                    self.export_modal.message = "Export cancelled".to_string();
                    self.export_modal.error = None;
                    self.editor.status = "Export cancelled".to_string();
                }
                VideoExportEvent::Failed(err) => {
                    self.export_cancel = None;
                    self.export_modal.status = ExportRunStatus::Failed;
                    self.export_modal.stage = "failed".to_string();
                    self.export_modal.message = "Export failed".to_string();
                    self.export_modal.error = Some(err.clone());
                    self.editor.status = format!("Export failed: {err}");
                }
            }
        }

        if self.export_cancel.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }

    pub(super) fn update_export_preview_texture(
        &mut self,
        ctx: &Context,
        preview: VideoExportPreview,
    ) {
        if preview.width == 0 || preview.height == 0 || preview.rgba.is_empty() {
            return;
        }
        let image = ColorImage::from_rgba_unmultiplied(
            [preview.width as usize, preview.height as usize],
            &preview.rgba,
        );
        if let Some(texture) = self.export_preview_texture.as_mut() {
            texture.set(image, TextureOptions::LINEAR);
        } else {
            self.export_preview_texture =
                Some(ctx.load_texture("export-preview", image, TextureOptions::LINEAR));
        }
    }
}
