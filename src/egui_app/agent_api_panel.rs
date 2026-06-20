use super::*;

const API_PANEL_W: f32 = 340.0;
const API_PANEL_MARGIN: f32 = 8.0;
const API_PANEL_GAP: f32 = 4.0;

impl LatentSlateApp {
    pub(super) fn agent_api_panel(&mut self, ctx: &Context) {
        self.sync_agent_api_status();

        let mut close_clicked = false;
        let app_rect = ctx.content_rect();
        let fallback_anchor = Rect::from_min_size(
            Pos2::new(app_rect.right() - 120.0, app_rect.top() + 4.0),
            Vec2::new(kit::TOP_BAR_BUTTON_MIN_W, kit::TOP_BAR_BUTTON_H),
        );
        let anchor = self.agent_api_button_rect.unwrap_or(fallback_anchor);
        let bounds = app_rect.shrink(API_PANEL_MARGIN);
        let panel_top = (anchor.bottom() + API_PANEL_GAP).clamp(bounds.top(), bounds.bottom());
        let max_x = (bounds.right() - API_PANEL_W).max(bounds.left());
        let panel_pos = Pos2::new(
            (anchor.right() - API_PANEL_W).clamp(bounds.left(), max_x),
            panel_top,
        );

        if kit::modal_scrim(ctx, "agent_api").clicked() {
            close_clicked = true;
        }

        egui::Area::new(egui::Id::new("agent_api_popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(panel_pos)
            .show(ctx, |ui| {
                ui.set_width(API_PANEL_W);
                kit::modal_frame().show(ui, |ui| {
                    ui.set_width(API_PANEL_W);
                    if kit::modal_header_with_close(
                        ui,
                        "Agent API",
                        Some("Localhost automation control"),
                        true,
                    ) {
                        close_clicked = true;
                    }
                    kit::modal_body(ui, |ui| {
                        ui.set_width(API_PANEL_W - 36.0);
                        self.agent_api_panel_contents(ui);
                    });
                });
            });

        if close_clicked {
            self.editor.overlays.agent_api = false;
        }
    }

    fn agent_api_panel_contents(&mut self, ui: &mut Ui) {
        let server_started = crate::core::automation::is_enabled();
        let active = crate::core::automation::is_active();
        let (status, status_color) = agent_api_status_label(server_started, active);

        ui.horizontal(|ui| {
            ui.label(RichText::new("Status").small().color(kit::TEXT_MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(status).small().strong().color(status_color));
            });
        });
        ui.add_space(8.0);

        let mut enabled = self.agent_api_enabled;
        if automation_checkbox(ui, &mut enabled, "Enabled").changed() {
            self.set_agent_api_enabled(enabled);
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Port").small().color(kit::TEXT_MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(!server_started, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.agent_api_port)
                            .range(1..=u16::MAX)
                            .speed(1),
                    );
                });
            });
        });

        if server_started {
            let port = crate::core::automation::current_port()
                .unwrap_or_else(crate::core::automation::default_port);
            ui.label(
                RichText::new(format!("http://127.0.0.1:{port}"))
                    .small()
                    .color(if active {
                        kit::PRIMARY_HOVER
                    } else {
                        kit::TEXT_MUTED
                    }),
            );
            if !active {
                ui.label(
                    RichText::new("Server is running; requests are disabled.")
                        .small()
                        .color(kit::TEXT_DIM),
                );
            }
        } else {
            ui.label(
                RichText::new("The listener starts when enabled.")
                    .small()
                    .color(kit::TEXT_DIM),
            );
        }

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if kit::secondary_button(ui, "Copy Primer", 132.0)
                .on_hover_text("Copy a skill-style bootstrap block for another agent.")
                .clicked()
            {
                let payload = crate::core::automation::build_agent_bootstrap(
                    &self.editor.project,
                    &self.editor.selection,
                    self.editor.current_time,
                    self.editor.provider_entries.len(),
                    self.editor.generation_queue.len(),
                );
                ui.ctx().copy_text(payload);
                self.editor.status = "Copied Agent API primer to clipboard.".to_string();
            }
            if automation_button(ui.button("Close"), "Close").clicked() {
                self.editor.overlays.agent_api = false;
            }
        });
    }
}

fn agent_api_status_label(server_started: bool, active: bool) -> (&'static str, Color32) {
    if active {
        ("ACTIVE", kit::PRIMARY_HOVER)
    } else if server_started {
        ("DISABLED", kit::TEXT_MUTED)
    } else {
        ("OFF", kit::TEXT_DIM)
    }
}
