//! Product-specific egui primitives for the editor shell.

use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{
    self, Align, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Frame, Layout, Margin, Pos2, Rect, Response, RichText, Sense, Shadow, Stroke, StrokeKind, Ui,
    Vec2,
};
use egui_extras::{Size, StripBuilder};

pub const APP_BG: Color32 = Color32::from_rgb(8, 9, 10);
pub const CHROME: Color32 = Color32::from_rgb(19, 20, 22);
pub const PANEL: Color32 = Color32::from_rgb(17, 18, 20);
pub const PANEL_RAISED: Color32 = Color32::from_rgb(24, 25, 28);
pub const PANEL_SUNKEN: Color32 = Color32::from_rgb(10, 11, 13);
pub const FIELD_BG: Color32 = Color32::from_rgb(14, 16, 19);
pub const FIELD_BG_HOVER: Color32 = Color32::from_rgb(18, 20, 24);
pub const FIELD_BG_ACTIVE: Color32 = Color32::from_rgb(20, 24, 26);
pub const BORDER: Color32 = Color32::from_rgb(43, 45, 51);
pub const BORDER_SOFT: Color32 = Color32::from_rgb(31, 33, 38);
pub const BORDER_FOCUS: Color32 = Color32::from_rgb(39, 190, 111);
pub const TEXT: Color32 = Color32::from_rgb(236, 239, 243);
pub const TEXT_ON_ACCENT: Color32 = Color32::from_rgb(248, 250, 249);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(145, 151, 162);
pub const TEXT_DIM: Color32 = Color32::from_rgb(86, 91, 101);
pub const PRIMARY: Color32 = Color32::from_rgb(31, 178, 91);
pub const PRIMARY_HOVER: Color32 = Color32::from_rgb(39, 204, 108);
pub const VIDEO: Color32 = Color32::from_rgb(148, 126, 245);
pub const IMAGE: Color32 = Color32::from_rgb(50, 178, 195);
pub const AUDIO: Color32 = Color32::from_rgb(79, 149, 232);
pub const MARKER: Color32 = Color32::from_rgb(244, 127, 45);
pub const DANGER: Color32 = Color32::from_rgb(235, 75, 75);
pub const PLAYHEAD: Color32 = Color32::from_rgb(241, 68, 68);
pub const TOP_BAR_H: f32 = 34.0;
pub const STATUS_BAR_H: f32 = 22.0;
pub const PANEL_PAD: i8 = 10;
pub const SECTION_PAD: i8 = 12;
pub const RADIUS: u8 = 7;
pub const MODAL_RADIUS: u8 = RADIUS;
pub const MODAL_STROKE: Color32 = Color32::from_rgb(54, 57, 64);
pub const MODAL_HEADER_FILL: Color32 = Color32::from_rgb(31, 32, 36);
pub const MODAL_TOP_RADIUS: CornerRadius = CornerRadius {
    nw: MODAL_RADIUS,
    ne: MODAL_RADIUS,
    sw: 0,
    se: 0,
};
pub const MODAL_BOTTOM_RADIUS: CornerRadius = CornerRadius {
    nw: 0,
    ne: 0,
    sw: MODAL_RADIUS,
    se: MODAL_RADIUS,
};
pub const FIELD_H: f32 = 30.0;
pub const TEXT_FIELD_H: f32 = FIELD_H;
pub const VALUE_FIELD_H: f32 = FIELD_H;
pub const FIELD_LABEL_H: f32 = 12.0;
pub const FIELD_TEXT_SIZE: f32 = 12.5;
pub const FIELD_INNER_MARGIN_X: i8 = 8;
pub const FIELD_INNER_MARGIN_Y: i8 = 5;
pub const MULTILINE_FIELD_ROW_H: f32 = 20.0;
pub const DEFAULT_MULTILINE_FIELD_ROWS: usize = 4;
pub const STANDALONE_BUTTON_H: f32 = 32.0;
pub const STANDALONE_BUTTON_RADIUS: u8 = 5;
pub const STANDALONE_BUTTON_TEXT_SIZE: f32 = 12.0;
pub const PRIMARY_BUTTON_H: f32 = STANDALONE_BUTTON_H;
pub const SECONDARY_BUTTON_H: f32 = STANDALONE_BUTTON_H;
pub const ICON_BUTTON_W: f32 = 24.0;
pub const ICON_BUTTON_H: f32 = 22.0;
pub const CLOSE_BUTTON_SIZE: f32 = STANDALONE_BUTTON_H;
pub const CLOSE_BUTTON_RADIUS: u8 = STANDALONE_BUTTON_RADIUS;
pub const CLOSE_BUTTON_ICON_SIZE: f32 = 11.0;
pub const CLOSE_BUTTON_ICON_STROKE: f32 = 1.35;
pub const MODAL_CLOSE_BUTTON_INSET_X: f32 = 12.0;
pub const MODAL_CLOSE_BUTTON_INSET_Y: f32 = 12.0;
pub const MODAL_CLOSE_BUTTON_TITLE_GAP: f32 = 12.0;
pub const MODAL_SCRIM_FILL: Color32 = Color32::from_rgba_unmultiplied_const(7, 9, 13, 184);
pub const MODAL_SCRIM_SOFT_WASH: Color32 = Color32::from_rgba_unmultiplied_const(24, 31, 38, 34);
pub const MODAL_SCRIM_VIGNETTE_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(3, 4, 7, 46);
pub const MODAL_SCRIM_VIGNETTE_BAND: f32 = 150.0;
pub const MODAL_SCRIM_VIGNETTE_STEPS: usize = 5;
pub const MODAL_SHADOW: Shadow = Shadow {
    offset: [0, 18],
    blur: 52,
    spread: 8,
    color: Color32::from_rgba_unmultiplied_const(2, 4, 7, 190),
};
pub const BROWSE_BUTTON_W: f32 = 76.0;
pub const FIELD_COMPOUND_GAP: f32 = 8.0;
pub const FIELD_TEXT_ALIGN: Align = Align::Center;
pub const FIELD_LABEL_GAP: f32 = 6.0;
pub const FIELD_GRID_ROW_CLIP_GUARD: f32 = 3.0;
pub const FORM_ROW_GAP: f32 = 8.0;
pub const ACTION_GAP: f32 = 12.0;
pub const MEDIA_PILL_H: f32 = 24.0;
pub const MEDIA_PILL_MIN_W: f32 = 42.0;
pub const MEDIA_PILL_MIN_GAP: f32 = 4.0;
pub const TIMELINE_TOOL_BUTTON_H: f32 = 20.0;
pub const TIMELINE_TOOL_ICON_W: f32 = 22.0;
pub const TIMELINE_TRANSPORT_BUTTON_W: f32 = 26.0;
pub const TIMELINE_TRANSPORT_BUTTON_H: f32 = TIMELINE_TOOL_BUTTON_H;
pub const TIMELINE_TEXT_BUTTON_RADIUS: u8 = 3;
pub const TOP_BAR_BUTTON_H: f32 = 24.0;
pub const TOP_BAR_BUTTON_MIN_W: f32 = 34.0;
pub const TOP_BAR_BUTTON_PAD_X: f32 = 22.0;
pub const TOP_BAR_BUTTON_RADIUS: u8 = 4;
pub const TOP_BAR_BUTTON_TEXT_SIZE: f32 = 12.0;
pub const POPOVER_BUTTON_H: f32 = 24.0;
pub const POPOVER_BUTTON_RADIUS: u8 = 6;
pub const COLLAPSED_RAIL_W: f32 = 34.0;
pub const COLLAPSED_RAIL_BUTTON_SIZE: f32 = 24.0;

pub fn configure_style(ctx: &Context) {
    configure_fonts(ctx);

    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = PANEL_RAISED;
    visuals.panel_fill = PANEL;
    visuals.faint_bg_color = PANEL;
    visuals.extreme_bg_color = APP_BG;
    visuals.code_bg_color = FIELD_BG;
    visuals.text_edit_bg_color = Some(FIELD_BG);
    visuals.hyperlink_color = PRIMARY_HOVER;
    visuals.selection.bg_fill = Color32::from_rgb(24, 94, 61);
    visuals.selection.stroke = Stroke::new(1.0, BORDER_FOCUS);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.inactive.bg_fill = FIELD_BG;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SOFT);
    visuals.widgets.hovered.bg_fill = FIELD_BG_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.active.bg_fill = FIELD_BG_ACTIVE;
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
        // Scroll bodies in this editor should feel like clipped panes, not faded web views.
        style.spacing.scroll.fade.strength = 0.0;
    });
}

fn configure_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    let mut installed = false;

    installed |= install_font(
        &mut fonts,
        "segoe_ui",
        Path::new(r"C:\Windows\Fonts\segoeui.ttf"),
        FontFamily::Proportional,
        true,
    );
    installed |= install_font(
        &mut fonts,
        "segoe_ui_symbol",
        Path::new(r"C:\Windows\Fonts\seguisym.ttf"),
        FontFamily::Proportional,
        false,
    );
    installed |= install_font(
        &mut fonts,
        "segoe_ui_symbol_mono",
        Path::new(r"C:\Windows\Fonts\seguisym.ttf"),
        FontFamily::Monospace,
        false,
    );

    if installed {
        ctx.set_fonts(fonts);
    }
}

fn install_font(
    fonts: &mut FontDefinitions,
    name: &str,
    path: &Path,
    family: FontFamily,
    primary: bool,
) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    fonts
        .font_data
        .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
    if let Some(family_fonts) = fonts.families.get_mut(&family) {
        let font_name = name.to_owned();
        if primary {
            family_fonts.insert(0, font_name);
        } else {
            family_fonts.push(font_name);
        }
    }
    true
}

