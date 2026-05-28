use super::*;

impl NlaEguiApp {
    pub(super) fn top_bar(&mut self, root: &mut Ui) {
        let response = egui::Panel::top("top_bar")
            .exact_size(kit::TOP_BAR_H)
            .frame(kit::chrome_frame())
            .show_inside(root, |ui| {
                ui.horizontal_centered(|ui| {
                    menu_button(
                        ui,
                        "File",
                        |ui, this: &mut Self| {
                            if automation_button(ui.button("New Project..."), "New Project...")
                                .clicked()
                            {
                                this.editor.overlays.new_project = true;
                                ui.close();
                            }
                            if automation_button(ui.button("Open Project..."), "Open Project...")
                                .clicked()
                            {
                                let initial_dir = default_projects_dir();
                                let options = kit::BrowsePathOptions::new()
                                    .id_salt("menu_open_project")
                                    .initial_dir(initial_dir.as_path())
                                    .remember_last_dir();
                                if let Some(folder) = kit::pick_folder_dialog(ui, options) {
                                    this.open_project_folder(folder);
                                }
                                ui.close();
                            }
                            ui.add_enabled_ui(this.editor.project.project_path.is_some(), |ui| {
                                if automation_button(
                                    ui.button("Project Settings..."),
                                    "Project Settings...",
                                )
                                .clicked()
                                {
                                    this.project_settings = this.editor.project.settings.clone();
                                    this.editor.overlays.project_settings = true;
                                    ui.close();
                                }
                                if automation_button(ui.button("Save"), "Save").clicked() {
                                    if let Err(err) = this.editor.save() {
                                        this.editor.status = err;
                                    }
                                    ui.close();
                                }
                                if automation_button(
                                    ui.button("Export Video..."),
                                    "Export Video...",
                                )
                                .clicked()
                                {
                                    this.open_export_modal();
                                    ui.close();
                                }
                            });
                            ui.separator();
                            if automation_button(ui.button("Quit"), "Quit").clicked() {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                                ui.close();
                            }
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "Edit",
                        |ui, this: &mut Self| {
                            if automation_button(ui.button("Add Marker"), "Add Marker").clicked() {
                                this.editor.add_marker(None);
                                ui.close();
                            }
                            if automation_button(
                                ui.button("Create Generative Video..."),
                                "Create Generative Video...",
                            )
                            .clicked()
                            {
                                this.editor.overlays.generative_video = true;
                                ui.close();
                            }
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "View",
                        |ui, this: &mut Self| {
                            automation_checkbox(
                                ui,
                                &mut this.editor.layout.preview_stats,
                                "Preview Stats",
                            );
                            let mut show_assets = !this.editor.layout.left_collapsed;
                            if automation_checkbox(ui, &mut show_assets, "Assets").changed() {
                                this.editor.layout.left_collapsed = !show_assets;
                            }
                            let mut show_attributes = !this.editor.layout.right_collapsed;
                            if automation_checkbox(ui, &mut show_attributes, "Attributes").changed()
                            {
                                this.editor.layout.right_collapsed = !show_attributes;
                            }
                            let mut show_timeline = !this.editor.layout.timeline_collapsed;
                            if automation_checkbox(ui, &mut show_timeline, "Timeline").changed() {
                                this.editor.layout.timeline_collapsed = !show_timeline;
                            }
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "Settings",
                        |ui, this: &mut Self| {
                            if automation_button(ui.button("AI Providers..."), "AI Providers...")
                                .clicked()
                            {
                                this.editor.refresh_providers();
                                this.editor.overlays.providers = true;
                                ui.close();
                            }
                            automation_checkbox(
                                ui,
                                &mut this.editor.layout.hardware_decode,
                                "Hardware Decode",
                            );
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "Help",
                        |ui, this: &mut Self| {
                            ui.label(RichText::new("NLA AI Video Creator").strong());
                            ui.label(
                                RichText::new("egui migration build")
                                    .small()
                                    .color(kit::TEXT_MUTED),
                            );
                            if automation_button(
                                ui.button("Open Harness Docs"),
                                "Open Harness Docs",
                            )
                            .clicked()
                            {
                                this.editor.status = "See docs/DESKTOP_TEST_HARNESS.md".to_string();
                                ui.close();
                            }
                        },
                        self,
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let active_count = self
                            .editor
                            .generation_queue
                            .iter()
                            .filter(|job| {
                                matches!(
                                    job.status,
                                    crate::state::GenerationJobStatus::Queued
                                        | crate::state::GenerationJobStatus::Running
                                )
                            })
                            .count();
                        let attention = active_count > 0;
                        let queue_response = kit::queue_toggle_button(
                            ui,
                            active_count,
                            self.editor.overlays.queue,
                            attention,
                        );
                        self.queue_button_rect = Some(queue_response.rect);
                        if queue_response.clicked() {
                            self.editor.overlays.queue = !self.editor.overlays.queue;
                        }
                    });
                });
            });
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Bottom);
    }

    pub(super) fn project_panel_id(&self, name: &'static str) -> egui::Id {
        egui::Id::new((name, self.editor.project.project_path.clone()))
    }

    pub(super) fn modals(&mut self, ctx: &Context) {
        let startup_open = self.editor.show_startup();
        if startup_open {
            self.startup_modal(ctx);
        }
        if self.editor.overlays.new_project {
            self.new_project_modal(ctx, false);
        }
        if self.editor.overlays.project_settings {
            self.project_settings_modal(ctx);
        }
        if self.editor.overlays.generative_video {
            self.generative_video_modal(ctx);
        }
        if self.editor.overlays.export_video {
            self.export_video_modal(ctx);
        }
        if self.editor.overlays.queue {
            self.queue_panel(ctx);
        }
        if self.editor.overlays.providers {
            self.providers_modal(ctx);
        }
        if self.editor.overlays.api_keys {
            self.api_keys_modal(ctx);
        }
        if self.editor.overlays.asset_lab {
            self.asset_lab_modal(ctx);
        }
        if self.asset_delete_confirmation.is_some() {
            self.asset_delete_confirmation_modal(ctx);
        }
        if self.track_delete_confirmation.is_some() {
            self.track_delete_confirmation_modal(ctx);
        }
        if self.bridge_keyframe_confirmation.is_some() {
            self.bridge_keyframe_confirmation_modal(ctx);
        }
        if self.provider_json_editor_path.is_some() {
            self.provider_json_editor_modal(ctx);
        }
        if self.provider_builder_open {
            self.provider_builder_modal(ctx);
        }
    }

    pub(super) fn status_bar(&mut self, root: &mut Ui) {
        let response = egui::Panel::bottom("status")
            .exact_size(kit::STATUS_BAR_H)
            .frame(kit::chrome_frame())
            .show_inside(root, |ui| {
                ui.horizontal(|ui| {
                    let status_text = if self.editor.project.project_path.is_some() {
                        format!("{} ({})", self.editor.status, self.editor.project_name())
                    } else {
                        self.editor.status.clone()
                    };
                    ui.label(RichText::new(status_text).small().color(kit::TEXT_MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{:.0} fps", self.editor.project.settings.fps))
                                .small()
                                .color(kit::TEXT_MUTED),
                        );
                    });
                });
            });
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Top);
    }
}

impl eframe::App for NlaEguiApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_automation_screenshot_events(&ctx);
        self.poll_automation(&ctx);
        self.keep_automation_responsive(&ctx);
        crate::core::automation::begin_ui_frame();
        self.tick_playback(&ctx);
        self.service_generation_queue(&ctx);
        self.service_export_events(&ctx);
        self.update_preview_texture(&ctx);
        self.service_preview_idle_prefetch(&ctx);
        self.handle_app_keyboard(&ctx);

        self.top_bar(ui);
        // App-wide bars claim root space first; docked editor panels sit above the status bar.
        self.status_bar(ui);
        self.left_panel(ui);
        self.handle_asset_file_drops(&ctx);
        self.right_panel(ui);
        self.timeline_panel(ui);
        self.central_preview(ui);

        self.modals(&ctx);
        self.service_audio_decode_warmup(&ctx);
        self.finish_automation_ui_actions();
    }
}
