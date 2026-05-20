## Executive summary

The core rule is: **egui is not CSS**. It is a single-pass, immediate-mode layout system where each widget asks for space as it is added, and the parent `Ui` cursor advances. The official docs and egui’s own tracking issue describe this as the fundamental layout challenge: you usually do not know a container’s final size before laying out its contents, so polished layouts need either fixed/bounded regions, remembered sizes, explicit rect math, or helpers like `egui_extras::StripBuilder`. 

For your desktop video editor, the most maintainable mental model is:

**Use panels for the application shell, `StripBuilder` for flex/grid-like regions, `ScrollArea` for overflowing body content, and manual `Rect` math only for canvas/timeline/preview areas or carefully isolated widget subregions.**

For the specific “growing middle + bottom-pinned footer” problem, the strongest pattern is:

```rust
StripBuilder::new(ui)
    .size(Size::remainder().at_least(min_body_h))
    .size(Size::exact(footer_h))
    .vertical(|mut strip| {
        strip.cell(|ui| body(ui));
        strip.cell(|ui| footer(ui));
    });
```

That pattern is much less fragile than `ui.add_space(ui.available_height() - reserved)`, less surprising than mixing `Layout::bottom_up` with scroll areas, and easier to compose than manual `Rect` splitting.

Your `egui-refactor` branch is already on `eframe = 0.34.2`, and it already uses `eframe::App::ui`, so the 0.34 migration advice below applies directly.  The main branch-specific cleanup is to stop routing the shell through cloned `Context` + deprecated `TopBottomPanel`/`SidePanel` calls, and instead build panels from the root `Ui` with `egui::Panel::{top,bottom,left,right}.show_inside(...)`. The 0.34 changelog explicitly says the new direction is “More `Ui`, less `Context`,” `App::update` was replaced by `App::ui`, and `SidePanel`/`TopBottomPanel` were replaced by unified `Panel`. ([GitHub][1])

---

## Recommended layout primitives and when to use each

| Primitive                                        | Use it for                                                                                              | Avoid it for                                                              | Notes                                                                                                                                                                                       |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `egui::Panel`                                    | App chrome: top menu, side asset/inspector panels, bottom timeline/status, central preview.             | Fine-grained card internals.                                              | `Panel` covers a side of a `Ui` or screen; panel order matters, and `CentralPanel` must be added last. ([Docs.rs][2])                                                                       |
| `egui_extras::StripBuilder`                      | CSS-ish flex/grid: fixed header, growing body, pinned footer, two columns, equal cells, exact gaps.     | Variable-height flow content where normal egui vertical layout is enough. | Official docs: strip cells **do not grow with children**, and you preallocate sizes before adding cells. This is why it approximates CSS grid/flex better than normal egui. ([Docs.rs][3])  |
| `ScrollArea`                                     | Bounded overflowing bodies: inspectors, asset lists, modal forms, queues, logs.                         | “I need this area to fill height” by itself.                              | `max_height` caps; it does not mean “fill”. Use a bounded parent cell plus `auto_shrink([false, false])`. ([Docs.rs][4])                                                                    |
| `Layout::bottom_up`                              | Small isolated footer/action row when you truly want to place widgets from the bottom edge upward.      | Body + scroll + footer layouts.                                           | It is tempting for flexbox-like UIs, but there is a known issue pattern involving `bottom_up` + vertical scroll/text input, and `with_layout` takes all available space.                    |
| `ui.add_space(ui.available_height() - reserved)` | Last-resort glue in a bounded region with clamped values.                                               | Responsive layouts, modals, scroll bodies.                                | `add_space` expands `min_rect`, depends on current layout direction, and is additional to normal `item_spacing`. ([Docs.rs][5])                                                             |
| `allocate_ui_with_layout`                        | Inline fixed-size child regions, custom row/cell widgets, card contents after a known allocation.       | Strict clipping/flex allocation.                                          | If contents overflow, egui allocates more space; it is not a hard CSS box. ([Docs.rs][5])                                                                                                   |
| `UiBuilder::max_rect` + `scope_builder`          | Precise child UIs inside manually split rects; replacing older `allocate_ui_at_rect`/ad-hoc child APIs. | Normal vertical/horizontal layout.                                        | `max_rect` constrains widgets, labels wrap to fit, but a child can still expand if something does not fit. `scope_builder` creates a child and allocates only what was used. ([Docs.rs][6]) |
| Manual `Rect` splitting                          | Timeline, preview/canvas painting, overlays, custom rows with painter interaction.                      | Ordinary forms/cards where `StripBuilder` works.                          | If you manually place widgets, allocate one parent rect first so the parent cursor remains sane.                                                                                            |