pub fn chrome_frame() -> Frame {
    Frame::new()
        .fill(CHROME)
        .inner_margin(Margin::symmetric(8, 4))
}

pub fn dock_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .inner_margin(Margin::same(PANEL_PAD))
}

pub fn collapsed_dock_frame() -> Frame {
    Frame::new().fill(PANEL).inner_margin(Margin::same(0))
}

pub fn timeline_frame() -> Frame {
    Frame::new()
        .fill(Color32::from_rgb(13, 14, 16))
        .inner_margin(Margin::same(6))
}

#[derive(Clone, Copy, Debug)]
pub enum PanelEdge {
    Top,
    Right,
    Bottom,
    Left,
}

pub fn paint_panel_edge(ui: &Ui, rect: Rect, edge: PanelEdge) {
    let stroke = Stroke::new(1.0, BORDER);
    let painter = ui.painter();
    match edge {
        PanelEdge::Top => {
            painter.line_segment([rect.left_top(), rect.right_top()], stroke);
        }
        PanelEdge::Right => {
            painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
        }
        PanelEdge::Bottom => {
            painter.line_segment([rect.left_bottom(), rect.right_bottom()], stroke);
        }
        PanelEdge::Left => {
            painter.line_segment([rect.left_top(), rect.left_bottom()], stroke);
        }
    }
}

pub fn modal_frame() -> Frame {
    Frame::new()
        .fill(PANEL_RAISED)
        .stroke(Stroke::new(1.0, MODAL_STROKE))
        .corner_radius(CornerRadius::same(MODAL_RADIUS))
        .inner_margin(Margin::same(0))
        .shadow(MODAL_SHADOW)
}

pub fn card_frame() -> Frame {
    Frame::new()
        .fill(Color32::from_rgb(20, 21, 24))
        .stroke(Stroke::new(1.0, BORDER_SOFT))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(SECTION_PAD))
}

pub fn fixed_panel_body(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) -> Response {
    let rect = ui.available_rect_before_wrap();
    let (allocated_rect, response) = ui.allocate_exact_size(rect.size(), Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(allocated_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.shrink_clip_rect(allocated_rect);
    child.set_min_size(allocated_rect.size());
    child.set_width(allocated_rect.width());
    child.set_max_width(allocated_rect.width());
    child.set_height(allocated_rect.height());
    add_contents(&mut child);
    crate::core::automation::instrument_response(response, "panel", None, false, false)
}

pub fn card_panel(ui: &mut Ui, height: f32, add_contents: impl FnOnce(&mut Ui)) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(RADIUS),
        Color32::from_rgb(20, 21, 24),
    );
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(RADIUS),
        Stroke::new(1.0, BORDER_SOFT),
        StrokeKind::Inside,
    );

    let content_rect = rect.shrink(SECTION_PAD as f32);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_min_size(content_rect.size());
    child.shrink_clip_rect(content_rect);
    child.set_width(content_rect.width());
    child.set_max_width(content_rect.width());
    add_contents(&mut child);
    crate::core::automation::instrument_response(response, "panel", None, false, false)
}

pub fn stack_card_panel(ui: &mut Ui, height: f32, add_contents: impl FnOnce(&mut Ui)) -> Response {
    card_panel(ui, height, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        add_contents(ui);
    })
}

/// Lays out a body in the remaining height above an exact bottom footer.
pub fn body_with_footer(
    ui: &mut Ui,
    min_body_height: f32,
    footer_height: f32,
    add_body: impl FnOnce(&mut Ui),
    add_footer: impl FnOnce(&mut Ui),
) {
    StripBuilder::new(ui)
        .clip(true)
        .size(Size::remainder().at_least(min_body_height))
        .size(Size::exact(footer_height))
        .vertical(|mut strip| {
            strip.cell(add_body);
            strip.cell(add_footer);
        });
}

pub fn equal_width_action_row(
    ui: &mut Ui,
    count: usize,
    height: f32,
    gap: f32,
    mut add_cell: impl FnMut(&mut Ui, usize, f32),
) {
    if count == 0 {
        return;
    }

    let row_width = ui.available_width().max(0.0);
    let gaps = count.saturating_sub(1) as f32;
    let gap = if gaps > 0.0 {
        gap.max(0.0).min(row_width / gaps)
    } else {
        0.0
    };
    let cell_width = ((row_width - gap * gaps) / count as f32).max(0.0);
    let (row_rect, _) = ui.allocate_exact_size(Vec2::new(row_width, height), Sense::hover());

    let mut x = row_rect.left();
    for index in 0..count {
        let rect = Rect::from_min_size(Pos2::new(x, row_rect.top()), Vec2::new(cell_width, height));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
        );
        child.shrink_clip_rect(rect);
        child.set_width(cell_width);
        child.set_max_width(cell_width);
        add_cell(&mut child, index, cell_width);
        x += cell_width + gap;
    }
}

pub fn field_grid_row(ui: &mut Ui, weights: &[f32], add_cell: impl FnMut(&mut Ui, usize)) {
    field_grid_row_with_height(
        ui,
        weights,
        labeled_field_height(FIELD_H),
        FORM_ROW_GAP,
        add_cell,
    );
}

pub fn field_grid_row_with_height(
    ui: &mut Ui,
    weights: &[f32],
    height: f32,
    gap: f32,
    mut add_cell: impl FnMut(&mut Ui, usize),
) {
    if weights.is_empty() {
        return;
    }

    let width = ui.available_width().max(0.0);
    let gap = gap.max(0.0).min(width);
    let total_gap = gap * weights.len().saturating_sub(1) as f32;
    let cell_area_w = (width - total_gap).max(0.0);
    let total_weight = weights
        .iter()
        .copied()
        .map(|weight| weight.max(0.0))
        .sum::<f32>()
        .max(1.0);
    let (row_rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let mut left = row_rect.left();

    for (index, weight) in weights.iter().copied().enumerate() {
        let is_last = index + 1 == weights.len();
        let cell_w = if is_last {
            (row_rect.right() - left).max(0.0)
        } else {
            cell_area_w * weight.max(0.0) / total_weight
        };
        let cell_rect = Rect::from_min_size(
            Pos2::new(left, row_rect.top()),
            Vec2::new(cell_w, row_rect.height()),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(cell_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        child.shrink_clip_rect(cell_rect);
        child.set_width(cell_rect.width());
        child.set_max_width(cell_rect.width());
        child.set_height(cell_rect.height());
        add_cell(&mut child, index);
        left = cell_rect.right() + gap;
    }
}

pub fn scroll_body(ui: &mut Ui, add_body: impl FnOnce(&mut Ui)) {
    let id_salt = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    clipped_scroll_body(ui, id_salt, add_body);
}

pub fn clipped_scroll_body(ui: &mut Ui, id_salt: impl Hash, add_body: impl FnOnce(&mut Ui)) {
    let available_rect = ui.available_rect_before_wrap();
    let (viewport_rect, _) = ui.allocate_exact_size(available_rect.size(), Sense::hover());
    let clip_rect = ui.clip_rect().intersect(viewport_rect);
    let viewport_width = viewport_rect.width().max(0.0);
    let viewport_height = viewport_rect.height().max(0.0);

    let mut viewport_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(viewport_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    viewport_ui.shrink_clip_rect(clip_rect);
    viewport_ui.set_width(viewport_width);
    viewport_ui.set_height(viewport_height);
    viewport_ui.spacing_mut().scroll.fade.strength = 0.0;

    egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .max_width(viewport_width)
        .max_height(viewport_height)
        .scroll_bar_rect(clip_rect)
        .auto_shrink([false, false])
        .show_viewport(&mut viewport_ui, |ui, _viewport| {
            let content_width = ui.max_rect().width().min(viewport_width).max(0.0);
            ui.shrink_clip_rect(clip_rect);
            ui.set_width(content_width);
            ui.set_min_width(content_width);
            ui.set_max_width(content_width);
            add_body(ui);
        });
}

/// Controls how a native browse dialog chooses its starting directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowseDirectoryMemory {
    /// Use `initial_dir` every time the dialog opens.
    ResetToInitial,
    /// Start from the last folder selected by this widget, falling back to `initial_dir`.
    RememberLastDirectory,
}

/// Shared options for file/folder browse fields and native browse dialogs.
#[derive(Clone, Copy, Debug)]
pub struct BrowsePathOptions<'a> {
    pub button_label: &'a str,
    pub initial_dir: Option<&'a Path>,
    pub directory_memory: BrowseDirectoryMemory,
    id_salt: Option<egui::Id>,
}

#[allow(dead_code)]
impl<'a> BrowsePathOptions<'a> {
    /// Creates browse options with a "Browse" button and no initial directory.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the button label shown at the end of the compound field.
    pub fn button_label(mut self, label: &'a str) -> Self {
        self.button_label = label;
        self
    }

    /// Sets the fallback directory for the native dialog.
    pub fn initial_dir(mut self, dir: &'a Path) -> Self {
        self.initial_dir = Some(dir);
        self
    }

    /// Makes the dialog start at the last selected folder after the first selection.
    pub fn remember_last_dir(mut self) -> Self {
        self.directory_memory = BrowseDirectoryMemory::RememberLastDirectory;
        self
    }

    /// Makes the dialog restart from `initial_dir` every time.
    pub fn reset_to_initial_dir(mut self) -> Self {
        self.directory_memory = BrowseDirectoryMemory::ResetToInitial;
        self
    }

    /// Provides a stable identity for the widget's remembered directory.
    pub fn id_salt(mut self, id_salt: impl Hash) -> Self {
        self.id_salt = Some(egui::Id::new(id_salt));
        self
    }
}

impl<'a> Default for BrowsePathOptions<'a> {
    fn default() -> Self {
        Self {
            button_label: "Browse",
            initial_dir: None,
            directory_memory: BrowseDirectoryMemory::ResetToInitial,
            id_salt: None,
        }
    }
}

/// One file-extension filter for a native file dialog.
#[derive(Clone, Copy, Debug)]
pub struct FileExtensionFilter<'a> {
    pub name: &'a str,
    /// Extensions without leading dots, e.g. `&["mp4", "mov"]`.
    pub extensions: &'a [&'a str],
}

