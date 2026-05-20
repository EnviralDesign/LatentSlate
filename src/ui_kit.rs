//! Product-specific egui primitives for the editor shell.

use eframe::egui::{
    self, Align, Color32, Context, CornerRadius, FontId, Frame, Layout, Margin, Rect, Response,
    RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};

pub const APP_BG: Color32 = Color32::from_rgb(8, 9, 10);
pub const CHROME: Color32 = Color32::from_rgb(19, 20, 22);
pub const PANEL: Color32 = Color32::from_rgb(17, 18, 20);
pub const PANEL_RAISED: Color32 = Color32::from_rgb(24, 25, 28);
pub const PANEL_SUNKEN: Color32 = Color32::from_rgb(10, 11, 13);
pub const FIELD_BG: Color32 = Color32::from_rgb(7, 8, 10);
pub const BORDER: Color32 = Color32::from_rgb(43, 45, 51);
pub const BORDER_SOFT: Color32 = Color32::from_rgb(31, 33, 38);
pub const BORDER_FOCUS: Color32 = Color32::from_rgb(39, 190, 111);
pub const TEXT: Color32 = Color32::from_rgb(236, 239, 243);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(145, 151, 162);
pub const TEXT_DIM: Color32 = Color32::from_rgb(86, 91, 101);
pub const PRIMARY: Color32 = Color32::from_rgb(31, 178, 91);
pub const PRIMARY_HOVER: Color32 = Color32::from_rgb(39, 204, 108);
pub const VIDEO: Color32 = Color32::from_rgb(38, 204, 122);
pub const IMAGE: Color32 = Color32::from_rgb(42, 184, 129);
pub const AUDIO: Color32 = Color32::from_rgb(79, 149, 232);
pub const MARKER: Color32 = Color32::from_rgb(244, 127, 45);
pub const DANGER: Color32 = Color32::from_rgb(235, 75, 75);
pub const PLAYHEAD: Color32 = Color32::from_rgb(241, 68, 68);
pub const TOP_BAR_H: f32 = 34.0;
pub const STATUS_BAR_H: f32 = 22.0;
pub const PANEL_PAD: i8 = 10;
pub const SECTION_PAD: i8 = 12;
pub const RADIUS: u8 = 7;

pub fn configure_style(ctx: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = PANEL_RAISED;
    visuals.panel_fill = PANEL;
    visuals.faint_bg_color = PANEL;
    visuals.extreme_bg_color = APP_BG;
    visuals.code_bg_color = FIELD_BG;
    visuals.hyperlink_color = PRIMARY_HOVER;
    visuals.selection.bg_fill = Color32::from_rgb(24, 94, 61);
    visuals.selection.stroke = Stroke::new(1.0, BORDER_FOCUS);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.inactive.bg_fill = FIELD_BG;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SOFT);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(34, 36, 41);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.active.bg_fill = Color32::from_rgb(40, 43, 49);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, BORDER_FOCUS);
    visuals.widgets.open.bg_fill = PANEL_RAISED;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = CornerRadius::same(RADIUS);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS);
    ctx.set_visuals(visuals);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.menu_margin = Margin::symmetric(8, 8);
    });
}

pub fn chrome_frame() -> Frame {
    Frame::new()
        .fill(CHROME)
        .stroke(Stroke::new(1.0, BORDER))
        .inner_margin(Margin::symmetric(8, 4))
}

pub fn dock_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .inner_margin(Margin::same(PANEL_PAD))
}

pub fn timeline_frame() -> Frame {
    Frame::new()
        .fill(Color32::from_rgb(13, 14, 16))
        .stroke(Stroke::new(1.0, BORDER))
        .inner_margin(Margin::same(6))
}

pub fn modal_frame() -> Frame {
    Frame::new()
        .fill(PANEL_RAISED)
        .stroke(Stroke::new(1.0, Color32::from_rgb(54, 57, 64)))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(0))
}

pub fn card_frame() -> Frame {
    Frame::new()
        .fill(Color32::from_rgb(20, 21, 24))
        .stroke(Stroke::new(1.0, BORDER_SOFT))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(SECTION_PAD))
}

pub fn sunken_frame() -> Frame {
    Frame::new()
        .fill(FIELD_BG)
        .stroke(Stroke::new(1.0, BORDER_SOFT))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::same(8))
}

pub fn modal_scrim(ctx: &Context, id: &'static str) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new(format!("modal_scrim_{id}")),
    ));
    painter.rect_filled(
        ctx.content_rect(),
        0.0,
        Color32::from_rgba_unmultiplied(0, 0, 0, 168),
    );
}

pub fn modal_header(ui: &mut Ui, title: &str, subtitle: Option<&str>) {
    Frame::new()
        .fill(Color32::from_rgb(31, 32, 36))
        .inner_margin(Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.label(RichText::new(title).color(TEXT).strong().size(17.0));
            if let Some(subtitle) = subtitle {
                ui.add_space(3.0);
                ui.label(RichText::new(subtitle).color(TEXT_MUTED).size(12.0));
            }
        });
}

