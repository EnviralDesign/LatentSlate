use super::*;

const API_PANEL_W: f32 = 380.0;
const API_PANEL_MARGIN: f32 = 8.0;
const API_PANEL_GAP: f32 = 4.0;
const API_SERVICE_CARD_PAD: i8 = 12;
const API_STATUS_BADGE_W: f32 = 64.0;
const API_STATUS_BADGE_H: f32 = 20.0;

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
        let panel_w = API_PANEL_W.min(bounds.width().max(0.0));
        let panel_top = (anchor.bottom() + API_PANEL_GAP).clamp(bounds.top(), bounds.bottom());
        let max_x = (bounds.right() - panel_w).max(bounds.left());
        let panel_pos = Pos2::new(
            (anchor.right() - panel_w).clamp(bounds.left(), max_x),
            panel_top,
        );

        if kit::modal_scrim(ctx, "agent_api").clicked() {
            close_clicked = true;
        }

        egui::Area::new(egui::Id::new("agent_api_popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(panel_pos)
            .show(ctx, |ui| {
                ui.set_width(panel_w);
                kit::modal_frame().show(ui, |ui| {
                    ui.set_width(panel_w);
                    if kit::modal_header_with_close(
                        ui,
                        "Agent API",
                        Some("Loopback automation for local tools"),
                        true,
                    ) {
                        close_clicked = true;
                    }
                    kit::modal_body(ui, |ui| {
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
        let (status, status_color, status_detail) =
            agent_api_status_summary(server_started, active);

        ui.label(kit::section_label("Service"));
        ui.add_space(kit::FIELD_LABEL_GAP);
        let mut enabled = self.agent_api_enabled;
        if agent_api_service_card(ui, &mut enabled, status, status_color, status_detail) {
            self.set_agent_api_enabled(enabled);
        }

        ui.add_space(16.0);
        ui.label(kit::section_label("Connection"));
        ui.add_space(10.0);

        kit::field_label(ui, "Port");
        ui.add_space(kit::FIELD_LABEL_GAP);
        let mut port = i64::from(self.agent_api_port);
        let port_control = ui.add_enabled_ui(!server_started, |ui| {
            kit::integer_step_drag(
                ui,
                &mut port,
                ui.available_width(),
                1,
                Some(1),
                Some(65_535),
            )
        });
        if port_control.inner {
            self.agent_api_port = port.clamp(1, i64::from(u16::MAX)) as u16;
        }
        port_control
            .response
            .on_disabled_hover_text("The port is fixed after the local listener has started.");
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(kit::caption(if server_started {
                "Fixed for this LatentSlate session."
            } else {
                "Used by the loopback listener on 127.0.0.1."
            }))
            .wrap(),
        );

        ui.add_space(12.0);
        kit::field_label(ui, "Endpoint");
        ui.add_space(kit::FIELD_LABEL_GAP);

        let endpoint_port = crate::core::automation::current_port().unwrap_or(self.agent_api_port);
        let endpoint = format!("http://127.0.0.1:{endpoint_port}");
        if agent_api_endpoint_row(ui, &endpoint, active)
            .on_hover_text("Copy the local Agent API endpoint.")
            .clicked()
        {
            ui.ctx().copy_text(endpoint);
            self.editor.status = "Copied Agent API endpoint to clipboard.".to_string();
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);
        let mut close_clicked = false;
        let mut copy_primer_clicked = false;
        kit::equal_width_action_row(
            ui,
            2,
            kit::SECONDARY_BUTTON_H,
            kit::ACTION_GAP,
            |ui, index, button_w| match index {
                0 => {
                    close_clicked = kit::secondary_button(ui, "Close", button_w).clicked();
                }
                _ => {
                    copy_primer_clicked = kit::primary_button(ui, "Copy Primer", button_w)
                        .on_hover_text("Copy a skill-style bootstrap block for another agent.")
                        .clicked();
                }
            },
        );

        if copy_primer_clicked {
            let payload = crate::core::automation::build_agent_bootstrap(
                &self.editor.project,
                &self.editor.selection,
                self.editor.current_time,
                self.editor.project_scoped_provider_entries().len(),
                self.editor.generation_queue.len(),
            );
            ui.ctx().copy_text(payload);
            self.editor.status = "Copied Agent API primer to clipboard.".to_string();
        }
        if close_clicked {
            self.editor.overlays.agent_api = false;
        }
    }
}

fn agent_api_service_card(
    ui: &mut Ui,
    enabled: &mut bool,
    status: &str,
    status_color: Color32,
    detail: &str,
) -> bool {
    let card_width = ui.available_width().max(0.0);
    let content_width = (card_width - f32::from(API_SERVICE_CARD_PAD) * 2.0).max(0.0);
    let mut changed = false;
    egui::Frame::new()
        .fill(kit::FIELD_BG)
        .stroke(Stroke::new(1.0_f32, kit::BORDER_SOFT))
        .corner_radius(kit::field_radius())
        .inner_margin(egui::Margin::same(API_SERVICE_CARD_PAD))
        .show(ui, |ui| {
            ui.set_width(content_width);
            let (row_rect, _) = ui.allocate_exact_size(
                Vec2::new(content_width, API_STATUS_BADGE_H.max(22.0)),
                Sense::hover(),
            );
            let badge_rect = Rect::from_min_size(
                Pos2::new(
                    row_rect.right() - API_STATUS_BADGE_W,
                    row_rect.center().y - API_STATUS_BADGE_H * 0.5,
                ),
                Vec2::new(API_STATUS_BADGE_W, API_STATUS_BADGE_H),
            );
            let toggle_rect = Rect::from_min_max(
                row_rect.left_top(),
                Pos2::new(
                    (badge_rect.left() - 12.0).max(row_rect.left()),
                    row_rect.bottom(),
                ),
            );
            let mut toggle_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(toggle_rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            toggle_ui.shrink_clip_rect(toggle_rect);
            if automation_checkbox(&mut toggle_ui, enabled, "Enabled").changed() {
                changed = true;
            }
            paint_agent_api_status_badge(ui, badge_rect, status, status_color);

            ui.add_space(5.0);
            ui.add(egui::Label::new(kit::caption(detail)).wrap());
        });
    changed
}

fn paint_agent_api_status_badge(ui: &Ui, rect: Rect, label: &str, color: Color32) {
    let [red, green, blue, _] = color.to_srgba_unmultiplied();
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(4),
        Color32::from_rgba_unmultiplied(red, green, blue, 22),
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0_f32, color.gamma_multiply(0.72)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.0),
        color,
    );
}

fn agent_api_endpoint_row(ui: &mut Ui, endpoint: &str, active: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(0.0), kit::FIELD_H),
        Sense::click(),
    );
    let response = crate::core::automation::instrument_response(
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
        "button",
        Some("Copy Agent API endpoint".to_string()),
        true,
        false,
    );
    let fill = if response.is_pointer_button_down_on() {
        kit::FIELD_BG_ACTIVE
    } else if response.hovered() || response.has_focus() {
        kit::FIELD_BG_HOVER
    } else {
        kit::FIELD_BG
    };
    let stroke = if response.has_focus() {
        kit::BORDER_FOCUS
    } else if response.hovered() {
        kit::BORDER
    } else {
        kit::BORDER_SOFT
    };
    ui.painter().rect_filled(rect, kit::field_radius(), fill);
    ui.painter().rect_stroke(
        rect,
        kit::field_radius(),
        Stroke::new(1.0_f32, stroke),
        egui::StrokeKind::Inside,
    );
    let endpoint_color = if active {
        kit::PRIMARY_HOVER
    } else {
        kit::TEXT_MUTED
    };
    ui.painter().text(
        Pos2::new(rect.left() + 9.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        endpoint,
        FontId::proportional(12.0),
        endpoint_color,
    );
    ui.painter().text(
        Pos2::new(rect.right() - 9.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "COPY",
        FontId::proportional(9.5),
        kit::TEXT_DIM,
    );
    response
}

fn agent_api_status_summary(
    server_started: bool,
    active: bool,
) -> (&'static str, Color32, &'static str) {
    if active {
        (
            "ACTIVE",
            kit::PRIMARY_HOVER,
            "Accepting automation requests from this device only.",
        )
    } else if server_started {
        (
            "PAUSED",
            kit::MARKER,
            "The local listener is ready; automation requests are paused.",
        )
    } else {
        (
            "OFF",
            kit::TEXT_DIM,
            "Enable the service to start the local listener.",
        )
    }
}