---

## The canonical pattern: vertical body that grows, footer pinned

This should become a `ui_kit` helper.

```rust
use egui::{Align, Layout, Ui};
use egui_extras::{Size, StripBuilder};

pub fn body_with_footer(
    ui: &mut Ui,
    min_body_h: f32,
    footer_h: f32,
    body: impl FnOnce(&mut Ui),
    footer: impl FnOnce(&mut Ui),
) {
    StripBuilder::new(ui)
        .clip(true)
        .size(Size::remainder().at_least(min_body_h))
        .size(Size::exact(footer_h))
        .vertical(|mut strip| {
            strip.cell(|ui| {
                body(ui);
            });

            strip.cell(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    footer(ui);
                });
            });
        });
}
```

Use it for asset panels, inspectors, modal columns, provider lists, and queue panels. Inside the body cell, use a scroll area that fills the cell:

```rust
egui::ScrollArea::vertical()
    .auto_shrink([false, false])
    .show(ui, |ui| {
        // Long form/list content.
    });
```

The important part is `auto_shrink([false, false])`: when content is short, blank space remains **inside** the scroll area, so the scroll viewport still occupies the available body cell. The docs describe the opposite default: `auto_shrink(true)` puts blank space outside the scroll area. ([Docs.rs][4])

---

## `StripBuilder`: closest supported abstraction to CSS flex/grid

Yes: **`egui_extras::StripBuilder` is the closest officially supported abstraction to CSS flex/grid for app layout**, with two caveats:

1. It is not a browser layout solver. You predeclare rows/columns.
2. Strip cells do not grow with children, which is actually good for polished desktop UI because it prevents a footer from being pushed away by overflowing content. ([Docs.rs][3])

The official demo uses `Size::exact`, `Size::remainder`, `Size::relative(...).at_least(...)`, nested horizontal/vertical strips, and empty spacer cells.  Rerun, a high-quality egui app, also builds reusable layout helpers around strip-like patterns; its `FormStrip` is described as a minimal horizontal version of `egui_extras::StripBuilder`, which is a useful design precedent for your own `ui_kit` form rows. 

### Fixed header + growing body + fixed footer

```rust
StripBuilder::new(ui)
    .size(Size::exact(header_h))
    .size(Size::remainder().at_least(220.0))
    .size(Size::exact(footer_h))
    .vertical(|mut strip| {
        strip.cell(|ui| header(ui));
        strip.cell(|ui| body(ui));
        strip.cell(|ui| footer(ui));
    });
```

### Two columns with a fixed gap

```rust
let gap = ui.spacing().item_spacing.x;

StripBuilder::new(ui)
    .size(Size::remainder().at_least(360.0)) // left: grows
    .size(Size::exact(gap))                  // gap cell
    .size(Size::exact(260.0))                // right: fixed inspector/list
    .horizontal(|mut strip| {
        strip.cell(|ui| left_column(ui));
        strip.empty();
        strip.cell(|ui| right_column(ui));
    });
```

### Equal cells

