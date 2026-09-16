use super::*;
use crate::core::media_binding::{
    lookup_media_binding, materialize_plan, prepare_reference_image, resolve_generation_context,
    resolve_media_binding, MediaResolveContext,
};
use crate::state::{
    AssetLabAuthoringProfile, AssetLabMask, AssetLabMaskGeometry, AssetLabRegion, ReferenceSizing,
};
use image::GrayImage;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tool {
    #[default]
    Select,
    Paint,
    Erase,
    Object,
    Text,
}

#[derive(Clone)]
pub(super) struct CanvasState {
    key: String,
    base: Option<(TextureHandle, Vec2)>,
    geometry: Option<AssetLabMaskGeometry>,
    error: Option<String>,
    pixels: Option<GrayImage>,
    mask_path: Option<PathBuf>,
    overlay: Option<TextureHandle>,
    overlay_enabled: bool,
    draft: Option<AssetLabSnapshot>,
    tool: Tool,
    brush: f32,
    pub(super) selected: Option<Uuid>,
    gesture: Option<Gesture>,
}

impl std::fmt::Debug for CanvasState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasState")
            .field("key", &self.key)
            .field("tool", &self.tool)
            .finish()
    }
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            key: String::new(),
            base: None,
            geometry: None,
            error: None,
            pixels: None,
            mask_path: None,
            overlay: None,
            overlay_enabled: true,
            draft: None,
            tool: Tool::Select,
            brush: 48.0,
            selected: None,
            gesture: None,
        }
    }
}

#[derive(Clone, Debug)]
enum Gesture {
    Stroke {
        last: Pos2,
    },
    Region {
        start: Pos2,
        original: [f32; 4],
        id: Uuid,
        resize: bool,
        creating: bool,
    },
}