/// Options for a native file browse field/dialog.
#[derive(Clone, Copy, Debug)]
pub struct BrowseFileOptions<'a> {
    pub path: BrowsePathOptions<'a>,
    pub filters: &'a [FileExtensionFilter<'a>],
}

#[allow(dead_code)]
impl<'a> BrowseFileOptions<'a> {
    /// Creates file browse options with no extension filters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the button label shown at the end of the compound field.
    pub fn button_label(mut self, label: &'a str) -> Self {
        self.path = self.path.button_label(label);
        self
    }

    /// Sets the fallback directory for the native dialog.
    pub fn initial_dir(mut self, dir: &'a Path) -> Self {
        self.path = self.path.initial_dir(dir);
        self
    }

    /// Makes the dialog start at the last selected file's folder after the first selection.
    pub fn remember_last_dir(mut self) -> Self {
        self.path = self.path.remember_last_dir();
        self
    }

    /// Makes the dialog restart from `initial_dir` every time.
    pub fn reset_to_initial_dir(mut self) -> Self {
        self.path = self.path.reset_to_initial_dir();
        self
    }

    /// Provides a stable identity for the widget's remembered directory.
    pub fn id_salt(mut self, id_salt: impl Hash) -> Self {
        self.path = self.path.id_salt(id_salt);
        self
    }

    /// Adds native dialog extension filters.
    pub fn filters(mut self, filters: &'a [FileExtensionFilter<'a>]) -> Self {
        self.filters = filters;
        self
    }
}

impl<'a> Default for BrowseFileOptions<'a> {
    fn default() -> Self {
        Self {
            path: BrowsePathOptions::default(),
            filters: &[],
        }
    }
}

pub fn browse_value_row(ui: &mut Ui, value: impl Into<String>, button_label: &str) -> Response {
    let value = value.into();
    let row_w = ui.available_width();
    let (row_rect, _) = ui.allocate_exact_size(Vec2::new(row_w, VALUE_FIELD_H), Sense::hover());
    let mut row_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(row_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    row_ui.set_min_size(row_rect.size());
    row_ui.spacing_mut().item_spacing.x = FIELD_COMPOUND_GAP;

    let mut button_response = None;
    StripBuilder::new(&mut row_ui)
        .clip(true)
        .size(Size::remainder().at_least(90.0))
        .size(Size::exact(BROWSE_BUTTON_W))
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                readonly_value_box(ui, value, Vec2::new(ui.available_width(), VALUE_FIELD_H));
            });
            strip.cell(|ui| {
                button_response = Some(field_button(ui, button_label, ui.available_width()));
            });
        });
    button_response.expect("browse row always creates a button")
}

/// Shows a folder browse field and opens a native folder dialog when its button is clicked.
#[allow(dead_code)]
pub fn browse_folder_field(
    ui: &mut Ui,
    value: impl Into<String>,
    options: BrowsePathOptions<'_>,
) -> Option<PathBuf> {
    browse_folder_field_with_id(ui, value, options, ("folder_field", options.button_label))
}

/// Shows a file browse field and opens a native file dialog when its button is clicked.
#[allow(dead_code)]
pub fn browse_file_field(
    ui: &mut Ui,
    value: impl Into<String>,
    options: BrowseFileOptions<'_>,
) -> Option<PathBuf> {
    browse_file_field_with_id(
        ui,
        value,
        options,
        ("file_field", options.path.button_label),
    )
}

/// Opens a native folder dialog using the shared browse dialog directory policy.
pub fn pick_folder_dialog(ui: &Ui, options: BrowsePathOptions<'_>) -> Option<PathBuf> {
    pick_folder_dialog_with_id(ui, options, ("folder_dialog", options.button_label))
}

/// Opens a native file dialog using the shared browse dialog directory policy.
pub fn pick_file_dialog(ui: &Ui, options: BrowseFileOptions<'_>) -> Option<PathBuf> {
    pick_file_dialog_with_id(ui, options, ("file_dialog", options.path.button_label))
}

pub fn labeled_field_height(control_height: f32) -> f32 {
    FIELD_LABEL_H + FIELD_LABEL_GAP + control_height + FIELD_GRID_ROW_CLIP_GUARD
}

#[derive(Clone, Copy, Debug)]
pub struct FieldPairLayout {
    pub gap: f32,
    pub height: f32,
    pub min_column_width: f32,
}

impl Default for FieldPairLayout {
    fn default() -> Self {
        Self {
            gap: FORM_ROW_GAP,
            height: FIELD_H,
            min_column_width: 0.0,
        }
    }
}

impl FieldPairLayout {
    pub fn min_column_width(mut self, width: f32) -> Self {
        self.min_column_width = width.max(0.0);
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height.max(0.0);
        self
    }
}

pub fn paired_field_rects(ui: &mut Ui, options: FieldPairLayout) -> (Rect, Rect) {
    let available = ui.available_width().max(0.0);
    let gap = options.gap.min(available);
    let available_columns = (available - gap).max(0.0);
    let natural_col_w = available_columns * 0.5;
    let col_w =
        if options.min_column_width > 0.0 && available_columns >= options.min_column_width * 2.0 {
            natural_col_w.max(options.min_column_width)
        } else {
            natural_col_w
        };
    let total_w = (col_w * 2.0 + gap).min(available);
    let (row_rect, _) = ui.allocate_exact_size(Vec2::new(total_w, options.height), Sense::hover());
    let left = Rect::from_min_size(row_rect.min, Vec2::new(col_w, options.height));
    let right = Rect::from_min_size(
        Pos2::new(row_rect.left() + col_w + gap, row_rect.top()),
        Vec2::new((row_rect.width() - col_w - gap).max(0.0), options.height),
    );
    (left, right)
}

pub fn labeled_text_field(ui: &mut Ui, label: &str, value: &mut String) -> Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = FIELD_LABEL_GAP;
        field_label(ui, label);
        singleline_text_field_labeled(ui, value, ui.available_width(), Some(label.to_string()))
    })
    .inner
}

pub fn labeled_password_field(ui: &mut Ui, label: &str, value: &mut String) -> Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = FIELD_LABEL_GAP;
        field_label(ui, label);
        password_text_field(ui, value, ui.available_width(), Some(label.to_string()))
    })
    .inner
}

pub fn combo_field<R>(
    ui: &mut Ui,
    id_salt: impl Hash,
    selected_text: impl Into<String>,
    width: f32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Response {
    let selected_text = selected_text.into();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, FIELD_H), Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_min_size(rect.size());
    child.shrink_clip_rect(rect);
    configure_field_widget_style(&mut child, rect.width());

    let combo_salt = egui::Id::new(("combo_field", id_salt));
    let combo_button_id = child.make_persistent_id(combo_salt);
    if crate::core::automation::consume_pending_click_for_egui_id(response.id)
        || crate::core::automation::consume_pending_click_for_egui_id(combo_button_id)
    {
        egui::Popup::open_id(child.ctx(), combo_button_id.with("popup"));
    }
    let inner = egui::ComboBox::from_id_salt(combo_salt)
        .selected_text(
            RichText::new(selected_text.clone())
                .color(TEXT)
                .size(FIELD_TEXT_SIZE),
        )
        .width(rect.width())
        .show_ui(&mut child, add_contents);

    let combo_response = crate::core::automation::instrument_response(
        inner.response,
        "combo",
        Some(selected_text),
        true,
        false,
    );
    response.union(combo_response)
}

pub fn labeled_combo_field<R>(
    ui: &mut Ui,
    label: &str,
    id_salt: impl Hash,
    selected_text: impl Into<String>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = FIELD_LABEL_GAP;
        field_label(ui, label);
        combo_field(
            ui,
            id_salt,
            selected_text,
            ui.available_width(),
            add_contents,
        )
    })
    .inner
}

/// Shows a labeled folder browse field.
pub fn labeled_browse_folder_field(
    ui: &mut Ui,
    label: &str,
    value: impl Into<String>,
    options: BrowsePathOptions<'_>,
) -> Option<PathBuf> {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = FIELD_LABEL_GAP;
        field_label(ui, label);
        browse_folder_field_with_id(ui, value, options, ("folder_field", label))
    })
    .inner
}

/// Shows a labeled file browse field.
#[allow(dead_code)]
pub fn labeled_browse_file_field(
    ui: &mut Ui,
    label: &str,
    value: impl Into<String>,
    options: BrowseFileOptions<'_>,
) -> Option<PathBuf> {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = FIELD_LABEL_GAP;
        field_label(ui, label);
        browse_file_field_with_id(ui, value, options, ("file_field", label))
    })
    .inner
}