pub fn modal_body(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(PANEL_RAISED)
        .inner_margin(Margin::symmetric(18, 16))
        .show(ui, add_contents);
}

pub fn panel_header(ui: &mut Ui, label: &str, toggle: Option<&str>, mut on_toggle: impl FnMut()) {
    ui.horizontal(|ui| {
        ui.label(section_label(label));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(toggle) = toggle {
                if icon_button(ui, toggle).clicked() {
                    on_toggle();
                }
            }
        });
    });
}

pub fn section_label(label: &str) -> RichText {
    RichText::new(label.to_ascii_uppercase())
        .size(10.0)
        .color(TEXT_MUTED)
        .strong()
}

pub fn field_label(ui: &mut Ui, label: &str) {
    ui.label(section_label(label));
}

pub fn primary_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    ui.add_sized(
        [width, 36.0],
        egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
            .fill(PRIMARY)
            .stroke(Stroke::new(1.0, Color32::from_rgb(27, 138, 74)))
            .corner_radius(CornerRadius::same(6)),
    )
}

pub fn secondary_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    ui.add_sized(
        [width, 32.0],
        egui::Button::new(RichText::new(label).color(TEXT))
            .fill(Color32::from_rgb(34, 35, 39))
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(5)),
    )
}

pub fn danger_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    ui.add_sized(
        [width, 32.0],
        egui::Button::new(RichText::new(label).color(Color32::WHITE))
            .fill(Color32::from_rgb(112, 28, 32))
            .stroke(Stroke::new(1.0, DANGER))
            .corner_radius(CornerRadius::same(5)),
    )
}

pub fn quiet_button(ui: &mut Ui, label: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(TEXT_MUTED).size(12.0))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(4)),
    )
}

pub fn icon_button(ui: &mut Ui, label: &str) -> Response {
    ui.add_sized(
        [24.0, 22.0],
        egui::Button::new(RichText::new(label).color(TEXT_MUTED).size(11.0))
            .fill(Color32::from_rgb(27, 28, 32))
            .stroke(Stroke::new(1.0, BORDER_SOFT))
            .corner_radius(CornerRadius::same(4)),
    )
}

pub fn media_pill(ui: &mut Ui, label: &str, color: Color32) -> Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(color).size(11.0).strong())
            .fill(Color32::from_rgb(17, 20, 22))
            .stroke(Stroke::new(1.0, color.gamma_multiply(0.55)))
            .corner_radius(CornerRadius::same(5)),
    )
}

pub fn menu_text(label: &str) -> RichText {
    RichText::new(label).color(TEXT).size(12.0)
}

pub fn caption(label: impl Into<String>) -> RichText {
    RichText::new(label.into()).color(TEXT_MUTED).size(11.0)
}

pub fn body(label: impl Into<String>) -> RichText {
    RichText::new(label.into()).color(TEXT).size(12.5)
}

pub fn value(label: impl Into<String>) -> RichText {
    RichText::new(label.into()).color(TEXT).size(12.0).strong()
}

pub fn empty_state(ui: &mut Ui, title: &str, detail: &str) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(title).color(TEXT_MUTED).strong());
            ui.add_space(4.0);
            ui.label(RichText::new(detail).color(TEXT_DIM).size(11.0));
        });
    });
}

pub fn collapsed_rail(ui: &mut Ui, label: &str, accent: Color32) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 140.0), Sense::click());
    let fill = if response.hovered() {
        Color32::from_rgb(26, 28, 32)
    } else {
        PANEL
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter().rect_filled(
        Rect::from_min_size(rect.left_top(), Vec2::new(3.0, rect.height())),
        0.0,
        accent,
    );
    let chars: Vec<char> = label.chars().filter(|ch| !ch.is_whitespace()).collect();
    let step = 12.0;
    let start_y = rect.center().y - (chars.len().saturating_sub(1) as f32 * step / 2.0);
    for (index, ch) in chars.iter().enumerate() {
        ui.painter().text(
            egui::pos2(rect.center().x + 1.0, start_y + index as f32 * step),
            egui::Align2::CENTER_CENTER,
            ch,
            FontId::proportional(10.0),
            TEXT_MUTED,
        );
    }
    response
}

pub fn row_fill(selected: bool, hovered: bool) -> Color32 {
    if selected {
        Color32::from_rgb(25, 74, 54)
    } else if hovered {
        Color32::from_rgb(29, 31, 35)
    } else {
        PANEL_RAISED
    }
}

pub fn draw_accent_row(
    ui: &mut Ui,
    height: f32,
    selected: bool,
    accent: Color32,
    add_contents: impl FnOnce(&mut Ui, Rect),
) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    let fill = row_fill(selected, response.hovered());
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(5),
        Stroke::new(1.0, if selected { accent } else { BORDER_SOFT }),
        StrokeKind::Inside,
    );
    ui.painter().rect_filled(
        Rect::from_min_size(rect.left_top(), Vec2::new(4.0, rect.height())),
        CornerRadius::same(2),
        accent,
    );
    let content_rect = rect.shrink2(Vec2::new(10.0, 5.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    add_contents(&mut child, content_rect);
    response
}
