use super::*;

impl LatentSlateApp {
    pub(super) fn top_bar(&mut self, root: &mut Ui) {
        self.top_bar_menu_open = false;
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
                                this.request_app_close(ui.ctx());
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
                            ui.separator();
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
                            ui.label(RichText::new("LatentSlate").strong());
                            ui.label(
                                RichText::new("From latent space to timeline.")
                                    .small()
                                    .color(kit::TEXT_MUTED),
                            );
                            ui.label(
                                RichText::new("by Enviral Design")
                                    .small()
                                    .color(kit::TEXT_MUTED),
                            );
                            ui.label(
                                RichText::new("functional alpha")
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

                    if self.editor.project.project_path.is_some() {
                        ui.separator();
                        let project_label = if self.editor.project_dirty {
                            format!("{} *", self.editor.project_name())
                        } else {
                            self.editor.project_name().to_string()
                        };
                        let color = if self.editor.project_dirty {
                            kit::MARKER
                        } else {
                            kit::TEXT_MUTED
                        };
                        ui.label(RichText::new(project_label).small().color(color));
                    }

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
                            if self.editor.overlays.queue {
                                self.editor.overlays.agent_api = false;
                            }
                        }

                        let api_response = kit::api_toggle_button(
                            ui,
                            crate::core::automation::is_active(),
                            self.editor.overlays.agent_api,
                        );
                        self.agent_api_button_rect = Some(api_response.rect);
                        if api_response.clicked() {
                            self.editor.overlays.agent_api = !self.editor.overlays.agent_api;
                            if self.editor.overlays.agent_api {
                                self.editor.overlays.queue = false;
                            }
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
        if self.top_bar_menu_open {
            kit::paint_top_bar_menu_scrim(ctx);
        }

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
        if self.editor.overlays.agent_api {
            self.agent_api_panel(ctx);
        }
        if self.editor.overlays.providers {
            self.providers_modal(ctx);
        }
        if self.editor.overlays.asset_lab {
            self.asset_lab_modal(ctx);
        }
        if self.unsaved_close_confirmation_open {
            self.unsaved_close_confirmation_modal(ctx);
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
                        let suffix = if self.editor.project_dirty { " *" } else { "" };
                        format!(
                            "{} ({}{})",
                            self.editor.status,
                            self.editor.project_name(),
                            suffix
                        )
                    } else {
                        self.editor.status.clone()
                    };
                    let status_color = status_text_color(&self.editor.status);
                    let mut status_rich = RichText::new(status_text).small().color(status_color);
                    if status_color == kit::DANGER {
                        status_rich = status_rich.strong();
                    }
                    ui.label(status_rich);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{:.0} fps", self.editor.project.settings.fps))
                                .small()
                                .color(kit::TEXT_MUTED),
                        );
                        if crate::core::automation::is_active() {
                            let port = crate::core::automation::current_port()
                                .unwrap_or_else(crate::core::automation::default_port);
                            ui.separator();
                            ui.label(
                                RichText::new(format!("Agent API :{port}"))
                                    .small()
                                    .color(kit::PRIMARY),
                            );
                        }
                    });
                });
            });
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Top);
    }
}

impl LatentSlateApp {
    fn update_window_dirty_title(&mut self, ctx: &Context) {
        let dirty = self.editor.project_dirty;
        if self.last_window_title_dirty == Some(dirty) {
            return;
        }
        let title = if dirty {
            "LatentSlate *"
        } else {
            "LatentSlate"
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.to_string()));
        self.last_window_title_dirty = Some(dirty);
    }

    fn handle_viewport_close_request(&mut self, ctx: &Context) {
        let close_requested = ctx.input(|input| input.viewport().close_requested());
        if !close_requested || self.allow_close_without_prompt {
            return;
        }
        if self.editor.project_dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.unsaved_close_confirmation_open = true;
        }
    }

    fn request_app_close(&mut self, ctx: &Context) {
        self.editor.refresh_project_dirty_state();
        if self.editor.project_dirty {
            self.unsaved_close_confirmation_open = true;
        } else {
            self.allow_close_without_prompt = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn unsaved_close_confirmation_modal(&mut self, ctx: &Context) {
        let _ = kit::dismissible_modal_scrim(ctx, "unsaved_close_confirmation", false);
        let mut open = true;
        egui::Window::new("Unsaved Changes")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([460.0, 250.0])
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                kit::modal_header(ui, "Unsaved Changes", Some("Save before closing LatentSlate?"));
                kit::modal_body(ui, |ui| {
                    ui.add(egui::Label::new(kit::caption(
                        "The current project has unsaved changes. Save now, discard them, or cancel closing.",
                    ))
                    .wrap());
                    ui.add_space(18.0);
                    ui.horizontal(|ui| {
                        if kit::secondary_button(ui, "Cancel", 110.0).clicked() {
                            self.unsaved_close_confirmation_open = false;
                        }
                        if kit::danger_button(ui, "Don't Save", 130.0).clicked() {
                            self.allow_close_without_prompt = true;
                            self.unsaved_close_confirmation_open = false;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if kit::primary_button(ui, "Save", 110.0).clicked() {
                            match self.editor.save() {
                                Ok(()) => {
                                    self.allow_close_without_prompt = true;
                                    self.unsaved_close_confirmation_open = false;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                                Err(err) => {
                                    self.editor.status = err;
                                }
                            }
                        }
                    });
                });
            });
        if !open {
            self.unsaved_close_confirmation_open = false;
        }
    }
}

fn status_text_color(status: &str) -> Color32 {
    let lower = status.to_ascii_lowercase();
    if [
        "failed",
        "missing",
        "error",
        "unavailable",
        "not found",
        "cannot",
        "could not",
        "unsupported",
        "offline",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        kit::DANGER
    } else {
        kit::TEXT_MUTED
    }
}

impl eframe::App for LatentSlateApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.editor.refresh_project_dirty_state();
        self.handle_viewport_close_request(&ctx);
        self.update_window_dirty_title(&ctx);
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
        self.editor.refresh_project_dirty_state();
        self.update_window_dirty_title(&ctx);
    }
}