```rust
StripBuilder::new(ui)
    .sizes(Size::remainder(), 3)
    .horizontal(|mut strip| {
        strip.cell(|ui| preset_button(ui, "1080p"));
        strip.cell(|ui| preset_button(ui, "4K"));
        strip.cell(|ui| preset_button(ui, "9:16"));
    });
```

### Relative cell with bounds

```rust
StripBuilder::new(ui)
    .size(Size::relative(0.66).at_least(360.0))
    .size(Size::exact(ui.spacing().item_spacing.x))
    .size(Size::remainder().at_least(220.0).at_most(340.0))
    .horizontal(|mut strip| {
        strip.cell(|ui| main_form(ui));
        strip.empty();
        strip.cell(|ui| side_list(ui));
    });
```

`Size` supports absolute, relative, and remainder sizing; `Size::exact` is fixed points, `Size::relative` is a fraction of available space, and multiple `Size::remainder()` cells share remaining space. ([Docs.rs][7])

---

## ScrollArea sizing: what actually happens

`ScrollArea::max_height(h)` sets the maximum height of the **outer** scroll area frame. It does not force the scroll area to use `h`, and it does not pin anything below it. The docs say the default `f32::INFINITY` lets the scroll area expand to fit the surrounding `Ui`. ([Docs.rs][4])

`min_scrolled_height(h)` only applies when a vertical scroll area requires scrollbars. It is a lower bound for the scrolled viewport; the default is `64.0`, and the scroll area can still be smaller when content itself is smaller and no scrollbar is required. ([Docs.rs][4])

`auto_shrink([false, false])` is the key for “fill this bounded cell.” With `true`, blank space goes outside the scroll area. With `false`, blank space goes inside it. In a modal body cell, you almost always want:

```rust
egui::ScrollArea::vertical()
    .auto_shrink([false, false])
    .show(ui, |ui| {
        // form/list rows
    });
```

For huge lists, prefer `show_rows`; the docs show measuring row height via `ui.text_style_height(&TextStyle::Body)` for labels or `ui.spacing().interact_size.y` for button rows. ([Docs.rs][4])

---

## Reusable `ui_kit` layout templates

Your current `ui_kit` already centralizes colors, frames, margins, button drawing, modal headers, card frames, and row painting. It also currently embeds important heights inside widget functions: primary buttons are painted at `36.0`, secondary buttons at `32.0`, icon buttons at `24x22`, text fields at `24.0`, and cards use exact allocation plus `UiBuilder::max_rect`.  Make those heights explicit semantic constants or derive them from `ui.spacing().interact_size.y`.

### Color discipline

Visible UI primitives should use semantic `ui_kit` tokens rather than direct `Color32` literals. Avoid pure black or pure white for field fills, text, strokes, panels, and buttons; use tinted near-black and near-white tokens such as `FIELD_BG`, `FIELD_BG_HOVER`, `FIELD_BG_ACTIVE`, `TEXT`, and `TEXT_ON_ACCENT`. Editable text fields, read-only value fields, and numeric `DragValue` fields should share the same field surface tokens so they read as one control family.

### 1. Metrics helper

```rust
pub struct LayoutMetrics {
    pub gap_x: f32,
    pub gap_y: f32,
    pub row_h: f32,
    pub body_text_h: f32,
    pub primary_button_h: f32,
    pub secondary_button_h: f32,
    pub action_bar_h: f32,
}

impl LayoutMetrics {
    pub fn from_ui(ui: &egui::Ui) -> Self {
        let spacing = ui.spacing();
        let row_h = spacing.interact_size.y;

        // Keep these synced with kit::primary_button / secondary_button,
        // or better: expose them from ui_kit as constants.
        let primary_button_h = 36.0;
        let secondary_button_h = 32.0;

        Self {
            gap_x: spacing.item_spacing.x,
            gap_y: spacing.item_spacing.y,
            row_h,
            body_text_h: ui.text_style_height(&egui::TextStyle::Body),
            primary_button_h,
            secondary_button_h,
            action_bar_h: primary_button_h + spacing.item_spacing.y * 2.0,
        }
    }
}
```

