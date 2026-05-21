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

Use it for asset panels, inspectors, modal columns, provider lists, and queue panels. Inside the body cell, use the shared scroll helper so the parent first allocates an exact viewport and then clips the scroll content:

```rust
kit::scroll_body(ui, |ui| {
    // Long form/list content.
});
```

The important parts are the exact parent rect, the scroll area's `max_width`, and the inner UI width clamp. `auto_shrink([false, false])` keeps blank vertical space **inside** the scroll body, but it is not enough by itself: a vertical scroll area still has a disabled horizontal axis, and wide descendants can otherwise make the scroll response/content width leak outside a side rail.

The shared helper also routes the scroll area through an exact-rect child UI. A raw `ScrollArea::vertical().show(ui, ...)` in a modal column can still paint text past the visual body if the parent cell was not bounded first. Treat scroll panes as hard clipped viewports before drawing cards, rows, or long form fields inside them.

Egui 0.34 paints scroll-edge fade gradients by default through `style.spacing.scroll.fade`. The shared app scroll helper disables those fades for editor-pane bodies because inspectors, provider forms, and asset lists should read as recessed clipped surfaces, not blurred/faded web panels.

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

`auto_shrink([false, false])` is one part of “fill this bounded cell.” With `true`, blank space goes outside the scroll area. With `false`, blank space goes inside it. However, do not use a raw vertical scroll area in sidebars or modals. The helper must also cap the outer width and set the content UI width so long fields/cards clip instead of expanding the panel:

```rust
kit::clipped_scroll_body(ui, "inspector-scroll", |ui| {
    // form/list rows
});
```

For huge lists, prefer `show_rows`; the docs show measuring row height via `ui.text_style_height(&TextStyle::Body)` for labels or `ui.spacing().interact_size.y` for button rows. ([Docs.rs][4])

---

## Reusable `ui_kit` layout templates

Your current `ui_kit` centralizes colors, frames, margins, button drawing, modal headers, card frames, row painting, field rows, browse fields, and modal shell styling. Keep pushing fixes down into these primitives. If a visual issue appears in two places, treat it as a kit bug until proven otherwise.

### Resizable panel containment

Resizable `egui::Panel` stores the rendered panel frame rect as its next size. That means content can accidentally resize a side panel if any descendant measures wider than the current panel. The common failure mode is a slow side-panel creep during mouse movement, because pointer motion triggers repaint and the oversized content rect feeds back into the next frame.

For side panels, route contents through an exact viewport helper such as `fixed_panel_body` before rendering cards, scroll areas, and full-width rows. Inside that fixed body, children can use `ui.available_width()` safely because their desired width no longer expands the parent panel state. Prefer exact-allocation cards/rows inside resizable panels; avoid content-sized `Frame::show` wrappers for full-width panel sections unless they are contained by an exact parent rect.

For fixed-count action rows in narrow panels, allocate one exact row rect first, compute gaps and cell widths from that rect, then render each child into exact sub-rects. Do not let pill/button rows depend on each button's natural minimum width; labels can truncate before the row is allowed to overflow or resize its parent.

When a card height is token-derived, disable implicit vertical `item_spacing.y` inside that card and use explicit token gaps only. Otherwise egui adds default item spacing on top of `add_space(...)`, and a mathematically correct fixed-height card can still overflow by several pixels.

### Color discipline

Visible UI primitives should use semantic `ui_kit` tokens rather than direct `Color32` literals. Avoid pure black or pure white for field fills, text, strokes, panels, and buttons; use tinted near-black and near-white tokens such as `FIELD_BG`, `FIELD_BG_HOVER`, `FIELD_BG_ACTIVE`, `TEXT`, and `TEXT_ON_ACCENT`. Editable text fields, read-only value fields, and numeric `DragValue` fields should share the same field surface tokens so they read as one control family.

### Metric discipline

Every control family needs semantic size tokens. Do not let individual widgets invent their own heights, radii, padding, text size, or inter-control gaps.

Current control families:

- `FIELD_H`, `TEXT_FIELD_H`, and `VALUE_FIELD_H` define the normal form-field row height. Editable text fields, read-only value boxes, browse path fields, and numeric value fields should all start here.
- `STANDALONE_BUTTON_H`, `PRIMARY_BUTTON_H`, and `SECONDARY_BUTTON_H` define standalone action buttons. A primary action may change color, not shape, unless a new named button variant is deliberately added.
- `FIELD_COMPOUND_GAP` defines the gap between a field and its attached action, such as path field + Browse.
- `CLOSE_BUTTON_SIZE`, `CLOSE_BUTTON_RADIUS`, and modal close insets define the square modal close hit target. Close buttons are icon controls, not tiny text labels.

If a future screen needs compact controls, add a named compact family such as `COMPACT_FIELD_H` or `TOOLBAR_BUTTON_H`. Do not locally shave 2-4 pixels off the standard controls.

### Field family rules

Text fields, read-only value fields, browse path fields, and numeric `DragValue` fields should look like variations of the same field primitive:

- same outer height,
- same radius,
- same default fill,
- same hover/focus border behavior,
- same text size,
- same global text alignment token until the product intentionally changes it.

Functional differences should stay inside the component. For example, numeric fields can keep drag-to-adjust and prefix labels, but the surface should not look like a different button family. Path browse fields are field-first controls: the text/path field flexes, the Browse button keeps a fixed width, and the whole compound row uses field height.

First-focus selection is also a field-family behavior. A single-line editable field should select all text on first focus, then allow normal cursor placement on subsequent interaction. Browse path fields and read-only value fields should still show the same focus/edge affordance where they are interactive or selectable.

### Button family rules

Separate button role from button geometry:

- Standalone buttons share height, radius, typography, and horizontal padding.
- Primary/action buttons use a different skin, not a different size.
- Field-attached buttons, such as Browse, use field height because they belong to a compound field.
- Icon buttons and modal close buttons have their own square metrics and should paint actual icons or strokes, not text glyphs pretending to be icons.

This lets the product have reusable variants such as primary, secondary, danger, field-attached, icon, and close without every use site becoming a one-off.

### List and inspector templates

Main-shell lists and inspectors should use reusable row/card templates, not local one-off layout. Asset rows are a good pattern: one fixed row height token, one thumbnail/icon region, one text column with truncation, and one semantic accent color derived from asset kind. The same row painter should handle selected, hovered, and normal states so every asset-like list can inherit future row improvements.

Inspector panels should be built from section cards with shared field labels, field-height controls, two-column numeric helpers, and small metadata rows. If a clip, asset, marker, or track inspector needs a new control, first ask whether it is a field, value field, numeric drag field, metadata row, action button, or preview thumbnail. Add a named helper only when none of those fits.

Dropdowns/selectors belong to the same field family. A version picker, provider picker, seed strategy selector, or schema enum input should use a field-height combo helper with the same fill, radius, focus stroke, text sizing, and clipping as text/value fields. Provider-driven inspectors should be generated from the provider schema into shared field primitives rather than hand-building a separate form style for each provider.

Media thumbnails should be treated as a first-class UI primitive. Prefer cached project thumbnails for videos and generated assets, fall back to the source image for stills, and only fall back to text badges when no previewable image exists. Clear thumbnail/preview caches when the project changes so stale textures do not leak across projects.

### 1. Metrics helper

