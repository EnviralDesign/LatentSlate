use super::*;

const CHAT_PANEL_W: f32 = 380.0;

impl LatentSlateApp {
    pub(super) fn chat_panel(&mut self, ctx: &Context) {
        let viewport_id =
            egui::ViewportId::from_hash_of(("chat_window", self.editor.project_session_revision));
        let main_rects =
            ctx.input(|input| input.viewport().outer_rect.zip(input.viewport().inner_rect));
        let saved = self.editor.layout.chat_window;
        let mut builder = egui::ViewportBuilder::default()
            .with_title("LatentSlate Chat")
            .with_min_inner_size([300.0, 240.0])
            .with_resizable(true);
        if !ctx.input(|input| input.raw.viewports.contains_key(&viewport_id)) {
            if let Some(placement) = saved {
                builder = builder
                    .with_position(placement.outer_position)
                    .with_inner_size(placement.inner_size);
            } else if let Some((outer, inner)) = main_rects {
                builder = builder
                    .with_position([outer.right() + 6.0, outer.top()])
                    .with_inner_size([CHAT_PANEL_W, inner.height()]);
            } else {
                builder = builder.with_inner_size([CHAT_PANEL_W, 600.0]);
            }
        }
        ctx.show_viewport_immediate(viewport_id, builder, |ui, _class| {
            let placement = ui.input(|input| {
                let viewport = input.viewport();
                if viewport.minimized == Some(true) {
                    return None;
                }
                let (outer, inner) = viewport.outer_rect.zip(viewport.inner_rect)?;
                Some((outer, inner))
            });
            if let Some((outer, inner)) = placement {
                let mut placement = crate::state::ChatWindowPlacement {
                    outer_position: [outer.min.x, outer.min.y],
                    inner_size: [inner.width(), inner.height()],
                };
                if saved.is_none() {
                    if let Some((main_outer, _)) = main_rects {
                        // Match outer edges using the child window's actual native frame height.
                        let frame_height = outer.height() - inner.height();
                        let size = Vec2::new(
                            CHAT_PANEL_W,
                            (main_outer.height() - frame_height).max(240.0),
                        );
                        let position = Pos2::new(main_outer.right() + 6.0, main_outer.top());
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                        placement.outer_position = [position.x, position.y];
                        placement.inner_size = [size.x, size.y];
                    }
                }
                self.editor.layout.chat_window = Some(placement);
            }
            if ui.input(|input| input.viewport().close_requested()) {
                self.editor.overlays.chat = false;
            }
            egui::Frame::new()
                .fill(kit::PANEL)
                .inner_margin(egui::Margin::same(16))
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    ui.label(kit::section_label("Project assistant"));
                    ui.add_space(20.0);
                    ui.label(RichText::new("Chat is coming soon.").color(kit::TEXT));
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(kit::caption(
                            "This space is reserved for your project assistant.",
                        ))
                        .wrap(),
                    );
                });
        });
    }
}
