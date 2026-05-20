//! Product-specific egui primitives for the editor shell.

use std::hash::Hash;
use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Align, Color32, Context, CornerRadius, FontId, Frame, Layout, Margin, Pos2, Rect,
    Response, RichText, Sense, Shadow, Stroke, StrokeKind, Ui, Vec2,
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
pub const STANDALONE_BUTTON_H: f32 = 32.0;
pub const STANDALONE_BUTTON_RADIUS: u8 = 5;
pub const STANDALONE_BUTTON_TEXT_SIZE: f32 = 12.0;
pub const PRIMARY_BUTTON_H: f32 = STANDALONE_BUTTON_H;
pub const SECONDARY_BUTTON_H: f32 = STANDALONE_BUTTON_H;
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
pub const FORM_ROW_GAP: f32 = 8.0;
pub const ACTION_GAP: f32 = 12.0;

pub fn configure_style(ctx: &Context) {
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
    add_contents(&mut child);
    response
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

pub fn scroll_body(ui: &mut Ui, add_body: impl FnOnce(&mut Ui)) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, add_body);
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
    FIELD_LABEL_H + FIELD_LABEL_GAP + control_height
}

pub fn labeled_text_field(ui: &mut Ui, label: &str, value: &mut String) -> Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = FIELD_LABEL_GAP;
        field_label(ui, label);
        singleline_text_field(ui, value, ui.available_width())
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
    output.response.response.on_hover_text(value)
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
    configure_field_widget_style(&mut child, rect.width());

    let field_id = child.next_auto_id();
    child.skip_ahead_auto_ids(1);

    ui.painter().rect_filled(rect, field_radius(), FIELD_BG);
    egui::TextEdit::singleline(value)
        .id(field_id)
        .desired_width(rect.width())
        .min_size(rect.size())
        .horizontal_align(FIELD_TEXT_ALIGN)
        .vertical_align(Align::Center)
        .text_color(TEXT)
        .font(FontId::proportional(FIELD_TEXT_SIZE))
        .frame(field_text_frame())
        .show(&mut child)
}

pub fn singleline_text_field(ui: &mut Ui, value: &mut String, width: f32) -> Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, TEXT_FIELD_H), Sense::hover());
    let mut output = field_text_edit(ui, value, rect);
    let selected_text = value.clone();
    select_all_on_focus(&mut output, &selected_text);
    ui.painter().rect_stroke(
        rect,
        field_radius(),
        field_stroke(&output),
        StrokeKind::Inside,
    );
    output.response.response
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

pub fn modal_scrim(ctx: &Context, id: &'static str) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new(format!("modal_scrim_{id}")),
    ));
    let rect = ctx.content_rect();
    painter.rect_filled(rect, 0.0, MODAL_SCRIM_FILL);
    painter.rect_filled(rect, 0.0, MODAL_SCRIM_SOFT_WASH);
    paint_modal_vignette(&painter, rect);
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
        .size(10.0)
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
        Vec2::new(24.0, 22.0),
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

    response
}

pub fn media_pill(ui: &mut Ui, label: &str, color: Color32) -> Response {
    let width = (label.chars().count() as f32 * 7.0 + 22.0).max(42.0);
    painted_button(
        ui,
        label,
        Vec2::new(width, 24.0),
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
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if response.is_pointer_button_down_on() {
        skin.active_fill
    } else if response.hovered() || response.has_focus() {
        skin.hover_fill
    } else {
        skin.fill
    };
    let stroke = if response.has_focus() {
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
    let galley = egui::WidgetText::from(RichText::new(label).color(skin.text).size(skin.text_size))
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (rect.width() - 12.0).max(0.0),
            FontId::proportional(skin.text_size),
        );
    ui.painter()
        .galley(rect.center() - galley.size() * 0.5, galley, skin.text);
    response
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