```rust
pub struct LayoutMetrics {
    pub gap_x: f32,
    pub gap_y: f32,
    pub row_h: f32,
    pub body_text_h: f32,
    pub field_h: f32,
    pub standalone_button_h: f32,
    pub action_bar_h: f32,
}

impl LayoutMetrics {
    pub fn from_ui(ui: &egui::Ui) -> Self {
        let spacing = ui.spacing();
        let row_h = spacing.interact_size.y;
        let field_h = kit::FIELD_H;
        let standalone_button_h = kit::STANDALONE_BUTTON_H;

        Self {
            gap_x: spacing.item_spacing.x,
            gap_y: spacing.item_spacing.y,
            row_h,
            body_text_h: ui.text_style_height(&egui::TextStyle::Body),
            field_h,
            standalone_button_h,
            action_bar_h: standalone_button_h + spacing.item_spacing.y * 2.0,
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

### Modal surface and corner rules

Do not rely on a rounded parent `Frame` to clip child paints. egui generally paints what each child asks it to paint; a square header or body can visually poke through a rounded outer modal frame. Any stacked surface that has rounded outer corners needs its child bands to carry compatible per-corner radii.

For the current modal shell:

- `modal_frame()` owns the outer fill, stroke, radius, and shadow.
- `modal_header_with_close()` paints the header with top-only radii.
- `modal_body()` paints the body with bottom-only radii.
- Floating utility windows that reuse modal header/body helpers inherit the same fix.

Use the same rule for future card sections, popovers, tab containers, and tool palettes. If a child band touches an outer rounded edge, give that child the matching partial radius. If a child band is fully internal, keep it square so stacked sections meet cleanly.

### Modal backdrop rules

There is no CSS-style `backdrop-filter: blur(...)` primitive in egui. A true full-window backdrop blur requires rendering the already-drawn app into an offscreen texture, blurring it, drawing it back, and then drawing the modal. Treat that as a rendering feature, not as ordinary UI layout polish.

The current default should be a faux-blur/depth treatment in `ui_kit`:

- layered tinted scrim,
- subtle edge vignette,
- modal drop shadow,
- no pure black overlay,
- all values tokenized as modal backdrop/surface constants.

Only blur owned textures directly when the scope is narrow and obvious, such as dimming or blurring the preview texture while a modal is open. Do not build a separate modal-by-modal blur path.

### Before: fragile modal body pattern

This is the kind of historical pattern to retire. The earlier New Project modal had manual left/right widths, fixed card heights, `allocate_ui_with_layout`, a recent-project list with `max_height(ui.available_height() - 48.0)`, `Layout::bottom_up` for a bottom button, and a left form split by manual `Rect`s.

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

### Warning hygiene

Keep `cargo check` warning-free. Deprecation warnings should be treated as migration cleanup, not accepted background noise. Dead-code warnings need triage: reconnect the path, remove it if obsolete, or add a narrow `#[allow(dead_code)]` with a comment explaining the dormant subsystem and the condition for reconnecting it. Avoid crate-wide dead-code suppression because it hides real regressions during this refactor.

### Modal editor flows

Do not compress complex editor surfaces into inspector previews. A management modal should list/select entities and then hand off to a purpose-built editor surface when editing needs depth. The AI Providers flow is the current pattern: a compact provider list with equal-width top actions, a selection hub with explicit editor choices, a wide JSON editor for raw structured text, and a wide builder modal for workflow/node/input editing.

- Keep chooser/list modals narrow enough to scan, but make editing modals as wide/tall as their task requires.
- Use fixed left rails plus remainder editor bodies with `StripBuilder`; do not let text content determine the width of a JSON/code panel.
- Keep text editors in their own allocated body cell and action buttons in a footer cell so text cursor focus cannot steal clicks from Save/Cancel/Edit controls.
- Prefer shared file/folder browse fields for workflow and manifest paths, with JSON extension filters and remembered directories where repeated picking is expected.
- Validate structured text before writing it, then format it with `serde_json::to_string_pretty` so user-edited JSON returns to a stable shape.

### Narrow form degradation

Repeated field grids should keep their outer allocation inside the parent card before protecting inner text. A field overflowing a rounded card is worse than a truncated prefix/value inside a correctly bounded field. Use shared pair/grid helpers with an explicit minimum-column option: set the minimum to zero for inspectors and other resizable sidebars, and reserve nonzero minimums only for modal forms where horizontal scrolling or a wider modal is intentional.

- Allocate row width from `ui.available_width()` and split that exact rect; do not let per-column minimums expand the parent row by accident.
- Shrink child UI clip rects for painted/custom fields so text edits, `DragValue` prefixes, and read-only values cannot draw outside their field box or the parent scroll viewport. In nested/scrollable UI, use `shrink_clip_rect(rect)` rather than `set_clip_rect(rect)`; `set_clip_rect` can grow past the inherited scroll clip when the field's rect is scrolled partially offscreen.
- Prefer truncation/hover text for long display values, and keep full editability by allowing focus/selection inside the clipped field.