### 2. Filled card

```rust
pub fn fill_card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    kit::card_frame().show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        add(ui);
    });
}
```

### 3. Bottom action bar

```rust
pub fn right_aligned_action_bar(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        add(ui);
    });
}

pub fn left_filled_primary_bar(
    ui: &mut egui::Ui,
    label: &str,
) -> egui::Response {
    kit::primary_button(ui, label, ui.available_width())
}
```

### 4. Two-column form row

```rust
pub fn form_row(
    ui: &mut egui::Ui,
    label: &str,
    label_w: f32,
    add_control: impl FnOnce(&mut egui::Ui),
) {
    let row_h = ui.spacing().interact_size.y;

    ui.horizontal(|ui| {
        ui.add_sized(
            [label_w, row_h],
            egui::Label::new(kit::caption(label.to_ascii_uppercase())).truncate(),
        );

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| add_control(ui),
        );
    });
}
```

For inspector fields, your branch currently uses manual `Rect` splitting for numeric pairs. That is okay for tightly controlled custom widgets, but use a `StripBuilder` or a reusable `FormStrip`-style helper for regular property rows. Rerun’s form helper computes fractions and adds fixed-height fields with `allocate_ui_with_layout`, which is a good pattern for compact editor inspectors. 

### 5. Inspector panel with scrollable body and pinned footer

```rust
pub fn inspector_panel(
    ui: &mut egui::Ui,
    title: &str,
    footer_h: f32,
    body: impl FnOnce(&mut egui::Ui),
    footer: impl FnOnce(&mut egui::Ui),
) {
    let min_body_h = ui.spacing().interact_size.y * 4.0;

    kit::panel_header(ui, title, None, || {});
    ui.add_space(ui.spacing().item_spacing.y);

    body_with_footer(
        ui,
        min_body_h,
        footer_h,
        |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, body);
        },
        footer,
    );
}
```

### 6. Split pane inside central content

Use top-level `Panel` for major resizable shell splits. Use `StripBuilder` for internal non-interactive columns. If you need draggable internal splitters, either store a split fraction yourself and paint/interact with a separator rect, or use a docking/split-pane crate. For a video editor, keep manual splitter math isolated to preview/timeline-style surfaces.

```rust
pub fn preview_with_inspector_strip(
    ui: &mut egui::Ui,
    inspector_w: f32,
    preview: impl FnOnce(&mut egui::Ui),
    inspector: impl FnOnce(&mut egui::Ui),
) {
    let gap = ui.spacing().item_spacing.x;

    StripBuilder::new(ui)
        .clip(true)
        .size(Size::remainder().at_least(480.0))
        .size(Size::exact(gap))
        .size(Size::exact(inspector_w))
        .horizontal(|mut strip| {
            strip.cell(|ui| preview(ui));
            strip.empty();
            strip.cell(|ui| inspector(ui));
        });
}
```

---

## Modal/dialog layout structure

Use this hierarchy:

```text
Modal / Window
└── fixed-size or responsive outer Ui
    └── vertical Strip
        ├── exact header
        └── remainder body shell
            └── body Frame with padding
                └── horizontal Strip
                    ├── left column
                    │   └── vertical Strip: scrollable form + pinned footer
                    ├── gap
                    └── right column
                        └── vertical Strip: scrollable list + pinned button
```

In 0.34, `egui::Modal` is now a good fit for real modals: it is centered, has a backdrop, and blocks input behind it. ([Docs.rs][8]) Your branch currently implements modal scrims manually with `kit::modal_scrim` plus `Window`, which works, but `Modal` can remove a lot of that custom layering. 

### Before: fragile modal body pattern

This is the kind of pattern to retire. Your current new-project modal has manual left/right widths, fixed card heights, `allocate_ui_with_layout`, a recent-project list with `max_height(ui.available_height() - 48.0)`, `Layout::bottom_up` for a bottom button, and a left form split by manual `Rect`s.  