/// Shows a labeled editable save-file field.
pub fn labeled_save_file_field(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    options: BrowseFileOptions<'_>,
) -> Option<PathBuf> {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = FIELD_LABEL_GAP;
        field_label(ui, label);
        save_file_field_with_id(ui, value, options, ("save_file_field", label), label)
    })
    .inner
}

#[allow(dead_code)]
pub fn labeled_browse_value_row(
    ui: &mut Ui,
    label: &str,
    value: impl Into<String>,
    button_label: &str,
) -> Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = FIELD_LABEL_GAP;
        field_label(ui, label);
        browse_value_row(ui, value, button_label)
    })
    .inner
}

fn browse_folder_field_with_id(
    ui: &mut Ui,
    value: impl Into<String>,
    options: BrowsePathOptions<'_>,
    fallback_id: impl Hash,
) -> Option<PathBuf> {
    let response = browse_value_row(ui, value, options.button_label);
    if response.clicked() {
        pick_folder_dialog_with_id(ui, options, fallback_id)
    } else {
        None
    }
}

#[allow(dead_code)]
fn browse_file_field_with_id(
    ui: &mut Ui,
    value: impl Into<String>,
    options: BrowseFileOptions<'_>,
    fallback_id: impl Hash,
) -> Option<PathBuf> {
    let response = browse_value_row(ui, value, options.path.button_label);
    if response.clicked() {
        pick_file_dialog_with_id(ui, options, fallback_id)
    } else {
        None
    }
}

fn pick_folder_dialog_with_id(
    ui: &Ui,
    options: BrowsePathOptions<'_>,
    fallback_id: impl Hash,
) -> Option<PathBuf> {
    let memory_id = browse_dialog_memory_id(ui, options, "folder", fallback_id);
    let start_dir = browse_start_dir(ui, memory_id, options);
    let picked = with_dialog_directory(rfd::FileDialog::new(), start_dir.as_deref()).pick_folder();
    if let Some(folder) = picked.as_ref() {
        remember_dialog_dir(ui, memory_id, options, folder);
    }
    picked
}

fn pick_file_dialog_with_id(
    ui: &Ui,
    options: BrowseFileOptions<'_>,
    fallback_id: impl Hash,
) -> Option<PathBuf> {
    let memory_id = browse_dialog_memory_id(ui, options.path, "file", fallback_id);
    let start_dir = browse_start_dir(ui, memory_id, options.path);
    let mut dialog = with_dialog_directory(rfd::FileDialog::new(), start_dir.as_deref());
    for filter in options
        .filters
        .iter()
        .filter(|filter| !filter.extensions.is_empty())
    {
        dialog = dialog.add_filter(filter.name, filter.extensions);
    }
    let picked = dialog.pick_file();
    if let Some(file) = picked.as_ref() {
        let remembered_dir = file.parent().unwrap_or(file);
        remember_dialog_dir(ui, memory_id, options.path, remembered_dir);
    }
    picked
}

fn save_file_dialog_with_id(
    ui: &Ui,
    options: BrowseFileOptions<'_>,
    fallback_id: impl Hash,
) -> Option<PathBuf> {
    let memory_id = browse_dialog_memory_id(ui, options.path, "save_file", fallback_id);
    let start_dir = browse_start_dir(ui, memory_id, options.path);
    let mut dialog = with_dialog_directory(rfd::FileDialog::new(), start_dir.as_deref());
    for filter in options
        .filters
        .iter()
        .filter(|filter| !filter.extensions.is_empty())
    {
        dialog = dialog.add_filter(filter.name, filter.extensions);
    }
    let picked = dialog.save_file();
    if let Some(file) = picked.as_ref() {
        let remembered_dir = file.parent().unwrap_or(file);
        remember_dialog_dir(ui, memory_id, options.path, remembered_dir);
    }
    picked
}

fn save_file_field_with_id(
    ui: &mut Ui,
    value: &mut String,
    options: BrowseFileOptions<'_>,
    fallback_id: impl Hash,
    automation_label: &str,
) -> Option<PathBuf> {
    let row_w = ui.available_width();
    let (row_rect, _) = ui.allocate_exact_size(Vec2::new(row_w, VALUE_FIELD_H), Sense::hover());
    let mut row_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(row_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    row_ui.set_min_size(row_rect.size());
    row_ui.spacing_mut().item_spacing.x = FIELD_COMPOUND_GAP;

    let mut clicked = false;
    StripBuilder::new(&mut row_ui)
        .clip(true)
        .size(Size::remainder().at_least(90.0))
        .size(Size::exact(BROWSE_BUTTON_W))
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                singleline_text_field_labeled(
                    ui,
                    value,
                    ui.available_width(),
                    Some(automation_label.to_string()),
                );
            });
            strip.cell(|ui| {
                clicked =
                    field_button(ui, options.path.button_label, ui.available_width()).clicked();
            });
        });

    if clicked {
        save_file_dialog_with_id(ui, options, fallback_id)
    } else {
        None
    }
}

fn with_dialog_directory(dialog: rfd::FileDialog, dir: Option<&Path>) -> rfd::FileDialog {
    if let Some(dir) = dir {
        dialog.set_directory(dir)
    } else {
        dialog
    }
}

fn browse_dialog_memory_id(
    ui: &Ui,
    options: BrowsePathOptions<'_>,
    kind: &'static str,
    fallback_id: impl Hash,
) -> egui::Id {
    options
        .id_salt
        .unwrap_or_else(|| ui.make_persistent_id(("browse_dialog", kind, fallback_id)))
        .with("last_dir")
}

fn browse_start_dir(
    ui: &Ui,
    memory_id: egui::Id,
    options: BrowsePathOptions<'_>,
) -> Option<PathBuf> {
    if options.directory_memory == BrowseDirectoryMemory::RememberLastDirectory {
        let remembered = ui
            .ctx()
            .data_mut(|data| data.get_temp::<PathBuf>(memory_id));
        if remembered.is_some() {
            return remembered;
        }
    }
    options.initial_dir.map(Path::to_path_buf)
}

fn remember_dialog_dir(ui: &Ui, memory_id: egui::Id, options: BrowsePathOptions<'_>, dir: &Path) {
    if options.directory_memory == BrowseDirectoryMemory::RememberLastDirectory {
        ui.ctx()
            .data_mut(|data| data.insert_temp(memory_id, dir.to_path_buf()));
    }
}

pub fn sunken_frame() -> Frame {
    Frame::new()
        .fill(FIELD_BG)
        .stroke(Stroke::new(1.0, BORDER_SOFT))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::same(8))
}

pub fn readonly_value_box(ui: &mut Ui, value: impl Into<String>, size: Vec2) -> Response {
    let value = value.into();
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_min_size(rect.size());
    child.shrink_clip_rect(rect);
    configure_field_widget_style(&mut child, rect.width());

    let field_id = child.next_auto_id();
    child.skip_ahead_auto_ids(1);

    ui.painter().rect_filled(rect, field_radius(), FIELD_BG);
    let mut value_view = value.as_str();
    let mut output = egui::TextEdit::singleline(&mut value_view)
        .id(field_id)
        .desired_width(rect.width())
        .min_size(rect.size())
        .horizontal_align(FIELD_TEXT_ALIGN)
        .vertical_align(Align::Center)
        .text_color(TEXT)
        .font(FontId::proportional(FIELD_TEXT_SIZE))
        .frame(field_text_frame())
        .show(&mut child);
    select_all_on_focus(&mut output, &value);

    ui.painter().rect_stroke(
        rect,
        field_radius(),
        field_stroke(&output),
        StrokeKind::Inside,
    );
    crate::core::automation::instrument_response(
        output.response.response.on_hover_text(value.clone()),
        "readonly_field",
        Some(value),
        false,
        false,
    )
}

fn select_all_on_focus(output: &mut egui::text_edit::TextEditOutput, text: &str) {
    let response = &output.response.response;
    if response.gained_focus() {
        let range = egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(text.chars().count()),
        );
        output.state.cursor.set_char_range(Some(range));
        output.state.clone().store(&response.ctx, response.id);
    }
}

fn field_text_edit(ui: &mut Ui, value: &mut String, rect: Rect) -> egui::text_edit::TextEditOutput {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_min_size(rect.size());
    child.shrink_clip_rect(rect);
    configure_field_widget_style(&mut child, rect.width());

    let field_id = child.next_auto_id();
    child.skip_ahead_auto_ids(1);
    let text_align = editable_field_text_align(&child, field_id);

    ui.painter().rect_filled(rect, field_radius(), FIELD_BG);
    egui::TextEdit::singleline(value)
        .id(field_id)
        .desired_width(rect.width())
        .min_size(rect.size())
        .horizontal_align(text_align)
        .vertical_align(Align::Center)
        .text_color(TEXT)
        .font(FontId::proportional(FIELD_TEXT_SIZE))
        .frame(field_text_frame())
        .show(&mut child)
}

pub fn singleline_text_field(ui: &mut Ui, value: &mut String, width: f32) -> Response {
    singleline_text_field_labeled(ui, value, width, None)
}