### Subtle chrome controls

Dense editor chrome should not look like a wall of standalone form buttons. For top-bar menus, timeline transport, view toggles, and compact utility chips, use the shared subtle button substrate: transparent inactive fill, visible hover fill, active/open fill, and tokenized size variants. Keep this separate from primary/secondary form buttons, which are meant to draw attention inside cards and modals.

- Top-bar menu triggers can be custom popup-backed subtle buttons so popup menu contents keep normal menu styling.
- Timeline controls and app chrome controls may share paint behavior while using different height/text-size/radius constants.
- Active or open state should be visible even when the pointer is not hovering.
- Dense icon controls should prefer geometric/icon drawing over raw symbol-font glyphs when vertical precision matters. Glyph bounding boxes and baselines can make play/pause/caret controls look clipped or off-center even when the button rect is correct.

### Button-like rows

Rows that behave like buttons should not contain nested selectable text widgets. Allocate the row once, paint the row fill/stroke/accent, paint truncated text directly into the row rect, and return one click response with a pointing-hand cursor. Use real `Label`/`TextEdit` widgets only when text selection or editing is intended. If a row can also start drag/drop, keep it as the same row primitive with `Sense::click_and_drag()` and attach a typed egui drag payload at the call site rather than adding a separate hidden drag handle.

### Shell panel borders

Top-level shell panels should not use all-edge `Frame::stroke` borders when they touch other shell panels. That doubles adjacent one-pixel strokes and makes the editor feel over-lined. Shell frames should own fill and padding only; paint explicit one-pixel separators on the single edge that defines the layout boundary:

- top app chrome owns its bottom edge,
- bottom status chrome owns its top edge,
- left docks own their right edge,
- right docks own their left edge,
- timeline owns its top edge.

Cards, modals, fields, and inner preview/timeline canvases can still use full strokes because they are contained surfaces rather than app-shell dividers.

### Preview surfaces

The Preview panel should have one body surface and one rendered canvas/texture. Avoid adding an extra stroked plate between the panel body and the preview texture unless it communicates a real editable boundary. The preview body fill is the neutral stage; the rendered frame should be clipped to the padded body rect and letterbox naturally against that stage. Do not bake UI borders into the preview image bytes; if the canvas needs an outline later, draw it consistently in UI space on all four sides.

## Precision Timeline Surfaces

Timeline editors are not normal form layouts. Treat them as precision canvases with explicit geometry, not as a row of flexible widgets.