impl LatentSlateApp {
    fn prepare_lab_mask_canvas(
        &self,
        config: &GenerativeConfig,
        provider: &ProviderEntry,
        asset: &Asset,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
    ) {
        let result = (|| {
            let field = provider
                .inputs
                .iter()
                .find(|field| {
                    crate::core::media_binding::bound_media_type_for_input(field)
                        == Some(crate::state::BoundMediaType::Image)
                })
                .ok_or("Choose an image input to paint against.")?;
            let binding = lookup_media_binding(config, field, &self.editor.project)
                .ok_or("Choose an edit-base image in the first image slot.")?;
            let context = resolve_generation_context(
                &self.editor.project,
                asset.id,
                None,
                self.generation_context_by_asset.get(&asset.id).copied(),
            )
            .map_err(|e| e.message(&field.label))?;
            let plan = resolve_media_binding(
                MediaResolveContext {
                    project: &self.editor.project,
                    target_asset_id: Some(asset.id),
                    context_clip_id: context,
                    field,
                    provider: Some(provider),
                    config: Some(config),
                },
                &binding,
            );
            if !plan.is_ok() {
                return Err(plan
                    .primary_error_message()
                    .unwrap_or("Input is unresolved".into()));
            }
            let source = plan
                .source_path_absolute
                .as_ref()
                .ok_or("Source is missing")?;
            let metadata = std::fs::metadata(source).map_err(|e| e.to_string())?;
            let values = config
                .inputs
                .iter()
                .filter_map(|(name, value)| {
                    if let InputValue::Literal { value } = value {
                        Some((name.clone(), value.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            let extent = crate::core::generation::effective_canvas_dimensions(provider, &values);
            let sizing = config
                .reference_sizing
                .get(&field.name)
                .copied()
                .unwrap_or_default();
            let key = format!(
                "{source:?}:{:?}:{:?}:{:?}:{extent:?}:{sizing:?}",
                metadata.modified().ok(),
                metadata.len(),
                (
                    &plan.normalized_sample,
                    plan.source_frame_time,
                    plan.source_range
                )
            );
            if canvas.key == key {
                return Ok(());
            }
            let materialized = materialize_plan(&self.editor.project, &plan)
                .map_err(|e| e.message(&field.label))?;
            let dimensions = image::image_dimensions(&materialized).map_err(|e| e.to_string())?;
            let (width, height) = extent
                .map(|(w, h)| (w.max(1) as u32, h.max(1) as u32))
                .unwrap_or(dimensions);
            if sizing == ReferenceSizing::Exact && dimensions != (width, height) {
                return Err(format!("Edit source is {} × {}; canvas is {width} × {height}. Configure reference sizing or match the canvas before painting.", dimensions.0, dimensions.1));
            }
            let prepared = prepare_reference_image(
                &self.editor.project,
                &materialized,
                width,
                height,
                sizing,
            )?;
            let bytes = std::fs::read(&prepared).map_err(|e| e.to_string())?;
            let identity = format!("{:x}", Sha256::digest(&bytes));
            let image = image::load_from_memory(&bytes)
                .map_err(|e| e.to_string())?
                .to_rgba8();
            let root = self
                .editor
                .project
                .project_path
                .as_ref()
                .ok_or("Save the project first")?;
            let resolved = plan
                .to_resolved(
                    materialized
                        .strip_prefix(root)
                        .unwrap_or(&materialized)
                        .to_path_buf(),
                )
                .ok_or("Could not resolve the edit image")?;
            canvas.base = Some((
                ctx.load_texture(
                    "lab_edit_base",
                    ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize],
                        image.as_raw(),
                    ),
                    egui::TextureOptions::LINEAR,
                ),
                Vec2::new(width as f32, height as f32),
            ));
            canvas.geometry = Some(AssetLabMaskGeometry {
                input_field: field.name.clone(),
                width,
                height,
                base: resolved,
                sizing,
                source_identity: identity,
            });
            canvas.key = key;
            Ok::<_, String>(())
        })();
        canvas.error = result.err();
        if canvas.error.is_some() {
            canvas.base = None;
            canvas.geometry = None;
            canvas.key.clear();
        }
    }

    pub(in crate::egui_app::asset_lab) fn asset_lab_authoring_canvas(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: &GenerativeConfig,
        provider: Option<&ProviderEntry>,
    ) {
        let profile = provider
            .map(crate::state::asset_lab_authoring_profile)
            .unwrap_or(AssetLabAuthoringProfile::Generic);
        let mut canvas = std::mem::take(&mut self.asset_lab.v4.canvas);
        if profile == AssetLabAuthoringProfile::Mask
            && !matches!(canvas.tool, Tool::Paint | Tool::Erase)
        {
            canvas.tool = Tool::Paint;
        }
        if profile == AssetLabAuthoringProfile::Regions
            && matches!(canvas.tool, Tool::Paint | Tool::Erase)
        {
            canvas.tool = Tool::Select;
        }
        let audition = self.asset_lab.v4.preview.clone();
        let mut setup = canvas
            .draft
            .clone()
            .unwrap_or_else(|| AssetLabSnapshot::from_config(config));
        if profile == AssetLabAuthoringProfile::Mask && audition.is_none() {
            if let Some(provider) = provider {
                self.prepare_lab_mask_canvas(config, provider, asset, ui.ctx(), &mut canvas);
            }
        }
        if let Some(mask) = &config.lab_authoring.mask {
            if canvas.mask_path.as_ref() != Some(&mask.path) {
                canvas.pixels = self
                    .editor
                    .project
                    .project_path
                    .as_ref()
                    .and_then(|root| image::open(root.join(&mask.path)).ok())
                    .map(|image| image.to_luma8());
                canvas.mask_path = Some(mask.path.clone());
                canvas.overlay = None;
            }
        } else if canvas.mask_path.is_some() {
            canvas.pixels = None;
            canvas.mask_path = None;
            canvas.overlay = None;
        }
        let aligned = config.lab_authoring.mask.as_ref().is_none_or(|mask| {
            canvas.pixels.as_ref().is_some_and(|pixels| {
                pixels.dimensions() == (mask.geometry.width, mask.geometry.height)
            }) && canvas
                .geometry
                .as_ref()
                .is_some_and(|geometry| mask_geometry_matches(&mask.geometry, geometry))
        });
        let mut clear = false;
        let shortcuts_active = self.asset_lab.v4.view == AssetLabView::Create
            && audition.is_none()
            && self.source_picker.is_none()
            && self.asset_lab.v4.pending_adopt.is_none()
            && !ui.ctx().text_edit_focused()
            && !ui.ctx().any_popup_open()
            && !self.asset_lab.v4.interacting
            && canvas.gesture.is_none()
            && ui.is_enabled()
            && ui.input(|i| i.focused && !i.pointer.any_down());
        let mut undo = shortcuts_active
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z));
        if shortcuts_active && profile == AssetLabAuthoringProfile::Mask {
            ui.input_mut(|input| {
                if input.modifiers != egui::Modifiers::NONE {
                    return;
                }
                for (key, tool) in [(egui::Key::B, Tool::Paint), (egui::Key::E, Tool::Erase)] {
                    if input.consume_key(egui::Modifiers::NONE, key) {
                        canvas.tool = tool;
                        self.asset_lab.v4.mask_visible = true;
                    }
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::OpenBracket) {
                    canvas.brush = (canvas.brush * 0.8)
                        .round()
                        .min(canvas.brush - 1.0)
                        .clamp(1.0, 512.0);
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::CloseBracket) {
                    canvas.brush = (canvas.brush * 1.25)
                        .round()
                        .max(canvas.brush + 1.0)
                        .clamp(1.0, 512.0);
                }
            });
        }
        let message = if profile == AssetLabAuthoringProfile::Mask && audition.is_none() {
            canvas.error.clone().or_else(|| (!aligned).then(||
                "Mask unavailable or aligned to a different source/canvas. Restore its artifact and matching source, or explicitly clear the mask to paint here.".to_owned()))
        } else {
            None
        };
        let mut preview = if let Some(version) = &audition {
            self.asset_lab_preview_texture(ui.ctx(), asset, Some(version))
        } else if profile == AssetLabAuthoringProfile::Mask {
            canvas
                .base
                .as_ref()
                .map(|(texture, size)| (texture.id(), *size))
        } else if profile != AssetLabAuthoringProfile::Regions || self.asset_lab.v4.guide_visible {
            self.asset_lab_preview_texture(
                ui.ctx(),
                asset,
                config.lab_authoring.working_version.as_deref(),
            )
        } else {
            None
        };
        let mut extent = preview.map(|(_, size)| size).unwrap_or(Vec2::splat(1024.0));
        if profile == AssetLabAuthoringProfile::Regions && audition.is_none() {
            if let Some(provider) = provider {
                let values = config
                    .inputs
                    .iter()
                    .filter_map(|(name, value)| {
                        if let InputValue::Literal { value } = value {
                            Some((name.clone(), value.clone()))
                        } else {
                            None
                        }
                    })
                    .collect();
                if let Some((w, h)) =
                    crate::core::generation::effective_canvas_dimensions(provider, &values)
                {
                    extent = Vec2::new(w as f32, h as f32);
                }
            }
            // A guide is presentation only; it never changes region coordinates or source bindings.
            if preview
                .is_some_and(|(_, size)| (size.x / size.y - extent.x / extent.y).abs() > 0.001)
            {
                preview = None;
            }
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        let header_rect =
            Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 44.0));
        ui.painter().rect_filled(header_rect, 0, kit::PANEL);
        kit::paint_panel_edge(ui, header_rect, kit::PanelEdge::Bottom);
        kit::bounded_horizontal_row(ui, 44.0, |ui, width| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_space(16.0);
            ui.label(kit::caption(
                audition
                    .as_ref()
                    .map(|v| format!("Preview · {v}"))
                    .unwrap_or_else(|| match profile {
                        AssetLabAuthoringProfile::Mask => "Image edit".into(),
                        AssetLabAuthoringProfile::Regions => "Scene composition".into(),
                        _ => "Create".into(),
                    }),
            ));
            if profile == AssetLabAuthoringProfile::Mask && audition.is_none() {
                ui.add_space(12.0);
                ui.label(kit::caption("Size"));
                let brush_hint = kit::Tooltip::new("Brush size")
                    .description("Diameter in canvas pixels. [ makes it smaller; ] makes it larger. Active in Create when you are not typing.")
                    .shortcut("[ / ]");
                if width > 650.0 {
                    brush_hint.apply(kit::compact_slider(
                        ui,
                        "Brush size",
                        &mut canvas.brush,
                        1.0..=512.0,
                        105.0,
                    ));
                }
                let mut value = canvas.brush.round() as i64;
                brush_hint.apply(
                    ui.scope(|ui| {
                        kit::integer_step_drag(ui, &mut value, 46.0, 1, Some(1), Some(512))
                    })
                    .response,
                );
                canvas.brush = value as f32;
                ui.label(kit::caption("px"));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(12.0);
                if profile == AssetLabAuthoringProfile::Regions && audition.is_none() {
                    let visible = self.asset_lab.v4.guide_visible;
                    if kit::tool_button(
                        ui,
                        if visible {
                            kit::Icon::Eye
                        } else {
                            kit::Icon::EyeOff
                        },
                        "Show guide image",
                        visible,
                    )
                    .clicked()
                    {
                        self.asset_lab.v4.guide_visible = !visible;
                    }
                }
                ui.label(kit::caption(format!(
                    "{} × {}",
                    extent.x as u32, extent.y as u32
                )));
                if width > 540.0 {
                    ui.add_space(10.0);
                    if kit::tool_button(ui, kit::Icon::Plus, "Zoom in", false).clicked() {
                        self.asset_lab.preview_zoom =
                            (self.asset_lab.preview_zoom * 1.2).clamp(0.001, 32.0);
                        self.asset_lab.preview_auto_fit = false;
                    }
                    if kit::Tooltip::new("Actual size · 100%")
                        .description("Click to reset zoom to 100% and center the canvas. Use Fit to show the whole canvas.")
                        .apply(kit::icon_button_sized(
                            ui,
                            &format!("{:.0}%", self.asset_lab.preview_zoom * 100.0),
                            Vec2::new(52.0, 32.0),
                        ))
                        .clicked()
                    {
                        self.asset_lab.preview_zoom = 1.0;
                        self.asset_lab.preview_pan = Vec2::ZERO;
                        self.asset_lab.preview_auto_fit = false;
                    }
                    if kit::tool_button(ui, kit::Icon::Minus, "Zoom out", false).clicked() {
                        self.asset_lab.preview_zoom =
                            (self.asset_lab.preview_zoom / 1.2).clamp(0.001, 32.0);
                        self.asset_lab.preview_auto_fit = false;
                    }
                }
            });
        });
        let mut committed_mask = false;
        let mut commit_regions = false;
        let height = (ui.available_height() - if asset.is_video() { 50.0 } else { 0.0 }).max(1.0);
        kit::bounded_horizontal_row(ui, height, |ui, width| {
            let rail = 48.0;
            let rail_rect = Rect::from_min_size(ui.cursor().min, Vec2::new(rail, height));
            ui.painter().rect_filled(rail_rect, 0, kit::PANEL);
            kit::paint_panel_edge(ui, rail_rect, kit::PanelEdge::Right);
            ui.allocate_ui_with_layout(
                Vec2::new(rail, height),
                Layout::top_down(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.add_space(8.0);
                    let tools: &[(Tool, kit::Icon, &str)] = match profile {
                        AssetLabAuthoringProfile::Mask => &[
                            (Tool::Paint, kit::Icon::Brush, "Brush"),
                            (Tool::Erase, kit::Icon::Erase, "Erase"),
                        ],
                        AssetLabAuthoringProfile::Regions => &[
                            (Tool::Select, kit::Icon::Select, "Select"),
                            (Tool::Object, kit::Icon::Object, "Object"),
                            (Tool::Text, kit::Icon::Text, "Text"),
                        ],
                        _ => &[],
                    };
                    for (tool, icon, label) in tools {
                        let hint = match tool {
                            Tool::Paint => kit::Tooltip::new(label).shortcut("B").description("Paint the area to change. Use [ / ] to adjust brush size. Shortcut active in Create when you are not typing."),
                            Tool::Erase => kit::Tooltip::new(label).shortcut("E").description("Erase painted mask areas. Use [ / ] to adjust brush size. Shortcut active in Create when you are not typing."),
                            Tool::Select => kit::Tooltip::new(label).description("Select a prompt region, then drag to move it or drag its corner to resize."),
                            Tool::Object => kit::Tooltip::new(label).description("Drag on the canvas to draw an object region, then describe it in the inspector."),
                            Tool::Text => kit::Tooltip::new(label).description("Drag on the canvas to draw a text region, then enter its text in the inspector."),
                        };
                        if kit::tool_button(ui, *icon, hint, canvas.tool == *tool).clicked() {
                            canvas.tool = *tool;
                            if profile == AssetLabAuthoringProfile::Mask {
                                self.asset_lab.v4.mask_visible = true;
                            }
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_enabled_ui(
                        !self.asset_lab.v4.undo.is_empty() && audition.is_none(),
                        |ui| {
                            let hint = kit::Tooltip::new("Undo").shortcut("Ctrl+Z").description(
                                if self.asset_lab.v4.undo.is_empty() { "No authoring changes to undo in this Create session." }
                                else { "Undo the last authoring change in this Create session. Text fields keep their own undo." });
                            undo |= kit::tool_button(ui, kit::Icon::Undo, hint, false).clicked();
                        },
                    );
                    if profile != AssetLabAuthoringProfile::Generic {
                        ui.add_enabled_ui(audition.is_none(), |ui| {
                            let hint = kit::Tooltip::new("Clear").description(
                                if profile == AssetLabAuthoringProfile::Mask { "Clear the entire painted mask. You can undo this in the current Create session." }
                                else { "Clear all prompt regions. You can undo this in the current Create session." });
                            clear = kit::tool_button(ui, kit::Icon::Trash, hint, false).clicked();
                        });
                    }
                    ui.add_space((rail_rect.bottom() - 40.0 - ui.cursor().min.y).max(0.0));
                    if kit::tool_button(ui, kit::Icon::Fit, kit::Tooltip::new("Fit").description("Fit the whole canvas in view. Use the wheel to zoom and right-drag to pan."), false).clicked() {
                        self.asset_lab.preview_auto_fit = true;
                    }
                },
            );
            let (rect, response) = ui.allocate_exact_size(
                Vec2::new(
                    (width - rail - ui.spacing().item_spacing.x).max(1.0),
                    height,
                ),
                Sense::click_and_drag(),
            );
            let response = crate::core::automation::instrument_response(
                response,
                "authoring_canvas",
                Some("Asset Lab canvas".into()),
                false,
                false,
            );
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0, kit::PANEL_SUNKEN);
            let notice = message
                .as_deref()
                .map(|message| kit::ViewportNotice::new(ui, rect, message));
            let over_notice = notice.as_ref().is_some_and(|notice| {
                ui.input(|i| {
                    i.pointer
                        .interact_pos()
                        .is_some_and(|point| notice.rect.contains(point))
                })
            });
            let fit = ((rect.width() - 32.0) / extent.x)
                .min((rect.height() - 32.0) / extent.y)
                .max(0.001);
            if self.asset_lab.preview_auto_fit {
                self.asset_lab.preview_zoom = fit;
                self.asset_lab.preview_pan = Vec2::ZERO;
            }
            let scroll = if over_notice {
                0.0
            } else {
                preview_scroll_delta(ui, rect)
            };
            if scroll != 0.0 {
                let old = self.asset_lab.preview_zoom;
                let next = (old * canvas_wheel_zoom_factor(scroll)).clamp(0.001, 32.0);
                if let Some(pointer) = response.hover_pos() {
                    self.asset_lab.preview_pan = pointer
                        - (pointer - rect.center() - self.asset_lab.preview_pan) * (next / old)
                        - rect.center();
                }
                self.asset_lab.preview_zoom = next;
                self.asset_lab.preview_auto_fit = false;
            }
            if !over_notice && response.dragged_by(egui::PointerButton::Secondary) {
                self.asset_lab.preview_pan += response.drag_delta();
                self.asset_lab.preview_auto_fit = false;
            }
            let image_rect = Rect::from_center_size(
                rect.center() + self.asset_lab.preview_pan,
                extent * self.asset_lab.preview_zoom,
            );
            painter.rect_filled(image_rect, 0, kit::FIELD_BG);
            if let Some((texture, _)) = preview {
                painter.image(
                    texture,
                    image_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let pressed = !over_notice
                && response.is_pointer_button_down_on()
                && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
            let down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary));
            if audition.is_some() {
                if pressed {
                    self.asset_lab.v4.dismissed_preview = self.asset_lab.v4.preview.take();
                    response.request_focus();
                }
                return;
            }
            if pressed && self.source_picker.is_none() && self.asset_lab.v4.pending_adopt.is_none()
            {
                response.request_focus();
                let label_hit = pointer
                    .filter(|point| {
                        profile == AssetLabAuthoringProfile::Regions && rect.contains(*point)
                    })
                    .and_then(|point| {
                        setup
                            .authoring
                            .regions
                            .iter()
                            .rev()
                            .find(|region| {
                                region_label_layout(&painter, region, image_rect, rect)
                                    .0
                                    .contains(point)
                            })
                            .map(|region| region.id)
                    });
                if let Some(id) = label_hit {
                    canvas.selected = Some(id);
                    canvas.tool = Tool::Select;
                } else if let Some(point) = pointer.filter(|point| image_rect.contains(*point)) {
                    let point = canvas_point(point, image_rect, extent);
                    if profile == AssetLabAuthoringProfile::Mask
                        && aligned
                        && canvas.geometry.is_some()
                        && matches!(canvas.tool, Tool::Paint | Tool::Erase)
                    {
                        self.asset_lab.v4.mask_visible = true;
                        if canvas.pixels.is_none() {
                            canvas.pixels = Some(GrayImage::new(extent.x as u32, extent.y as u32));
                        }
                        canvas.gesture = Some(Gesture::Stroke { last: point });
                    } else if profile == AssetLabAuthoringProfile::Regions {
                        let normalized = Pos2::new(point.x / extent.x, point.y / extent.y);
                        let hit = setup
                            .authoring
                            .regions
                            .iter()
                            .rev()
                            .find(|region| {
                                region_rect(region.bounds, image_rect).expand(6.0).contains(
                                    image_rect.min + point.to_vec2() * self.asset_lab.preview_zoom,
                                )
                            })
                            .map(|region| (region.id, region.bounds));
                        if canvas.tool == Tool::Select {
                            canvas.selected = hit.map(|(id, _)| id);
                            if let Some((id, bounds)) = hit {
                                let bottom_right =
                                    Pos2::new(bounds[0] + bounds[2], bounds[1] + bounds[3]);
                                let resize = ((normalized - bottom_right) * image_rect.size())
                                    .length()
                                    < 12.0;
                                canvas.gesture = Some(Gesture::Region {
                                    start: normalized,
                                    original: bounds,
                                    id,
                                    resize,
                                    creating: false,
                                });
                            }
                        } else if matches!(canvas.tool, Tool::Object | Tool::Text) {
                            let id = Uuid::new_v4();
                            let bounds = [normalized.x, normalized.y, 0.001, 0.001];
                            setup.authoring.regions.push(AssetLabRegion {
                                id,
                                name: format!(
                                    "{} {}",
                                    if canvas.tool == Tool::Text {
                                        "Text"
                                    } else {
                                        "Object"
                                    },
                                    setup.authoring.regions.len() + 1
                                ),
                                description: String::new(),
                                text: (canvas.tool == Tool::Text).then(String::new),
                                bounds,
                            });
                            canvas.selected = Some(id);
                            canvas.gesture = Some(Gesture::Region {
                                start: normalized,
                                original: bounds,
                                id,
                                resize: true,
                                creating: true,
                            });
                        }
                    }
                }
            }
            if down {
                if let (Some(point), Some(gesture)) = (pointer, canvas.gesture.as_mut()) {
                    let point = canvas_point(point, image_rect, extent);
                    match gesture {
                        Gesture::Stroke { last } => {
                            if let Some(pixels) = canvas.pixels.as_mut() {
                                paint_segment(
                                    pixels,
                                    *last,
                                    point,
                                    canvas.brush * 0.5,
                                    canvas.tool == Tool::Erase,
                                );
                                canvas.overlay = None;
                            }
                            *last = point;
                        }
                        Gesture::Region {
                            start,
                            original,
                            id,
                            resize,
                            creating,
                            ..
                        } => {
                            let point = Pos2::new(
                                (point.x / extent.x).clamp(0.0, 1.0),
                                (point.y / extent.y).clamp(0.0, 1.0),
                            );
                            if let Some(region) = setup
                                .authoring
                                .regions
                                .iter_mut()
                                .find(|region| region.id == *id)
                            {
                                region.bounds =
                                    drag_region(*original, *start, point, *resize, *creating);
                            }
                        }
                    }
                }
            } else if let Some(gesture) = canvas.gesture.take() {
                match gesture {
                    Gesture::Stroke { .. } => committed_mask = true,
                    Gesture::Region { .. } => commit_regions = true,
                }
            }
            if profile == AssetLabAuthoringProfile::Mask
                && aligned
                && self.asset_lab.v4.mask_visible
            {
                if let Some(pixels) = &canvas.pixels {
                    if canvas.overlay_enabled != setup.authoring.mask_enabled {
                        canvas.overlay = None;
                        canvas.overlay_enabled = setup.authoring.mask_enabled;
                    }
                    if canvas.overlay.is_none() {
                        let color = if setup.authoring.mask_enabled {
                            kit::IMAGE
                        } else {
                            Color32::GRAY
                        };
                        let rgba: Vec<u8> = pixels
                            .pixels()
                            .flat_map(|p| {
                                [
                                    color.r(),
                                    color.g(),
                                    color.b(),
                                    (u16::from(p[0]) * 110 / 255) as u8,
                                ]
                            })
                            .collect();
                        let image = ColorImage::from_rgba_unmultiplied(
                            [pixels.width() as usize, pixels.height() as usize],
                            &rgba,
                        );
                        if let Some(texture) = canvas.overlay.as_mut() {
                            texture.set(image, egui::TextureOptions::NEAREST);
                        } else {
                            canvas.overlay = Some(ui.ctx().load_texture(
                                "lab_mask_overlay",
                                image,
                                egui::TextureOptions::NEAREST,
                            ));
                        }
                    }
                    painter.image(
                        canvas.overlay.as_ref().unwrap().id(),
                        image_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                if let Some(pointer) = response
                    .hover_pos()
                    .filter(|_| !over_notice)
                    .filter(|_| matches!(canvas.tool, Tool::Paint | Tool::Erase))
                {
                    painter.circle_stroke(
                        pointer,
                        canvas.brush * 0.5 * self.asset_lab.preview_zoom,
                        Stroke::new(1.0_f32, Color32::WHITE),
                    );
                }
            }
            if profile == AssetLabAuthoringProfile::Regions {
                for region in &setup.authoring.regions {
                    let bounds = region_rect(region.bounds, image_rect);
                    let color = if setup.authoring.regions_enabled {
                        kit::IMAGE
                    } else {
                        Color32::GRAY
                    };
                    painter.rect_stroke(
                        bounds,
                        0,
                        Stroke::new(
                            if canvas.selected == Some(region.id) {
                                2.0_f32
                            } else {
                                1.0_f32
                            },
                            color,
                        ),
                        egui::StrokeKind::Inside,
                    );
                    let (label_rect, label) =
                        region_label_layout(&painter, region, image_rect, rect);
                    crate::core::automation::instrument_response(
                        ui.interact(label_rect, response.id.with(region.id), Sense::hover())
                            .on_hover_cursor(egui::CursorIcon::PointingHand),
                        "region_label",
                        Some(region.name.clone()),
                        false,
                        false,
                    );
                    painter.rect_filled(
                        label_rect,
                        3,
                        if canvas.selected == Some(region.id) {
                            Color32::from_rgb(26, 58, 66)
                        } else {
                            kit::PANEL
                        },
                    );
                    painter.galley(label_rect.min + Vec2::new(6.0, 3.0), label, kit::TEXT);
                    if canvas.selected == Some(region.id) {
                        painter.rect_filled(
                            Rect::from_center_size(bounds.max, Vec2::splat(8.0)),
                            0,
                            color,
                        );
                    }
                }
                if shortcuts_active
                    && response.has_focus()
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
                {
                    setup
                        .authoring
                        .regions
                        .retain(|r| Some(r.id) != canvas.selected);
                    commit_regions = true;
                }
            }
            if let Some(notice) = notice {
                notice.show(ui);
            }
        });
        if asset.is_video() {
            self.asset_lab_video_scrubber(
                ui,
                asset.duration_seconds.unwrap_or(0.0),
                asset_lab_video_fps(asset, self.editor.project.settings.fps),
            );
        }
        self.asset_lab.v4.interacting = canvas.gesture.is_some();
        canvas.draft = if matches!(canvas.gesture, Some(Gesture::Region { .. })) {
            Some(setup.clone())
        } else {
            None
        };
        if clear && audition.is_none() {
            if profile == AssetLabAuthoringProfile::Mask {
                setup.authoring.mask = None;
                canvas.pixels = None;
                canvas.mask_path = None;
            } else {
                setup.authoring.regions.clear();
            }
            commit_regions = true;
        }
        if committed_mask {
            if let (Some(pixels), Some(geometry), Some(root), Some(folder)) = (
                &canvas.pixels,
                &canvas.geometry,
                &self.editor.project.project_path,
                generative_folder_for_asset(asset),
            ) {
                let path = folder
                    .join("inputs")
                    .join("authored_masks")
                    .join(format!("{}.png", Uuid::new_v4()));
                let result = std::fs::create_dir_all(root.join(&path).parent().unwrap())
                    .map_err(|e| e.to_string())
                    .and_then(|_| pixels.save(root.join(&path)).map_err(|e| e.to_string()));
                match result {
                    Ok(()) => {
                        setup.authoring.mask = Some(AssetLabMask {
                            path: path.clone(),
                            geometry: geometry.clone(),
                            has_content: pixels.as_raw().iter().any(|p| *p > 0),
                        });
                        canvas.mask_path = Some(path);
                        commit_regions = true;
                    }
                    Err(error) => self.editor.status = error,
                }
            }
        }
        if commit_regions && setup != AssetLabSnapshot::from_config(config) {
            if let Err(error) = self.editor.set_asset_lab_setup(asset.id, &setup) {
                self.editor.status = error;
            } else {
                self.asset_lab.v4.observe(setup, None);
            }
        }
        if undo {
            self.undo_asset_lab_v4(asset.id);
            canvas.mask_path = None;
        }
        self.asset_lab.v4.canvas = canvas;
    }

    fn undo_asset_lab_v4(&mut self, asset_id: Uuid) {
        if let Some(setup) = self.asset_lab.v4.undo.pop() {
            if let Err(error) = self.editor.set_asset_lab_setup(asset_id, &setup) {
                self.editor.status = error;
                self.asset_lab.v4.undo.push(setup);
            } else {
                self.asset_lab.v4.observed = Some(setup);
                self.asset_lab.v4.revision = self.asset_lab.v4.revision.wrapping_add(1);
                self.asset_lab.v4.edit_group = None;
            }
        }
    }
}

fn canvas_point(point: Pos2, rect: Rect, extent: Vec2) -> Pos2 {
    let relative = (point - rect.min) / rect.size() * extent;
    Pos2::new(relative.x, relative.y)
}
fn region_label_layout(
    painter: &egui::Painter,
    region: &AssetLabRegion,
    image_rect: Rect,
    viewport: Rect,
) -> (Rect, std::sync::Arc<egui::Galley>) {
    let bounds = region_rect(region.bounds, image_rect);
    let label = painter.layout_no_wrap(region.name.clone(), FontId::proportional(12.0), kit::TEXT);
    let size = label.size() + Vec2::new(12.0, 6.0);
    let pos = Pos2::new(
        bounds.left().max(viewport.left()),
        (bounds.top() - size.y).max(viewport.top()),
    );
    (Rect::from_min_size(pos, size), label)
}

fn region_rect(bounds: [f32; 4], rect: Rect) -> Rect {
    Rect::from_min_size(
        rect.min + Vec2::new(bounds[0], bounds[1]) * rect.size(),
        Vec2::new(bounds[2], bounds[3]) * rect.size(),
    )
}
fn mask_geometry_matches(a: &AssetLabMaskGeometry, b: &AssetLabMaskGeometry) -> bool {
    a.width == b.width
        && a.height == b.height
        && a.sizing == b.sizing
        && a.source_identity == b.source_identity
        && a.base.source_path == b.base.source_path
        && a.base.source_frame_time == b.base.source_frame_time
}

fn drag_region(
    original: [f32; 4],
    start: Pos2,
    point: Pos2,
    resize: bool,
    creating: bool,
) -> [f32; 4] {
    if creating {
        let x = start.x.min(point.x).clamp(0.0, 0.995);
        let y = start.y.min(point.y).clamp(0.0, 0.995);
        [
            x,
            y,
            (point.x - start.x).abs().clamp(0.005, 1.0 - x),
            (point.y - start.y).abs().clamp(0.005, 1.0 - y),
        ]
    } else if resize {
        [
            original[0],
            original[1],
            (original[2] + point.x - start.x).clamp(0.005, 1.0 - original[0]),
            (original[3] + point.y - start.y).clamp(0.005, 1.0 - original[1]),
        ]
    } else {
        [
            (original[0] + point.x - start.x).clamp(0.0, 1.0 - original[2]),
            (original[1] + point.y - start.y).clamp(0.0, 1.0 - original[3]),
            original[2],
            original[3],
        ]
    }
}

fn paint_segment(image: &mut GrayImage, from: Pos2, to: Pos2, radius: f32, erase: bool) {
    let steps = ((to - from).length() / (radius * 0.35).max(0.5))
        .ceil()
        .max(1.0) as usize;
    for step in 0..=steps {
        let center = from + (to - from) * (step as f32 / steps as f32);
        let left = (center.x - radius).floor().max(0.0) as u32;
        let right = (center.x + radius).ceil().min(image.width() as f32) as u32;
        let top = (center.y - radius).floor().max(0.0) as u32;
        let bottom = (center.y + radius).ceil().min(image.height() as f32) as u32;
        for y in top..bottom {
            for x in left..right {
                if (Pos2::new(x as f32 + 0.5, y as f32 + 0.5) - center).length_sq()
                    <= radius * radius
                {
                    image.put_pixel(x, y, image::Luma([if erase { 0 } else { 255 }]));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn asset_lab_v4_real_egui_strokes_and_keyboard_preview_preserve_setup() {
        let ctx = Context::default();
        let mut app = LatentSlateApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        let root = std::env::temp_dir().join(format!("ls-v4-egui-{}", Uuid::new_v4()));
        let folder = PathBuf::from("generated/image/test");
        std::fs::create_dir_all(root.join(&folder)).unwrap();
        for version in ["v1", "v2"] {
            image::RgbImage::from_pixel(320, 160, image::Rgb([30, 70, 110]))
                .save(root.join(&folder).join(format!("{version}.png")))
                .unwrap();
        }
        let asset = Asset::new_generative_image("Egui fixture", folder);
        let provider:ProviderEntry=serde_json::from_value(serde_json::json!({"id":Uuid::new_v4(),"name":"Offline authoring","output_type":"image","inputs":[{"name":"image","label":"Image","input_type":{"type":"image"},"required":true}],"connection":{"type":"latent_slate_engine","base_url":"http://127.0.0.1:9","tool_key":"qwen2511.edit","schema_revision":1,"schema_hash":"offline","available":false}})).unwrap();
        let mut config = GenerativeConfig::default();
        config.provider_id = Some(provider.id);
        config.active_version = Some("v1".into());
        config.lab_authoring.initialized = true;
        config.lab_authoring.working_version = Some("v1".into());
        config.media_bindings.insert(
            "image".into(),
            crate::state::MediaBindingSpec {
                source: crate::state::MediaBindingSource::WorkingOutput,
                sample: crate::state::MediaSample::Whole,
                coverage: Default::default(),
            },
        );
        for version in ["v1", "v2"] {
            config.versions.push(GenerationRecord {
                version: version.into(),
                timestamp: chrono::Utc::now(),
                provider_id: provider.id,
                inputs_snapshot: Default::default(),
                media_bindings_snapshot: Default::default(),
                resolved_media_inputs: Default::default(),
                lab_node_id: None,
                authoring_snapshot: None,
                engine_execution: None,
            });
        }
        app.editor.project = crate::state::Project::new("Native input fixture");
        app.editor.project.project_path = Some(root.clone());
        app.editor.project.assets.push(asset.clone());
        app.editor
            .project
            .generative_configs
            .insert(asset.id, config);
        app.editor.provider_entries = vec![provider];
        app.open_asset_lab(asset.id);
        let frame = |app: &mut LatentSlateApp, events| {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 768.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.set_max_size(Vec2::new(1024.0, 768.0));
                    app.asset_lab_v4_contents(ui, &asset);
                    app.source_picker_modal(&ctx);
                },
            );
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        let key = |key, pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        let press = |app: &mut LatentSlateApp, code| {
            frame(app, vec![key(code, true)]);
            frame(app, vec![key(code, false)]);
        };
        let original_setup =
            AssetLabSnapshot::from_config(app.editor.project.generative_config(asset.id).unwrap());
        press(&mut app, egui::Key::E);
        assert_eq!(app.asset_lab.v4.canvas.tool, Tool::Erase);
        press(&mut app, egui::Key::B);
        assert_eq!(app.asset_lab.v4.canvas.tool, Tool::Paint);
        press(&mut app, egui::Key::CloseBracket);
        assert_eq!(app.asset_lab.v4.canvas.brush, 60.0);
        press(&mut app, egui::Key::OpenBracket);
        assert_eq!(app.asset_lab.v4.canvas.brush, 48.0);
        app.asset_lab.v4.canvas.brush = 1.0;
        press(&mut app, egui::Key::CloseBracket);
        assert_eq!(app.asset_lab.v4.canvas.brush, 2.0);
        press(&mut app, egui::Key::OpenBracket);
        assert_eq!(app.asset_lab.v4.canvas.brush, 1.0);
        app.asset_lab.v4.canvas.brush = 512.0;
        press(&mut app, egui::Key::CloseBracket);
        assert_eq!(app.asset_lab.v4.canvas.brush, 512.0);
        app.asset_lab.v4.canvas.brush = 48.0;
        app.asset_lab.v4.view = AssetLabView::Lineage;
        press(&mut app, egui::Key::E);
        press(&mut app, egui::Key::CloseBracket);
        assert_eq!(
            (app.asset_lab.v4.canvas.tool, app.asset_lab.v4.canvas.brush),
            (Tool::Paint, 48.0)
        );
        app.asset_lab.v4.view = AssetLabView::Create;
        frame(&mut app, vec![]);
        assert_eq!(
            AssetLabSnapshot::from_config(app.editor.project.generative_config(asset.id).unwrap()),
            original_setup
        );
        let point = Pos2::new(350.0, 300.0);
        let wheel = || egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, 1.0),
            phase: egui::TouchPhase::Move,
            modifiers: Default::default(),
        };
        let before_zoom = app.asset_lab.preview_zoom;
        frame(&mut app, vec![egui::Event::PointerMoved(point), wheel()]);
        let zoomed = app.asset_lab.preview_zoom;
        assert!((zoomed / before_zoom - 1.048).abs() < 0.0001);
        let provider = app.editor.provider_entries[0].clone();
        app.open_source_picker(asset.id, None, &provider, &provider.inputs[0]);
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        for pointer in [point, Pos2::new(80.0, 300.0)] {
            frame(&mut app, vec![egui::Event::PointerMoved(pointer), wheel()]);
            assert_eq!(
                app.asset_lab.preview_zoom, zoomed,
                "picker and scrim shield canvas wheel input"
            );
        }
        press(&mut app, egui::Key::E);
        press(&mut app, egui::Key::CloseBracket);
        assert_eq!(
            (app.asset_lab.v4.canvas.tool, app.asset_lab.v4.canvas.brush),
            (Tool::Paint, 48.0)
        );
        app.source_picker = None;
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        let pointer = |pressed| egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(point), pointer(true)],
        );
        assert!(app.asset_lab.v4.interacting);
        press(&mut app, egui::Key::E);
        assert_eq!(app.asset_lab.v4.canvas.tool, Tool::Paint);
        frame(&mut app, vec![pointer(false)]);
        let config = app.editor.project.generative_config(asset.id).unwrap();
        let mask = config
            .lab_authoring
            .mask
            .as_ref()
            .expect("real egui response commits a stroke");
        assert_eq!((mask.geometry.width, mask.geometry.height), (320, 160));
        assert!(image::open(root.join(&mask.path))
            .unwrap()
            .to_luma8()
            .pixels()
            .any(|pixel| pixel[0] > 0));
        let before = AssetLabSnapshot::from_config(config);
        let revision = app.asset_lab.v4.revision;
        let session = app.asset_lab.v4.session_id;
        let undo = app.asset_lab.v4.undo.len();
        app.asset_lab.v4.results.push(LabResult {
            job_id: Uuid::new_v4(),
            version: Some("v2".into()),
            status: GenerationJobStatus::Succeeded,
        });
        frame(&mut app, vec![egui::Event::PointerGone]);
        for _ in 0..20 {
            frame(&mut app, vec![key(egui::Key::Tab, true)]);
            frame(&mut app, vec![key(egui::Key::Tab, false)]);
            if app.asset_lab.v4.preview.is_some() {
                break;
            }
        }
        assert_eq!(app.asset_lab.v4.preview.as_deref(), Some("v2"));
        press(&mut app, egui::Key::E);
        assert_eq!(app.asset_lab.v4.canvas.tool, Tool::Paint);
        frame(&mut app, vec![key(egui::Key::Escape, true)]);
        assert!(app.asset_lab.v4.preview.is_none());
        assert_eq!(
            AssetLabSnapshot::from_config(app.editor.project.generative_config(asset.id).unwrap()),
            before
        );
        assert_eq!(app.asset_lab.v4.revision, revision);
        assert_eq!(app.asset_lab.v4.session_id, session);
        assert_eq!(app.asset_lab.v4.undo.len(), undo);
        app.editor
            .project
            .update_generative_config(asset.id, |config| {
                config
                    .versions
                    .iter_mut()
                    .find(|record| record.version == "v2")
                    .unwrap()
                    .authoring_snapshot = Some(before.clone());
            });
        app.asset_lab.v4.mask_visible = false;
        app.adopt_asset_lab_result(asset.id, "v2");
        frame(&mut app, vec![]);
        let adopted =
            AssetLabSnapshot::from_config(app.editor.project.generative_config(asset.id).unwrap());
        assert_eq!(adopted.authoring.working_version.as_deref(), Some("v2"));
        assert_eq!(
            adopted.authoring.mask.as_ref().unwrap().path,
            before.authoring.mask.as_ref().unwrap().path
        );
        assert_eq!(
            adopted.authoring.mask_enabled,
            before.authoring.mask_enabled
        );
        assert!(mask_geometry_matches(
            &adopted.authoring.mask.as_ref().unwrap().geometry,
            app.asset_lab.v4.canvas.geometry.as_ref().unwrap()
        ));
        assert!(!app.asset_lab.v4.mask_visible);
        assert_ne!(app.asset_lab.v4.session_id, session);
        assert!(app.asset_lab.v4.undo.is_empty() && app.asset_lab.v4.results.is_empty());
        app.pin_asset_lab_result_v4(asset.id, "v2");
        assert_eq!(
            AssetLabSnapshot::from_config(app.editor.project.generative_config(asset.id).unwrap()),
            adopted
        );
    }
    #[test]
    fn asset_lab_v4_non_square_zoom_pan_preserves_pixel_coordinates() {
        let extent = Vec2::new(800.0, 320.0);
        let rect = Rect::from_min_size(Pos2::new(-120.0, 74.0), extent * 1.75);
        let point = canvas_point(rect.min + Vec2::new(400.0, 160.0) * 1.75, rect, extent);
        assert_eq!(point, Pos2::new(400.0, 160.0));
        let mut pixels = GrayImage::new(800, 320);
        paint_segment(
            &mut pixels,
            point,
            point + Vec2::new(100.0, 0.0),
            10.0,
            false,
        );
        assert_eq!(pixels.get_pixel(450, 160)[0], 255);
        assert_eq!(pixels.get_pixel(450, 172)[0], 0);
        paint_segment(&mut pixels, point, point + Vec2::new(100.0, 0.0), 5.0, true);
        assert_eq!(pixels.get_pixel(450, 160)[0], 0);
        assert_eq!(pixels.get_pixel(450, 168)[0], 255);
    }

    #[test]
    fn asset_lab_v4_regions_move_resize_and_reverse_draw_without_axis_distortion() {
        let moved = drag_region(
            [0.2, 0.1, 0.3, 0.4],
            Pos2::new(0.3, 0.2),
            Pos2::new(0.4, 0.4),
            false,
            false,
        );
        for (actual, expected) in moved.into_iter().zip([0.3, 0.3, 0.3, 0.4]) {
            assert!((actual - expected).abs() < 0.00001);
        }
        let region = drag_region(
            [0.2, 0.1, 0.3, 0.4],
            Pos2::new(0.5, 0.5),
            Pos2::new(1.0, 1.0),
            true,
            false,
        );
        for (actual, expected) in region.into_iter().zip([0.2, 0.1, 0.8, 0.9]) {
            assert!((actual - expected).abs() < 0.00001);
        }
        let region = drag_region(
            [0.7, 0.7, 0.001, 0.001],
            Pos2::new(0.7, 0.7),
            Pos2::new(0.2, 0.3),
            true,
            true,
        );
        assert!((region[2] - 0.5).abs() < 0.00001 && (region[3] - 0.4).abs() < 0.00001);
    }
}