```rust
let card_h = ui.available_height().min(PROJECT_WIZARD_CARD_H).max(360.0);
let left_w = ((ui.available_width() - gap) * 2.0 / 3.0).max(360.0);
let right_w = (ui.available_width() - gap - left_w).max(180.0);

ui.horizontal(|ui| {
    ui.allocate_ui_with_layout(
        Vec2::new(left_w, card_h),
        Layout::top_down(Align::Min),
        |ui| {
            kit::card_panel(ui, card_h, |ui| {
                self.new_project_create_card(ui); // internally splits Rects
            });
        },
    );

    ui.allocate_ui_with_layout(
        Vec2::new(right_w, card_h),
        Layout::top_down(Align::Min),
        |ui| {
            kit::card_panel(ui, card_h, |ui| {
                let list_height = (ui.available_height() - 48.0).max(120.0);

                egui::ScrollArea::vertical()
                    .max_height(list_height)
                    .show(ui, |ui| {
                        // recent projects
                    });

                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    kit::secondary_button(ui, "Browse for Project...", ui.available_width());
                });
            });
        },
    );
});
```

The weaknesses are: the list height is guessed, the bottom button competes with the scroll area, and the left card’s footer is split manually even though this is a standard “body + footer” layout.

### After: modal with fixed header, two-column body, scrollable bodies, pinned footers

Add this dependency:

```toml
egui_extras = "0.34.2"
```

Then structure the modal like this:

```rust
use egui::{Align, Color32, Id, Layout, Margin, Ui};
use egui_extras::{Size, StripBuilder};

const MODAL_HEADER_H: f32 = 72.0;
const MODAL_BODY_MIN_H: f32 = 260.0;
const LEFT_MIN_W: f32 = 360.0;
const RIGHT_W: f32 = 260.0;

fn footer_h(ui: &Ui, rows: f32) -> f32 {
    let m = LayoutMetrics::from_ui(ui);
    rows * m.row_h + (rows + 1.0) * m.gap_y
}

fn new_project_modal_after(&mut self, ctx: &egui::Context) {
    let size = project_wizard_size(ctx);

    egui::Modal::new(Id::new("new_project_modal"))
        .frame(kit::modal_frame())
        .backdrop_color(Color32::from_black_alpha(168))
        .show(ctx, |ui| {
            ui.set_width(size.x);
            ui.set_height(size.y);

            StripBuilder::new(ui)
                .clip(true)
                .size(Size::exact(MODAL_HEADER_H))
                .size(Size::remainder().at_least(MODAL_BODY_MIN_H))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        let close = kit::modal_header_with_close(
                            ui,
                            "New Project",
                            Some("Choose project settings and save location."),
                            true,
                        );

                        if close {
                            self.editor.overlays.new_project = false;
                        }
                    });

                    strip.cell(|ui| {
                        modal_two_column_body(ui, |ui| {
                            new_project_left_column(self, ui);
                        }, |ui| {
                            new_project_right_column(self, ui);
                        });
                    });
                });
        });
}

fn modal_two_column_body(
    ui: &mut Ui,
    left: impl FnOnce(&mut Ui),
    right: impl FnOnce(&mut Ui),
) {
    egui::Frame::new()
        .fill(kit::PANEL_RAISED)
        .inner_margin(Margin::symmetric(18, 16))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());

            let gap = ui.spacing().item_spacing.x;

            StripBuilder::new(ui)
                .clip(true)
                .size(Size::remainder().at_least(LEFT_MIN_W))
                .size(Size::exact(gap))
                .size(Size::exact(RIGHT_W))
                .horizontal(|mut strip| {
                    strip.cell(left);
                    strip.empty();
                    strip.cell(right);
                });
        });
}

fn new_project_left_column(app: &mut NlaEguiApp, ui: &mut Ui) {
    fill_card(ui, |ui| {
        let save_location_h = footer_h(ui, 3.0);

        body_with_footer(
            ui,
            180.0,
            save_location_h,
            |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        kit::field_label(ui, "Create New Project");
                        ui.add_space(ui.spacing().item_spacing.y);

                        form_row(ui, "Project Name", 116.0, |ui| {
                            kit::singleline_text_field(
                                ui,
                                &mut app.new_project_name,
                                ui.available_width(),
                            );
                        });

                        ui.add_space(ui.spacing().item_spacing.y);
                        settings_fields(ui, &mut app.project_settings);
                    });
            },
            |ui| {
                kit::field_label(ui, "Save Location");
                ui.add_space(ui.spacing().item_spacing.y);

                if location_picker_row(ui, &app.new_project_parent).clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        app.new_project_parent = folder;
                    }
                }

                ui.add_space(ui.spacing().item_spacing.y);

                let enabled = !app.new_project_name.trim().is_empty();
                ui.add_enabled_ui(enabled, |ui| {
                    if kit::primary_button(ui, "Create Project", ui.available_width()).clicked() {
                        match app.editor.create_project(
                            &app.new_project_parent,
                            app.new_project_name.trim(),
                            app.project_settings.clone(),
                        ) {
                            Ok(_) => app.editor.overlays.new_project = false,
                            Err(err) => app.editor.status = err,
                        }
                    }
                });
            },
        );
    });
}

fn new_project_right_column(app: &mut NlaEguiApp, ui: &mut Ui) {
    fill_card(ui, |ui| {
        let button_footer_h = footer_h(ui, 1.0);

        body_with_footer(
            ui,
            180.0,
            button_footer_h,
            |ui| {
                kit::field_label(ui, "Recent Projects");
                ui.add_space(ui.spacing().item_spacing.y);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let recent = recent_projects(&app.new_project_parent);

                        if recent.is_empty() {
                            kit::empty_state(
                                ui,
                                "No recent projects",
                                "Browse to open an existing project folder.",
                            );
                        }

                        for folder in recent {
                            let label = folder
                                .file_name()
                                .and_then(|v| v.to_str())
                                .unwrap_or("Project");

                            if kit::secondary_button(ui, label, ui.available_width()).clicked() {
                                match app.editor.open_project(folder) {
                                    Ok(_) => app.editor.overlays.new_project = false,
                                    Err(err) => app.editor.status = err,
                                }
                            }
                        }
                    });
            },
            |ui| {
                if kit::secondary_button(ui, "Browse for Project...", ui.available_width()).clicked()
                {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        match app.editor.open_project(folder) {
                            Ok(_) => app.editor.overlays.new_project = false,
                            Err(err) => app.editor.status = err,
                        }
                    }
                }
            },
        );
    });
}
```

That version gives you:

* fixed header,
* padded modal body,
* two-column layout,
* left scrollable form,
* left bottom-pinned primary footer,
* right scrollable recent-project list,
* right bottom-pinned secondary button,
* no `available_height() - 48.0`,
* no `bottom_up` mixed with a scroll area,
* no manual left/footer `Rect` split for ordinary form layout.

---

## Branch-specific shell migration for 0.34.x

Your branch already implements `eframe::App for NlaEguiApp` with `fn ui(&mut self, ui: &mut Ui, ...)`, but it immediately clones `ctx` and calls `self.top_bar(&ctx)`, `self.left_panel(&ctx)`, `self.right_panel(&ctx)`, etc.  That works but keeps you in deprecated 0.34 panel style.

Target shape:

```rust
impl eframe::App for NlaEguiApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        self.poll_automation();
        self.keep_automation_responsive(&ctx);
        self.tick_playback(&ctx);
        self.update_preview_texture(&ctx);

        egui::Panel::top("top_bar")
            .exact_size(kit::TOP_BAR_H)
            .frame(kit::chrome_frame())
            .show_inside(ui, |ui| self.top_bar_contents(ui));

        self.left_panel_inside(ui);
        self.right_panel_inside(ui);

        egui::Panel::bottom("status")
            .exact_size(kit::STATUS_BAR_H)
            .frame(kit::chrome_frame())
            .show_inside(ui, |ui| self.status_bar_contents(ui));

        self.timeline_panel_inside(ui);

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(kit::PANEL_SUNKEN))
            .show_inside(ui, |ui| self.central_preview_contents(ui));

        self.modals(&ctx);
    }
}
```

The 0.34 `Panel` docs show the new unified `Panel::{left,right,top,bottom}` API, warn that panel order matters, and say `CentralPanel` should be added last. ([Docs.rs][9]) The old width/height-specific names are deprecated: use `default_size`, `min_size`, `max_size`, `size_range`, and `exact_size` instead of `default_width`, `height_range`, `exact_height`, etc.; `Panel::show(ctx, ...)` is also deprecated in favor of `show_inside(ui, ...)`. ([Docs.rs][9])

---

## Pitfalls and debugging checklist

### Common anti-patterns

| Anti-pattern                                                           | Why it breaks                                                                                           | Replacement                                                                                            |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `ScrollArea::vertical().show(ui, ...)` followed by footer              | The scroll area may consume the remaining region; footer placement depends on content/available height. | Put body and footer in a vertical `StripBuilder`.                                                      |
| `ScrollArea::max_height(ui.available_height() - 48.0)`                 | The `48.0` is a guessed reservation. Style/font/button height changes break it.                         | Use an exact footer cell and let the body be `Size::remainder()`.                                      |
| `Layout::bottom_up` around both scroll body and footer                 | It inverts cursor semantics and can behave badly with scroll/input patterns.                            | Use `StripBuilder` and render footer in its own cell.                                                  |
| `ui.add_space(ui.available_height() - footer_h)`                       | Negative/near-zero values, double spacing, and layout-direction surprises.                              | `StripBuilder` or manual rect split.                                                                   |
| `with_layout` as if it were a small inline row                         | Docs say the new layout takes all available space.                                                      | Use `horizontal`, `vertical`, or `allocate_ui_with_layout`. ([Docs.rs][5])                             |
| Manual `new_child(UiBuilder::max_rect(...))` without parent allocation | Parent cursor may not know about the child.                                                             | Allocate a parent rect first, or use `scope_builder`.                                                  |
| Hardcoded row heights sprinkled everywhere                             | DPI/font/style changes create clipping.                                                                 | Use `ui.spacing().interact_size.y`, `ui.text_style_height`, and centralized constants.                 |
| Resizable panel with content that does not fill                        | Dragging appears broken or leaves blank/unclaimed space.                                                | Use `ui.take_available_space()`, a scroll area, wrapping text, separator, or text edit. ([Docs.rs][9]) |
| Manual painter widgets without clipping                                | Paint bleeds outside cells/cards.                                                                       | Allocate a rect, use `ui.painter_at(rect)`, or `StripBuilder::clip(true)`.                             |

### Debugging checklist

When a layout clips, overflows, or misaligns:

1. Print or temporarily paint `ui.available_rect_before_wrap()`, `ui.min_rect()`, `ui.max_rect()`, and every important `response.rect`.
2. Check whether the parent is bounded. A scroll area in an unbounded vertical layout cannot know what “fill the middle” means.
3. Confirm panel order: top/left/right/bottom panels first, central panel last, windows/modals after panels. ([Docs.rs][2])
4. Check whether `auto_shrink(true)` is shrinking a scroll body that should fill.
5. Check whether a `with_layout` call is taking all available space.
6. Check whether `add_space` is adding on top of `item_spacing`.
7. Replace guessed constants like `48.0` with measured/semantic values from `LayoutMetrics`.
8. For repeated widgets, add `ui.push_id(...)` or `id_salt(...)` where needed; egui docs show `push_id` to avoid ID clashes in repeated UI. ([Docs.rs][5])
9. For huge lists, switch from drawing all rows to `ScrollArea::show_rows`.