- Use a single time-space mapping: `x = left + time_seconds * pixels_per_second - scroll_x`. Do not stretch timeline duration to the visible width except when computing the Fit zoom value.
- Store zoom and horizontal scroll as explicit state. Clip width, clip position, ruler ticks, playhead position, marker position, thumbnails, and waveform columns must all derive from the same zoom/scroll values.
- Keep fixed labels and scrollable track content as separate regions. The label column should never be part of horizontal scroll; the ruler and track content should share the same scroll coordinate system.
- Clip the scrollable timeline painter to the right-side content viewport. Offscreen clips, thumbnails, waveforms, ruler grid lines, markers, and playhead overlays must never paint into fixed track labels, footer buttons, or the header controls.
- Hit-test direct manipulation in priority order: clip edge resize, clip body move, marker move/select, ruler seek, track-label select, empty-track deselect. Avoid a single catch-all timeline drag that turns every click into playhead scrubbing.
- Keep marker hit targets aligned with their visible affordance. The stem, marker head, and label bubble should all behave like one draggable timeline item so switching directly from one marker to another does not require a second click.
- Use `Response::total_drag_delta()` for interactions computed from a drag-start baseline. `Response::drag_delta()` is movement since the previous frame in egui 0.34, so baseline math using it will appear to move only while the pointer is moving and then snap back.
- Snap in frame units, not pixels. Convert sources and targets to frames, apply snap deltas, then convert back to seconds. Alt is the timeline precision override and should bypass clip, marker, and playhead boundary snapping while preserving frame-quantized time math.
- Do not use selection green as a media type color. Timeline items should use neutral fills by default, subdued type-color outlines/stripes when unselected, and green focus/selection treatment only when selected.
- Treat colors and long text as first-class field types. A color field should be a field-height swatch backed by egui's popup color picker while project storage can remain hex; descriptions/notes/prompts should use the shared configurable multi-line text field instead of squeezing into single-line inputs.
- Draw frame ticks only when the zoom level gives them enough pixel separation to be readable. The frame ticks are a precision affordance, not decoration.
- Treat timeline headers as explicit regions, not one long horizontal row: left-side zoom/view tools, centered transport controls, and right-side timecode/collapse actions should occupy stable rects so one group cannot push another off balance.
- Timeline toolbar controls should be subtle by default. Use transparent icon/text buttons with hover and active fills instead of full standalone button chrome, and keep their height tied to the timeline header channel rather than the global form button height.
- Media previews on timeline clips should be clipped to the clip rect. Thumbnail strips and waveform strokes must not bleed through rounded clip corners or track dividers.
- Timeline thumbnail strips should sample the media cache by source time for each tile. Repeating a single first-frame texture is only an explicit fallback when a time-specific thumbnail is unavailable.
- Shift+wheel scrolling is platform-sensitive: handle both raw shifted wheel `y` deltas and horizontal `x` deltas from trackpads or OS-level shifted wheel translation. Egui wheel deltas describe content movement, so invert them when updating a viewport scroll offset: wheel down should increase timeline `scroll_x` and reveal later time to the right.
- Long-running media analysis, such as waveform peak cache generation, should not block the paint pass. Load existing caches synchronously when cheap, then queue background generation and repaint when the cache appears.
- Audio playback engines that own native device streams should stay on the UI/runtime owner thread unless the type is explicitly Send/Sync. Background workers can decode into shared caches; the UI thread should rebuild playback items and update the engine.
- Playback decode failures should be cached for the current project session. Silent videos on video tracks are valid visual clips, not actionable repeated audio errors; log once, mark them as unavailable for playback audio, and keep the timeline moving.

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
| Square child fills inside rounded frames                               | Header/body/card-section paint can poke through rounded parent corners.                                 | Give edge-touching child bands compatible partial radii.                                               |

### Debugging checklist

When a layout clips, overflows, or misaligns:

1. Print or temporarily paint `ui.available_rect_before_wrap()`, `ui.min_rect()`, `ui.max_rect()`, and every important `response.rect`.
2. Check whether the parent is bounded. A scroll area in an unbounded vertical layout cannot know what “fill the middle” means.
3. Confirm panel order: app-wide top/bottom chrome first, docked side panels next, center-scoped timeline/footer panels after side panels, central panel last, windows/modals after panels. A bottom panel registered after side panels will only span the remaining center region. ([Docs.rs][2])
4. Check whether `auto_shrink(true)` is shrinking a scroll body that should fill.
5. Check whether a `with_layout` call is taking all available space.
6. Check whether `add_space` is adding on top of `item_spacing`.
7. Inspect every visible edge at modal/card/popup corners. If a child band touches a rounded outer edge, it needs compatible partial radii because the parent frame will not automatically hide square child paint.
8. Compare every repeated control against its family tokens: fields vs standalone buttons vs field-attached buttons vs icon controls. If two controls serve the same family role but differ by 1-4 pixels, fix the shared primitive.
9. Verify default, hover, focus, active, disabled, and selected states for the shared primitive, not just the instance that exposed the bug.
10. Replace guessed constants like `48.0` with measured/semantic values from `LayoutMetrics` or exported `ui_kit` constants.
11. For repeated widgets, add `ui.push_id(...)` or `id_salt(...)` where needed; egui docs show `push_id` to avoid ID clashes in repeated UI. ([Docs.rs][5])
12. For huge lists, switch from drawing all rows to `ScrollArea::show_rows`.

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

The practical migration path for your branch is not to rewrite everything at once. `egui_extras`, `body_with_footer`, shared field metrics, browse fields, modal close controls, and modal surface tokens are now established. Next, migrate the app shell from deprecated panel calls to `Panel::show_inside`, build a reusable inspector/form grid on the same token vocabulary, and keep moving any repeated styling defect down into `ui_kit` before polishing individual call sites.

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