fn password_text_field(
    ui: &mut Ui,
    value: &mut String,
    width: f32,
    automation_label: Option<String>,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, TEXT_FIELD_H), Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_min_size(rect.size());
    child.shrink_clip_rect(rect);
    configure_field_widget_style(&mut child, rect.width());

    let field_id = child.next_auto_id();
    child.skip_ahead_auto_ids(1);
    let text_align = editable_field_text_align(&child, field_id);

    ui.painter().rect_filled(rect, field_radius(), FIELD_BG);
    let mut output = egui::TextEdit::singleline(value)
        .id(field_id)
        .password(true)
        .desired_width(rect.width())
        .min_size(rect.size())
        .horizontal_align(text_align)
        .vertical_align(Align::Center)
        .text_color(TEXT)
        .font(FontId::proportional(FIELD_TEXT_SIZE))
        .frame(field_text_frame())
        .show(&mut child);
    let selected_text = value.clone();
    select_all_on_focus(&mut output, &selected_text);
    let mut response = output.response.response.clone();
    crate::core::automation::apply_pending_text(&mut response, value);
    ui.painter().rect_stroke(
        rect,
        field_radius(),
        field_stroke(&output),
        StrokeKind::Inside,
    );
    crate::core::automation::instrument_response(
        response,
        "password_field",
        automation_label,
        true,
        true,
    )
}

fn singleline_text_field_labeled(
    ui: &mut Ui,
    value: &mut String,
    width: f32,
    automation_label: Option<String>,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, TEXT_FIELD_H), Sense::hover());
    let mut output = field_text_edit(ui, value, rect);
    let selected_text = value.clone();
    select_all_on_focus(&mut output, &selected_text);
    let mut response = output.response.response.clone();
    crate::core::automation::apply_pending_text(&mut response, value);
    ui.painter().rect_stroke(
        rect,
        field_radius(),
        field_stroke(&output),
        StrokeKind::Inside,
    );
    crate::core::automation::instrument_response(
        response,
        "text_field",
        automation_label,
        true,
        true,
    )
}

fn editable_field_text_align(ui: &Ui, field_id: egui::Id) -> Align {
    if ui.memory(|mem| mem.has_focus(field_id)) {
        Align::Min
    } else {
        FIELD_TEXT_ALIGN
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MultilineTextFieldOptions {
    pub rows: usize,
}

impl Default for MultilineTextFieldOptions {
    fn default() -> Self {
        Self {
            rows: DEFAULT_MULTILINE_FIELD_ROWS,
        }
    }
}

impl MultilineTextFieldOptions {
    pub fn rows(rows: usize) -> Self {
        Self { rows: rows.max(1) }
    }
}

pub fn multiline_text_field(
    ui: &mut Ui,
    value: &mut String,
    width: f32,
    options: MultilineTextFieldOptions,
) -> Response {
    let rows = options.rows.max(1);
    let height = multiline_text_field_height(rows);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let field_id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_min_size(rect.size());
    child.shrink_clip_rect(rect);
    child.set_width(rect.width());
    child.set_height(rect.height());
    child.spacing_mut().scroll.fade.strength = 0.0;
    configure_field_widget_style(&mut child, rect.width());

    ui.painter().rect_filled(rect, field_radius(), FIELD_BG);
    let mut text_response: Option<Response> = None;
    egui::ScrollArea::vertical()
        .id_salt(field_id.with("scroll"))
        .max_width(rect.width())
        .max_height(rect.height())
        .scroll_bar_rect(rect.shrink(1.0))
        .auto_shrink([false, false])
        .show_viewport(&mut child, |ui, _viewport| {
            ui.shrink_clip_rect(rect);
            ui.set_width(rect.width());
            ui.set_min_width(rect.width());
            ui.set_max_width(rect.width());
            let output = egui::TextEdit::multiline(value)
                .id(field_id)
                .desired_width(rect.width())
                .desired_rows(rows)
                .lock_focus(true)
                .text_color(TEXT)
                .font(FontId::proportional(FIELD_TEXT_SIZE))
                .frame(field_text_frame())
                .show(ui);
            text_response = Some(output.response.response);
        });

    let mut response = text_response.unwrap_or_else(|| {
        ui.interact(
            rect,
            field_id.with("multiline_fallback"),
            Sense::click_and_drag(),
        )
    });
    crate::core::automation::apply_pending_text(&mut response, value);
    let stroke = if response.has_focus() {
        Stroke::new(1.0, BORDER_FOCUS)
    } else if response.hovered() {
        Stroke::new(1.0, BORDER)
    } else {
        Stroke::new(1.0, BORDER_SOFT)
    };
    ui.painter()
        .rect_stroke(rect, field_radius(), stroke, StrokeKind::Inside);
    crate::core::automation::instrument_response(response, "multiline_text_field", None, true, true)
}

pub fn multiline_text_field_height(rows: usize) -> f32 {
    rows.max(1) as f32 * MULTILINE_FIELD_ROW_H + FIELD_INNER_MARGIN_Y as f32 * 2.0
}

pub fn code_editor_field(ui: &mut Ui, value: &mut String, id_salt: impl Hash) -> Response {
    let base_id = ui.make_persistent_id(("code_editor_field", id_salt));
    let available = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(available, Sense::hover());
    let inner_rect = rect.shrink(8.0);
    let clip_rect = ui.clip_rect().intersect(inner_rect);

    ui.painter().rect_filled(rect, field_radius(), FIELD_BG);

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_min_size(inner_rect.size());
    child.shrink_clip_rect(clip_rect);
    child.set_width(inner_rect.width());
    child.set_height(inner_rect.height());
    child.spacing_mut().scroll.fade.strength = 0.0;
    configure_field_widget_style(&mut child, inner_rect.width());

    let mut text_response: Option<Response> = None;
    egui::ScrollArea::vertical()
        .id_salt(base_id.with("scroll"))
        .max_width(inner_rect.width())
        .max_height(inner_rect.height())
        .scroll_bar_rect(clip_rect)
        .auto_shrink([false, false])
        .show_viewport(&mut child, |ui, _viewport| {
            ui.shrink_clip_rect(clip_rect);
            ui.set_width(inner_rect.width());
            ui.set_min_width(inner_rect.width());
            ui.set_max_width(inner_rect.width());
            let output = egui::TextEdit::multiline(value)
                .id(base_id.with("text"))
                .desired_width(inner_rect.width())
                .code_editor()
                .font(FontId::monospace(12.0))
                .text_color(TEXT)
                .frame(Frame::new().fill(Color32::TRANSPARENT))
                .show(ui);
            text_response = Some(output.response.response);
        });

    let mut response = if let Some(text_response) = text_response {
        response.union(text_response)
    } else {
        response
    };
    crate::core::automation::apply_pending_text(&mut response, value);
    let stroke = if response.has_focus() {
        Stroke::new(1.0, BORDER_FOCUS)
    } else if response.hovered() {
        Stroke::new(1.0, BORDER)
    } else {
        Stroke::new(1.0, BORDER_SOFT)
    };
    ui.painter()
        .rect_stroke(rect, field_radius(), stroke, StrokeKind::Inside);
    crate::core::automation::instrument_response(response, "code_editor", None, true, true)
}

pub fn color_field(ui: &mut Ui, color: &mut Color32, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, FIELD_H), Sense::click());
    let mut response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let popup_id = response.id.with("color_picker_popup");
    if crate::core::automation::consume_pending_click_for_egui_id(response.id) {
        egui::Popup::toggle_id(ui.ctx(), popup_id);
    }
    let popup_open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let rounding = field_radius();
    let stroke = if response.has_focus() || popup_open {
        Stroke::new(1.0, BORDER_FOCUS)
    } else if response.hovered() {
        Stroke::new(1.0, BORDER)
    } else {
        Stroke::new(1.0, BORDER_SOFT)
    };

    ui.painter().rect_filled(rect, rounding, FIELD_BG);
    let swatch_rect = rect.shrink2(Vec2::new(3.0, 3.0));
    ui.painter()
        .rect_filled(swatch_rect, field_radius(), *color);
    ui.painter()
        .rect_stroke(rect, rounding, stroke, StrokeKind::Inside);

    let label = color_hex_label(*color);
    let label_color = readable_text_on_color(*color);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label.clone(),
        FontId::proportional(FIELD_TEXT_SIZE),
        label_color,
    );

    let mut changed = false;
    egui::Popup::menu(&response)
        .id(popup_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.spacing_mut().slider_width = 260.0;
            changed |= egui::color_picker::color_picker_color32(
                ui,
                color,
                egui::color_picker::Alpha::Opaque,
            );
        });
    if changed {
        response.mark_changed();
    }

    crate::core::automation::instrument_response(
        response.on_hover_text("Click to edit color"),
        "color_field",
        Some(label),
        true,
        false,
    )
}