---

## Version-specific notes for egui/eframe 0.34.x

* **`eframe::App::ui` is the new main UI entrypoint.** The docs show `fn ui(&mut self, ui: &mut Ui, frame: &mut Frame)` as the required method; `update` is deprecated. The root `Ui` has no margin/background, so wrap shell content in panels or a central frame. ([Docs.rs][10])
* **Use `Ui` more, naked `Context` less.** The 0.34 changelog says egui moved from `Context` as the main entrypoint toward whole-app `Ui`, with `Ui` derefing to `Context`. ([GitHub][1])
* **`SidePanel` and `TopBottomPanel` are deprecated aliases.** Use `egui::Panel::left/right/top/bottom`. ([Docs.rs][2])
* **Panel sizing method names were unified.** Use `default_size`, `min_size`, `max_size`, `size_range`, `exact_size`; the old width/height-specific methods are deprecated. ([Docs.rs][9])
* **Top-level `Panel::show(ctx, ...)` is deprecated.** Use `show_inside(ui, ...)` from the root `App::ui` `Ui`. ([Docs.rs][9])
* **`CentralPanel::show` is also deprecated in the same direction.** Prefer `show_inside(ui, ...)` when you are in `App::ui`.
* **`UiBuilder` is the modern way to configure child UIs.** `max_rect` constrains where widgets try to fit; `sizing_pass` exists for special one-frame sizing; `scope_builder` is the current child-UI wrapper that allocates used space in the parent. ([Docs.rs][6])
* **`ScrollArea::content_margin` was added in 0.34**, which is useful for scrollable modal bodies where you want padding without manually wrapping every row. ([GitHub][1])
* **`id_source` is renamed to `id_salt`** in scroll areas and other APIs; prefer the new name. ([Docs.rs][4])

The practical migration path for your branch is not to rewrite everything at once. First, add `egui_extras`, introduce `body_with_footer`, and replace the modal/list/footer layouts. Then migrate the app shell from deprecated panel calls to `Panel::show_inside`. Finally, move repeated height constants into `ui_kit` metrics so all forms/cards/action bars share the same spacing vocabulary.

[1]: https://raw.githubusercontent.com/emilk/egui/master/CHANGELOG.md "https://raw.githubusercontent.com/emilk/egui/master/CHANGELOG.md"
[2]: https://docs.rs/egui/latest/egui/containers/panel/index.html "https://docs.rs/egui/latest/egui/containers/panel/index.html"
[3]: https://docs.rs/egui_extras/latest/egui_extras/struct.StripBuilder.html "https://docs.rs/egui_extras/latest/egui_extras/struct.StripBuilder.html"
[4]: https://docs.rs/egui/0.34.1/egui/containers/scroll_area/struct.ScrollArea.html "https://docs.rs/egui/0.34.1/egui/containers/scroll_area/struct.ScrollArea.html"
[5]: https://docs.rs/egui/latest/egui/struct.Ui.html "https://docs.rs/egui/latest/egui/struct.Ui.html"
[6]: https://docs.rs/egui/latest/egui/struct.UiBuilder.html "https://docs.rs/egui/latest/egui/struct.UiBuilder.html"
[7]: https://docs.rs/egui_extras/latest/egui_extras/enum.Size.html "https://docs.rs/egui_extras/latest/egui_extras/enum.Size.html"
[8]: https://docs.rs/egui/latest/egui/containers/modal/struct.Modal.html "https://docs.rs/egui/latest/egui/containers/modal/struct.Modal.html"
[9]: https://docs.rs/egui/latest/egui/containers/panel/struct.Panel.html "https://docs.rs/egui/latest/egui/containers/panel/struct.Panel.html"
[10]: https://docs.rs/eframe/0.34.1/eframe/trait.App.html "https://docs.rs/eframe/0.34.1/eframe/trait.App.html"