fn color_hex_label(color: Color32) -> String {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn readable_text_on_color(color: Color32) -> Color32 {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    let luma = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
    if luma > 0.56 {
        PANEL_SUNKEN
    } else {
        TEXT_ON_ACCENT
    }
}

fn field_text_frame() -> Frame {
    Frame::new()
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .inner_margin(Margin::symmetric(
            FIELD_INNER_MARGIN_X,
            FIELD_INNER_MARGIN_Y,
        ))
}

fn field_stroke(output: &egui::text_edit::TextEditOutput) -> Stroke {
    if output.response.has_focus() {
        Stroke::new(1.0, BORDER_FOCUS)
    } else if output.response.hovered() {
        Stroke::new(1.0, BORDER)
    } else {
        Stroke::new(1.0, BORDER_SOFT)
    }
}

pub fn modal_scrim(ctx: &Context, id: &'static str) -> Response {
    let rect = ctx.content_rect();
    let area = egui::Area::new(egui::Id::new(format!("modal_scrim_{id}")))
        .order(egui::Order::Middle)
        .fixed_pos(rect.min);

    area.show(ctx, |ui| {
        let (local_rect, response) = ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
        ui.painter().rect_filled(local_rect, 0.0, MODAL_SCRIM_FILL);
        ui.painter()
            .rect_filled(local_rect, 0.0, MODAL_SCRIM_SOFT_WASH);
        paint_modal_vignette(ui.painter(), local_rect);
        crate::core::automation::instrument_response(
            response,
            "modal_scrim",
            Some(id.to_string()),
            true,
            false,
        )
    })
    .inner
}

pub fn dismissible_modal_scrim(ctx: &Context, id: &'static str, close_enabled: bool) -> bool {
    let response = modal_scrim(ctx, id);
    close_enabled && response.clicked()
}

fn paint_modal_vignette(painter: &egui::Painter, rect: Rect) {
    let shortest_side = rect.width().min(rect.height());
    if shortest_side <= 0.0 {
        return;
    }

    let [vignette_r, vignette_g, vignette_b, vignette_a] =
        MODAL_SCRIM_VIGNETTE_COLOR.to_srgba_unmultiplied();
    let band = MODAL_SCRIM_VIGNETTE_BAND.min(shortest_side * 0.28);
    let step = band / MODAL_SCRIM_VIGNETTE_STEPS as f32;
    for index in 0..MODAL_SCRIM_VIGNETTE_STEPS {
        let inset = index as f32 * step;
        let alpha_scale = 1.0 - index as f32 / MODAL_SCRIM_VIGNETTE_STEPS as f32;
        let alpha = (vignette_a as f32 * alpha_scale * alpha_scale)
            .round()
            .clamp(0.0, 255.0) as u8;
        if alpha == 0 {
            continue;
        }

        let color = Color32::from_rgba_unmultiplied(vignette_r, vignette_g, vignette_b, alpha);
        let left = rect.left() + inset;
        let right = rect.right() - inset;
        let top = rect.top() + inset;
        let bottom = rect.bottom() - inset;

        painter.rect_filled(
            Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, top + step)),
            0.0,
            color,
        );
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(left, bottom - step), Pos2::new(right, bottom)),
            0.0,
            color,
        );
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(left, top + step),
                Pos2::new(left + step, bottom - step),
            ),
            0.0,
            color,
        );
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(right - step, top + step),
                Pos2::new(right, bottom - step),
            ),
            0.0,
            color,
        );
    }
}

pub fn modal_header(ui: &mut Ui, title: &str, subtitle: Option<&str>) {
    let _ = modal_header_with_close(ui, title, subtitle, false);
}

pub fn modal_header_with_close(
    ui: &mut Ui,
    title: &str,
    subtitle: Option<&str>,
    close_enabled: bool,
) -> bool {
    let height = if subtitle.is_some() { 72.0 } else { 56.0 };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    ui.painter()
        .rect_filled(rect, MODAL_TOP_RADIUS, MODAL_HEADER_FILL);

    let content_rect = rect.shrink2(Vec2::new(18.0, 12.0));
    let close_clicked = if close_enabled {
        let button_rect = Rect::from_min_size(
            Pos2::new(
                rect.right() - MODAL_CLOSE_BUTTON_INSET_X - CLOSE_BUTTON_SIZE,
                rect.top() + MODAL_CLOSE_BUTTON_INSET_Y,
            ),
            Vec2::splat(CLOSE_BUTTON_SIZE),
        );
        let mut close_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(button_rect)
                .layout(Layout::top_down(Align::Center)),
        );
        close_button(&mut close_ui).on_hover_text("Close").clicked()
    } else {
        false
    };
    let title_right = if close_enabled {
        rect.right() - MODAL_CLOSE_BUTTON_INSET_X - CLOSE_BUTTON_SIZE - MODAL_CLOSE_BUTTON_TITLE_GAP
    } else {
        content_rect.right()
    };
    let title_rect = Rect::from_min_max(
        content_rect.left_top(),
        Pos2::new(title_right.max(content_rect.left()), content_rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(title_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.add_sized(
        [title_rect.width(), 20.0],
        egui::Label::new(RichText::new(title).color(TEXT).strong().size(17.0)).truncate(),
    );
    if let Some(subtitle) = subtitle {
        child.add_space(3.0);
        child.add_sized(
            [title_rect.width(), 18.0],
            egui::Label::new(RichText::new(subtitle).color(TEXT_MUTED).size(12.0)).truncate(),
        );
    }
    close_clicked
}

pub fn modal_body(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(PANEL_RAISED)
        .corner_radius(MODAL_BOTTOM_RADIUS)
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
        .size(10.5)
        .color(TEXT_MUTED)
        .strong()
}

pub fn field_label(ui: &mut Ui, label: &str) {
    ui.label(section_label(label));
}

pub fn primary_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    standalone_button(
        ui,
        label,
        width,
        ButtonSkin {
            fill: PRIMARY,
            hover_fill: PRIMARY_HOVER,
            active_fill: Color32::from_rgb(24, 145, 77),
            stroke: Color32::from_rgb(27, 138, 74),
            text: TEXT_ON_ACCENT,
            text_size: STANDALONE_BUTTON_TEXT_SIZE,
            radius: STANDALONE_BUTTON_RADIUS,
        },
    )
}

pub fn primary_button_sized(ui: &mut Ui, label: &str, width: f32, height: f32) -> Response {
    painted_button(
        ui,
        label,
        Vec2::new(width, height),
        ButtonSkin {
            fill: PRIMARY,
            hover_fill: PRIMARY_HOVER,
            active_fill: Color32::from_rgb(24, 145, 77),
            stroke: Color32::from_rgb(27, 138, 74),
            text: TEXT_ON_ACCENT,
            text_size: STANDALONE_BUTTON_TEXT_SIZE,
            radius: STANDALONE_BUTTON_RADIUS,
        },
    )
}

pub fn secondary_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    standalone_button(
        ui,
        label,
        width,
        ButtonSkin {
            fill: Color32::from_rgb(34, 35, 39),
            hover_fill: Color32::from_rgb(44, 46, 52),
            active_fill: Color32::from_rgb(27, 72, 52),
            stroke: BORDER,
            text: TEXT,
            text_size: STANDALONE_BUTTON_TEXT_SIZE,
            radius: STANDALONE_BUTTON_RADIUS,
        },
    )
}

pub fn field_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    painted_button(
        ui,
        label,
        Vec2::new(width, FIELD_H),
        ButtonSkin {
            fill: Color32::from_rgb(34, 35, 39),
            hover_fill: Color32::from_rgb(44, 46, 52),
            active_fill: Color32::from_rgb(27, 72, 52),
            stroke: BORDER,
            text: TEXT,
            text_size: 12.0,
            radius: field_radius_u8(),
        },
    )
}

pub fn danger_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    standalone_button(
        ui,
        label,
        width,
        ButtonSkin {
            fill: Color32::from_rgb(112, 28, 32),
            hover_fill: Color32::from_rgb(135, 36, 40),
            active_fill: Color32::from_rgb(92, 22, 27),
            stroke: DANGER,
            text: TEXT_ON_ACCENT,
            text_size: STANDALONE_BUTTON_TEXT_SIZE,
            radius: STANDALONE_BUTTON_RADIUS,
        },
    )
}

pub fn icon_button(ui: &mut Ui, label: &str) -> Response {
    painted_button(
        ui,
        label,
        Vec2::new(ICON_BUTTON_W, ICON_BUTTON_H),
        ButtonSkin {
            fill: Color32::from_rgb(27, 28, 32),
            hover_fill: Color32::from_rgb(38, 40, 45),
            active_fill: Color32::from_rgb(27, 72, 52),
            stroke: BORDER_SOFT,
            text: TEXT_MUTED,
            text_size: 11.0,
            radius: 4,
        },
    )
}

pub fn timeline_tool_icon_button(ui: &mut Ui, label: &str) -> Response {
    subtle_button(
        ui,
        label,
        Vec2::new(TIMELINE_TOOL_ICON_W, TIMELINE_TOOL_BUTTON_H),
        false,
        11.0,
        TIMELINE_TEXT_BUTTON_RADIUS,
    )
}

pub fn timeline_tool_text_button(ui: &mut Ui, label: &str, width: f32, active: bool) -> Response {
    subtle_button(
        ui,
        label,
        Vec2::new(width, TIMELINE_TOOL_BUTTON_H),
        active,
        10.5,
        TIMELINE_TEXT_BUTTON_RADIUS,
    )
}

#[derive(Clone, Copy, Debug)]
pub enum TimelineTransportIcon {
    First,
    Previous,
    Play,
    Pause,
    Next,
    Last,
    CaretUp,
    CaretDown,
}

pub fn timeline_transport_icon_button(
    ui: &mut Ui,
    icon: TimelineTransportIcon,
    active: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(TIMELINE_TRANSPORT_BUTTON_W, TIMELINE_TRANSPORT_BUTTON_H),
        Sense::click(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let skin = subtle_button_skin(active, 11.0, 4);
    paint_button_background(ui, rect, &response, skin);
    let icon_color = if active || response.hovered() || response.is_pointer_button_down_on() {
        TEXT
    } else {
        TEXT_MUTED
    };
    paint_timeline_transport_icon(ui, rect, icon, icon_color);
    crate::core::automation::instrument_response(
        response,
        "transport_button",
        Some(format!("{icon:?}")),
        true,
        false,
    )
}

pub fn queue_toggle_button(ui: &mut Ui, count: usize, active: bool, attention: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(TOP_BAR_BUTTON_MIN_W, TOP_BAR_BUTTON_H),
        Sense::click(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let skin = subtle_button_skin(active, TOP_BAR_BUTTON_TEXT_SIZE, TOP_BAR_BUTTON_RADIUS);
    paint_button_background(ui, rect, &response, skin);

    let text_color = if active || response.hovered() || response.is_pointer_button_down_on() {
        TEXT
    } else {
        TEXT_MUTED
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "QUE",
        FontId::proportional(10.0),
        text_color,
    );

    if attention {
        let time = ui.input(|input| input.time);
        let pulse = ((time * std::f64::consts::TAU / 1.6).sin() as f32 + 1.0) * 0.5;
        let alpha = (60.0 + pulse * 110.0).round() as u8;
        ui.painter().rect_stroke(
            rect.expand(1.0),
            CornerRadius::same(12),
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(244, 127, 45, alpha)),
            StrokeKind::Inside,
        );
    }

    if count > 0 {
        let label = if count > 99 {
            "99+".to_string()
        } else {
            count.to_string()
        };
        let badge_w = if count > 99 { 24.0 } else { 16.0 };
        let badge_rect = Rect::from_min_size(
            Pos2::new(rect.right() - badge_w + 5.0, rect.top() - 5.0),
            Vec2::new(badge_w, 16.0),
        );
        ui.painter()
            .rect_filled(badge_rect, CornerRadius::same(8), MARKER);
        ui.painter().rect_stroke(
            badge_rect,
            CornerRadius::same(8),
            Stroke::new(1.0, APP_BG),
            StrokeKind::Inside,
        );
        ui.painter().text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            FontId::proportional(9.0),
            Color32::from_rgb(18, 13, 8),
        );
    }

    crate::core::automation::instrument_response(
        response,
        "queue_button",
        Some("QUE".to_string()),
        true,
        false,
    )
}

pub fn top_bar_menu_button<R>(
    ui: &mut Ui,
    label: &str,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Response {
    let button_id = ui.make_persistent_id(("top_bar_menu_button", label));
    let popup_id = button_id.with("popup");
    let active = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let response = subtle_button_with_id(
        ui,
        label,
        Vec2::new(top_bar_text_button_width(label), TOP_BAR_BUTTON_H),
        active,
        TOP_BAR_BUTTON_TEXT_SIZE,
        TOP_BAR_BUTTON_RADIUS,
        button_id,
    );
    egui::Popup::menu(&response)
        .id(popup_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(add_contents);
    response
}

fn top_bar_text_button_width(label: &str) -> f32 {
    (label.chars().count() as f32 * 7.0 + TOP_BAR_BUTTON_PAD_X).max(TOP_BAR_BUTTON_MIN_W)
}

pub fn popover_button(ui: &mut Ui, label: &str, width: f32, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, POPOVER_BUTTON_H), sense);
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    let hovered = enabled && response.hovered();
    let pressed = enabled && response.is_pointer_button_down_on();
    let fill = if pressed {
        Color32::from_rgb(35, 39, 43)
    } else if hovered {
        Color32::from_rgb(34, 36, 41)
    } else {
        Color32::from_rgb(24, 25, 29)
    };
    let text = if enabled { TEXT } else { TEXT_DIM };
    let stroke = if hovered { BORDER } else { BORDER_SOFT };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(POPOVER_BUTTON_RADIUS), fill);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(POPOVER_BUTTON_RADIUS),
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.0),
        text,
    );
    crate::core::automation::instrument_response(
        response,
        "popover_button",
        Some(label.to_string()),
        enabled,
        false,
    )
}

fn subtle_button(
    ui: &mut Ui,
    label: &str,
    size: Vec2,
    active: bool,
    text_size: f32,
    radius: u8,
) -> Response {
    painted_button(
        ui,
        label,
        size,
        subtle_button_skin(active, text_size, radius),
    )
}

fn subtle_button_with_id(
    ui: &mut Ui,
    label: &str,
    size: Vec2,
    active: bool,
    text_size: f32,
    radius: u8,
    id: egui::Id,
) -> Response {
    painted_button_with_id(
        ui,
        label,
        size,
        subtle_button_skin(active, text_size, radius),
        id,
    )
}

fn subtle_button_skin(active: bool, text_size: f32, radius: u8) -> ButtonSkin {
    let fill = if active {
        Color32::from_rgb(31, 33, 38)
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if active {
        BORDER_SOFT
    } else {
        Color32::TRANSPARENT
    };
    ButtonSkin {
        fill,
        hover_fill: Color32::from_rgb(32, 34, 39),
        active_fill: Color32::from_rgb(35, 39, 43),
        stroke,
        text: if active { TEXT } else { TEXT_MUTED },
        text_size,
        radius,
    }
}

fn paint_timeline_transport_icon(ui: &Ui, rect: Rect, icon: TimelineTransportIcon, color: Color32) {
    let center = rect.center();
    match icon {
        TimelineTransportIcon::First => {
            paint_transport_bar(ui, Pos2::new(center.x - 5.0, center.y), color);
            paint_transport_triangle(ui, Pos2::new(center.x + 2.0, center.y), -1.0, color);
        }
        TimelineTransportIcon::Previous => {
            paint_transport_triangle(ui, center, -1.0, color);
        }
        TimelineTransportIcon::Play => {
            paint_transport_triangle(ui, Pos2::new(center.x + 0.75, center.y), 1.0, color);
        }
        TimelineTransportIcon::Pause => {
            let bar_w = 3.0;
            let bar_h = 10.0;
            for dx in [-3.0, 3.0] {
                ui.painter().rect_filled(
                    Rect::from_center_size(
                        Pos2::new(center.x + dx, center.y),
                        Vec2::new(bar_w, bar_h),
                    ),
                    0.8,
                    color,
                );
            }
        }
        TimelineTransportIcon::Next => {
            paint_transport_triangle(ui, center, 1.0, color);
        }
        TimelineTransportIcon::Last => {
            paint_transport_triangle(ui, Pos2::new(center.x - 2.0, center.y), 1.0, color);
            paint_transport_bar(ui, Pos2::new(center.x + 5.0, center.y), color);
        }
        TimelineTransportIcon::CaretUp => {
            paint_transport_caret(ui, center, -1.0, color);
        }
        TimelineTransportIcon::CaretDown => {
            paint_transport_caret(ui, center, 1.0, color);
        }
    }
}

fn paint_transport_triangle(ui: &Ui, center: Pos2, direction: f32, color: Color32) {
    let w = 7.0;
    let h = 9.0;
    let points = if direction >= 0.0 {
        [
            Pos2::new(center.x - w * 0.45, center.y - h * 0.5),
            Pos2::new(center.x - w * 0.45, center.y + h * 0.5),
            Pos2::new(center.x + w * 0.5, center.y),
        ]
    } else {
        [
            Pos2::new(center.x + w * 0.45, center.y - h * 0.5),
            Pos2::new(center.x + w * 0.45, center.y + h * 0.5),
            Pos2::new(center.x - w * 0.5, center.y),
        ]
    };
    ui.painter().add(egui::Shape::convex_polygon(
        points.to_vec(),
        color,
        Stroke::NONE,
    ));
}

fn paint_transport_bar(ui: &Ui, center: Pos2, color: Color32) {
    ui.painter().rect_filled(
        Rect::from_center_size(center, Vec2::new(2.0, 10.0)),
        0.8,
        color,
    );
}

fn paint_transport_caret(ui: &Ui, center: Pos2, direction: f32, color: Color32) {
    let w = 8.0;
    let h = 5.0;
    let points = if direction >= 0.0 {
        [
            Pos2::new(center.x - w * 0.5, center.y - h * 0.35),
            Pos2::new(center.x + w * 0.5, center.y - h * 0.35),
            Pos2::new(center.x, center.y + h * 0.55),
        ]
    } else {
        [
            Pos2::new(center.x - w * 0.5, center.y + h * 0.35),
            Pos2::new(center.x + w * 0.5, center.y + h * 0.35),
            Pos2::new(center.x, center.y - h * 0.55),
        ]
    };
    ui.painter().add(egui::Shape::convex_polygon(
        points.to_vec(),
        color,
        Stroke::NONE,
    ));
}

pub fn close_button(ui: &mut Ui) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(CLOSE_BUTTON_SIZE), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if response.is_pointer_button_down_on() {
        Color32::from_rgb(101, 31, 37)
    } else if response.hovered() {
        Color32::from_rgb(74, 28, 33)
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if response.has_focus() {
        BORDER_FOCUS
    } else if response.hovered() {
        Color32::from_rgb(118, 47, 55)
    } else {
        Color32::TRANSPARENT
    };
    let icon_color = if response.hovered() || response.is_pointer_button_down_on() {
        TEXT_ON_ACCENT
    } else {
        TEXT_MUTED
    };

    ui.painter()
        .rect_filled(rect, CornerRadius::same(CLOSE_BUTTON_RADIUS), fill);
    if stroke != Color32::TRANSPARENT {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(CLOSE_BUTTON_RADIUS),
            Stroke::new(1.0, stroke),
            StrokeKind::Inside,
        );
    }

    let half = CLOSE_BUTTON_ICON_SIZE * 0.5;
    let center = rect.center();
    let stroke = Stroke::new(CLOSE_BUTTON_ICON_STROKE, icon_color);
    ui.painter().line_segment(
        [
            Pos2::new(center.x - half, center.y - half),
            Pos2::new(center.x + half, center.y + half),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            Pos2::new(center.x + half, center.y - half),
            Pos2::new(center.x - half, center.y + half),
        ],
        stroke,
    );

    crate::core::automation::instrument_response(
        response,
        "close_button",
        Some("Close".to_string()),
        true,
        false,
    )
}

pub fn media_pill(ui: &mut Ui, label: &str, color: Color32) -> Response {
    let width = (label.chars().count() as f32 * 7.0 + 22.0).max(42.0);
    media_pill_sized(ui, label, color, width)
}

pub fn media_pill_sized(ui: &mut Ui, label: &str, color: Color32, width: f32) -> Response {
    painted_button(
        ui,
        label,
        Vec2::new(width, MEDIA_PILL_H),
        ButtonSkin {
            fill: Color32::from_rgb(17, 20, 22),
            hover_fill: color.gamma_multiply(0.18),
            active_fill: color.gamma_multiply(0.28),
            stroke: color.gamma_multiply(0.55),
            text: color,
            text_size: 11.0,
            radius: 5,
        },
    )
}

pub fn equal_media_pill_row(
    ui: &mut Ui,
    items: &[(&str, Color32)],
    mut on_clicked: impl FnMut(usize),
) {
    if items.is_empty() {
        return;
    }

    let row_width = ui.available_width().max(0.0);
    let (row_rect, _) = ui.allocate_exact_size(Vec2::new(row_width, MEDIA_PILL_H), Sense::hover());
    let count = items.len() as f32;
    let gaps = (items.len().saturating_sub(1)) as f32;
    let ideal_gap = FORM_ROW_GAP
        .min(((row_width - MEDIA_PILL_MIN_W * count) / gaps.max(1.0)).max(MEDIA_PILL_MIN_GAP));
    let gap = if items.len() > 1 { ideal_gap } else { 0.0 };
    let button_w = ((row_width - gap * gaps) / count).max(0.0);

    let mut x = row_rect.left();
    for (index, (label, color)) in items.iter().enumerate() {
        let rect = Rect::from_min_size(
            Pos2::new(x, row_rect.top()),
            Vec2::new(button_w, MEDIA_PILL_H),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
        );
        child.shrink_clip_rect(rect);
        if media_pill_sized(&mut child, label, *color, button_w).clicked() {
            on_clicked(index);
        }
        x += button_w + gap;
    }
}

#[derive(Clone, Copy)]
struct ButtonSkin {
    fill: Color32,
    hover_fill: Color32,
    active_fill: Color32,
    stroke: Color32,
    text: Color32,
    text_size: f32,
    radius: u8,
}

fn standalone_button(ui: &mut Ui, label: &str, width: f32, skin: ButtonSkin) -> Response {
    painted_button(ui, label, Vec2::new(width, STANDALONE_BUTTON_H), skin)
}

pub fn field_radius() -> CornerRadius {
    CornerRadius::same(field_radius_u8())
}

pub fn field_radius_u8() -> u8 {
    4
}

pub fn field_prefix(label: &str) -> RichText {
    RichText::new(format!("{label} "))
        .color(TEXT_MUTED)
        .size(12.0)
        .strong()
}

pub fn configure_field_widget_style(ui: &mut Ui, min_width: f32) {
    ui.spacing_mut().interact_size = Vec2::new(min_width, FIELD_H);
    ui.spacing_mut().button_padding = Vec2::new(8.0, 5.0);
    ui.style_mut().drag_value_text_style = egui::TextStyle::Body;
    let visuals = ui.visuals_mut();
    visuals.text_edit_bg_color = Some(FIELD_BG);
    visuals.selection.stroke = Stroke::new(1.0, BORDER_FOCUS);
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.inactive.bg_fill = FIELD_BG;
    visuals.widgets.inactive.weak_bg_fill = FIELD_BG;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SOFT);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.corner_radius = field_radius();
    visuals.widgets.hovered.bg_fill = FIELD_BG;
    visuals.widgets.hovered.weak_bg_fill = FIELD_BG;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.corner_radius = field_radius();
    visuals.widgets.active.bg_fill = FIELD_BG;
    visuals.widgets.active.weak_bg_fill = FIELD_BG;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, BORDER_FOCUS);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.active.corner_radius = field_radius();
    visuals.widgets.open.bg_fill = FIELD_BG;
    visuals.widgets.open.weak_bg_fill = FIELD_BG;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, BORDER_FOCUS);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.open.corner_radius = field_radius();
}

fn painted_button(ui: &mut Ui, label: &str, size: Vec2, skin: ButtonSkin) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    paint_button_at(ui, rect, response, label, skin)
}

fn painted_button_with_id(
    ui: &mut Ui,
    label: &str,
    size: Vec2,
    skin: ButtonSkin,
    id: egui::Id,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let response = ui.interact(rect, id, Sense::click());
    paint_button_at(ui, rect, response, label, skin)
}

fn paint_button_at(
    ui: &mut Ui,
    rect: Rect,
    response: Response,
    label: &str,
    skin: ButtonSkin,
) -> Response {
    let enabled = response.enabled();
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    paint_button_background(ui, rect, &response, skin);
    let text_color = if enabled {
        skin.text
    } else {
        ui.visuals().gray_out(skin.text)
    };
    let galley =
        egui::WidgetText::from(RichText::new(label).color(text_color).size(skin.text_size))
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                (rect.width() - 12.0).max(0.0),
                FontId::proportional(skin.text_size),
            );
    ui.painter()
        .galley(rect.center() - galley.size() * 0.5, galley, text_color);
    crate::core::automation::instrument_response(
        response,
        "button",
        Some(label.to_string()),
        enabled,
        false,
    )
}

fn paint_button_background(ui: &Ui, rect: Rect, response: &Response, skin: ButtonSkin) {
    let enabled = response.enabled();
    let fill = if !enabled {
        disabled_button_color(ui, skin.fill)
    } else if response.is_pointer_button_down_on() {
        skin.active_fill
    } else if response.hovered() || response.has_focus() {
        skin.hover_fill
    } else {
        skin.fill
    };
    let stroke = if !enabled {
        disabled_button_color(ui, skin.stroke)
    } else if response.has_focus() {
        BORDER_FOCUS
    } else if response.hovered() {
        skin.stroke.gamma_multiply(1.35)
    } else {
        skin.stroke
    };

    ui.painter()
        .rect_filled(rect, CornerRadius::same(skin.radius), fill);
    if stroke != Color32::TRANSPARENT {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(skin.radius),
            Stroke::new(1.0, stroke),
            StrokeKind::Inside,
        );
    }
}

fn disabled_button_color(ui: &Ui, color: Color32) -> Color32 {
    if color == Color32::TRANSPARENT {
        Color32::TRANSPARENT
    } else {
        ui.visuals().gray_out(color)
    }
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

pub fn collapsed_rail_button(ui: &mut Ui, icon: &str) -> Response {
    let size = ui.available_size();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(size.x, size.y.max(1.0)), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if response.hovered() {
        Color32::from_rgb(20, 22, 25)
    } else {
        PANEL
    };
    ui.painter().rect_filled(rect, 0.0, fill);

    let button_rect = Rect::from_center_size(
        Pos2::new(
            rect.center().x,
            rect.top() + PANEL_PAD as f32 + COLLAPSED_RAIL_BUTTON_SIZE * 0.5,
        ),
        Vec2::splat(COLLAPSED_RAIL_BUTTON_SIZE),
    );
    let button_fill = if response.is_pointer_button_down_on() {
        FIELD_BG_ACTIVE
    } else if response.hovered() {
        FIELD_BG_HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(button_rect, CornerRadius::same(4), button_fill);
    if response.hovered() || response.has_focus() {
        ui.painter().rect_stroke(
            button_rect,
            CornerRadius::same(4),
            Stroke::new(
                1.0,
                if response.has_focus() {
                    BORDER_FOCUS
                } else {
                    BORDER_SOFT
                },
            ),
            StrokeKind::Inside,
        );
    }
    ui.painter().text(
        button_rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        FontId::proportional(12.0),
        TEXT_MUTED,
    );
    crate::core::automation::instrument_response(
        response,
        "collapsed_rail",
        Some(icon.to_string()),
        true,
        false,
    )
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
    draw_accent_row_with_status(ui, height, selected, accent, None, add_contents)
}

pub fn draw_accent_row_with_status(
    ui: &mut Ui,
    height: f32,
    selected: bool,
    accent: Color32,
    status_accent: Option<Color32>,
    add_contents: impl FnOnce(&mut Ui, Rect),
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        Sense::click_and_drag(),
    );
    let fill = row_fill(selected, response.hovered());
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    let stroke_color = status_accent
        .map(|color| color.gamma_multiply(if selected { 1.0 } else { 0.82 }))
        .unwrap_or(if selected { accent } else { BORDER_SOFT });
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(5),
        Stroke::new(1.0, stroke_color),
        StrokeKind::Inside,
    );
    if let Some(color) = status_accent {
        let inner = rect.shrink(1.5);
        ui.painter().rect_stroke(
            inner,
            CornerRadius::same(4),
            Stroke::new(1.0, color.gamma_multiply(0.38)),
            StrokeKind::Inside,
        );
    }
    ui.painter().rect_filled(
        Rect::from_min_size(rect.left_top(), Vec2::new(4.0, rect.height())),
        CornerRadius::same(2),
        status_accent.unwrap_or(accent),
    );
    let content_rect = rect.shrink2(Vec2::new(10.0, 5.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.shrink_clip_rect(content_rect);
    add_contents(&mut child, content_rect);
    crate::core::automation::instrument_response(
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
        "row",
        None,
        true,
        false,
    )
}
