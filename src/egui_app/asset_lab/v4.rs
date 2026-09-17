use super::*;
use crate::state::{AssetLabSnapshot, AssetLabSubmission, GenerationJob, GenerationJobStatus};
mod canvas;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::egui_app) enum AssetLabView {
    #[default]
    Create,
    Lineage,
    Compare,
}

#[derive(Clone, Debug)]
pub(in crate::egui_app) struct LabResult {
    pub job_id: Uuid,
    pub version: Option<String>,
    pub status: GenerationJobStatus,
}

#[derive(Clone, Debug)]
pub(in crate::egui_app) struct AssetLabV4State {
    canvas: canvas::CanvasState,
    pub view: AssetLabView,
    pub session_id: Uuid,
    pub revision: u64,
    baseline: Option<AssetLabSnapshot>,
    observed: Option<AssetLabSnapshot>,
    undo: Vec<AssetLabSnapshot>,
    edit_group: Option<egui::Id>,
    pub results: Vec<LabResult>,
    pub preview: Option<String>,
    dismissed_preview: Option<String>,
    pub interacting: bool,
    input_busy: bool,
    result_rect: Option<Rect>,
    pending_adopt: Option<String>,
    pin_undo: Option<String>,
    pub mask_visible: bool,
    pub guide_visible: bool,
    map_height: f32,
    compare_map_pan: Vec2,
    compare_map_zoom: f32,
    create_camera: (f32, Vec2, bool),
    compare_camera: (f32, Vec2, bool),
}

impl Default for AssetLabV4State {
    fn default() -> Self {
        Self {
            canvas: Default::default(),
            view: Default::default(),
            session_id: Uuid::new_v4(),
            revision: 0,
            baseline: None,
            observed: None,
            undo: Vec::new(),
            edit_group: None,
            results: Vec::new(),
            preview: None,
            dismissed_preview: None,
            interacting: false,
            input_busy: false,
            result_rect: None,
            pending_adopt: None,
            pin_undo: None,
            mask_visible: true,
            guide_visible: true,
            map_height: 220.0,
            compare_map_pan: Vec2::ZERO,
            compare_map_zoom: 1.0,
            create_camera: (1.0, Vec2::ZERO, true),
            compare_camera: (1.0, Vec2::ZERO, true),
        }
    }
}

impl AssetLabV4State {
    fn dirty(&self) -> bool {
        self.observed != self.baseline
    }

    fn observe(&mut self, setup: AssetLabSnapshot, group: Option<egui::Id>) {
        if self.observed.as_ref() == Some(&setup) {
            if self.edit_group != group {
                self.edit_group = None;
            }
            return;
        }
        if let Some(before) = self.observed.replace(setup) {
            if group.is_none() || self.edit_group != group || self.undo.is_empty() {
                self.undo.push(before);
            }
            self.edit_group = group;
            while self.undo.len() > 20
                || (self.undo.len() > 1
                    && self
                        .undo
                        .iter()
                        .map(|entry| {
                            serde_json::to_vec(entry)
                                .map(|bytes| bytes.len())
                                .unwrap_or(0)
                                + entry
                                    .authoring
                                    .mask
                                    .as_ref()
                                    .map(|mask| {
                                        mask.geometry.width as usize * mask.geometry.height as usize
                                    })
                                    .unwrap_or(0)
                        })
                        .sum::<usize>()
                        > 4 * 1024 * 1024)
            {
                self.undo.remove(0);
            }
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn can_advance(&self, submission: &AssetLabSubmission, modal: bool) -> bool {
        submission.session_id == self.session_id
            && submission.revision == self.revision
            && submission.allow_advance
            && self.view != AssetLabView::Compare
            && self.preview.is_none()
            && !self.interacting
            && !self.input_busy
            && !modal
            && self.pending_adopt.is_none()
    }
}

impl LatentSlateApp {
    pub(in crate::egui_app) fn set_asset_lab_review_results(
        &mut self,
        versions: Vec<String>,
        states: Vec<String>,
    ) -> Result<(), String> {
        let states: Vec<_> = states
            .iter()
            .map(|state| match state.as_str() {
                "queued" => Ok(GenerationJobStatus::Queued),
                "running" => Ok(GenerationJobStatus::Running),
                "canceling" => Ok(GenerationJobStatus::Canceling),
                "failed" => Ok(GenerationJobStatus::Failed),
                "canceled" => Ok(GenerationJobStatus::Canceled),
                _ => Err(
                    "Review states must be queued, running, canceling, failed, or canceled."
                        .to_string(),
                ),
            })
            .collect::<Result<_, _>>()?;
        let asset_id = self.asset_lab.asset_id.ok_or("Open Asset Lab first.")?;
        let config = self
            .editor
            .project
            .generative_config(asset_id)
            .ok_or("Asset unavailable.")?;
        if versions.iter().any(|version| {
            !config
                .versions
                .iter()
                .any(|record| record.version == *version)
        }) {
            return Err("Review results require existing completed versions; placeholder states cannot be Succeeded.".into());
        }
        if self.editor.generation_queue.iter().any(|job| {
            job.asset_id == asset_id
                && matches!(
                    job.status,
                    GenerationJobStatus::Queued
                        | GenerationJobStatus::Running
                        | GenerationJobStatus::Canceling
                )
        }) {
            return Err(
                "Wait for this asset's real jobs to finish before reviewing a fixture strip."
                    .into(),
            );
        }
        self.asset_lab.v4.results = versions
            .into_iter()
            .map(|version| LabResult {
                job_id: Uuid::new_v4(),
                version: Some(version),
                status: GenerationJobStatus::Succeeded,
            })
            .chain(states.into_iter().map(|status| LabResult {
                job_id: Uuid::new_v4(),
                version: None,
                status,
            }))
            .collect();
        self.asset_lab.v4.preview = None;
        self.asset_lab.v4.dismissed_preview = None;
        Ok(())
    }

    pub(in crate::egui_app) fn asset_lab_v4_input_guard(&mut self, ctx: &Context) {
        self.asset_lab.v4.input_busy = ctx.any_popup_open()
            || ctx.input(|input| {
                input.pointer.any_down()
                    || input.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Text(_)
                                | egui::Event::Paste(_)
                                | egui::Event::Key { pressed: true, .. }
                        )
                    })
            })
            || (self.asset_lab.v4.view == AssetLabView::Create
                && self.asset_lab.v4.result_rect.is_some_and(|rect| {
                    ctx.pointer_hover_pos()
                        .is_some_and(|point| rect.contains(point))
                }));
    }
    pub(in crate::egui_app) fn reset_asset_lab_session(&mut self) {
        let setup = self
            .asset_lab
            .asset_id
            .and_then(|id| self.editor.project.generative_config(id))
            .map(AssetLabSnapshot::from_config);
        let visible = (
            self.asset_lab.v4.mask_visible,
            self.asset_lab.v4.guide_visible,
        );
        let pin_undo = self.asset_lab.v4.pin_undo.take();
        self.asset_lab.v4 = AssetLabV4State {
            baseline: setup.clone(),
            observed: setup,
            mask_visible: visible.0,
            guide_visible: visible.1,
            pin_undo,
            ..Default::default()
        };
    }

    pub(in crate::egui_app::asset_lab) fn asset_lab_v4_header(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: &GenerativeConfig,
    ) -> bool {
        let can_compare = self.asset_lab.v4.view != AssetLabView::Create
            && self.asset_lab.selected_version.as_ref().is_some_and(|v| {
                config
                    .active_version
                    .as_ref()
                    .is_some_and(|active| active != v)
            });
        let header = kit::modal_header_layout(
            ui,
            "Asset Lab",
            Some(&asset.name),
            Some(kit::Icon::Lab),
            268.0,
            82.0,
            true,
        );
        let mut tabs = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(header.navigation)
                .layout(Layout::left_to_right(Align::Center)),
        );
        tabs.spacing_mut().item_spacing.x = 4.0;
        for (view, label) in [
            (AssetLabView::Create, "Create"),
            (AssetLabView::Lineage, "Lineage"),
            (AssetLabView::Compare, "Compare"),
        ] {
            let enabled =
                !self.asset_lab.v4.interacting && (view != AssetLabView::Compare || can_compare);
            let response =
                kit::workspace_tab(&mut tabs, label, self.asset_lab.v4.view == view, enabled);
            if !enabled {
                response.clone().on_disabled_hover_text(
                    "Choose a result in Lineage, or use Compare on a completed result thumbnail.",
                );
            }
            if response.clicked() {
                if view == AssetLabView::Compare {
                    self.begin_asset_lab_compare(asset.id);
                } else {
                    self.asset_lab.compare = None;
                    self.asset_lab.v4.view = view;
                }
                self.asset_lab.v4.preview = None;
            }
        }
        if let Some(pin) = &config.active_version {
            let mut trailing = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(header.trailing)
                    .layout(Layout::right_to_left(Align::Center)),
            );
            trailing.label(kit::caption(pin));
            let (rect, _) = trailing.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            kit::paint_icon(&trailing, kit::Icon::Pin, rect, kit::PRIMARY);
        }
        header.close_clicked
    }

    pub(in crate::egui_app) fn asset_lab_v4_contents(&mut self, ui: &mut Ui, asset: &Asset) {
        let Some(config) = self.editor.project.generative_config(asset.id).cloned() else {
            return;
        };
        self.asset_lab.v4.observe(
            AssetLabSnapshot::from_config(&config),
            ui.memory(|memory| memory.focused()),
        );
        if self.asset_lab.compare.is_some() {
            self.asset_lab.v4.view = AssetLabView::Compare;
        }
        if let Some(previous) = self.asset_lab.v4.pin_undo.clone() {
            kit::bounded_horizontal_row(ui, 27.0, |ui, _| {
                ui.label(kit::caption("Current output changed."));
                if kit::field_button(ui, "Undo output selection", 150.0).clicked() {
                    if let Err(error) = self.set_generative_active_version(asset.id, &previous) {
                        self.editor.status = error;
                    }
                    self.asset_lab.v4.pin_undo = None;
                }
                if kit::icon_button(ui, "×").clicked() {
                    self.asset_lab.v4.pin_undo = None;
                }
            });
        }
        match self.asset_lab.v4.view {
            AssetLabView::Create => {
                (
                    self.asset_lab.preview_zoom,
                    self.asset_lab.preview_pan,
                    self.asset_lab.preview_auto_fit,
                ) = self.asset_lab.v4.create_camera;
                self.asset_lab_create_v4(ui, asset, &config);
                self.asset_lab.v4.create_camera = (
                    self.asset_lab.preview_zoom,
                    self.asset_lab.preview_pan,
                    self.asset_lab.preview_auto_fit,
                );
            }
            AssetLabView::Lineage => {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                egui_extras::StripBuilder::new(ui)
                    .size(Size::remainder())
                    .size(Size::exact(340.0))
                    .horizontal(|mut strip| {
                        strip.cell(|ui| {
                            ui.painter()
                                .rect_filled(ui.max_rect(), 0, kit::PANEL_SUNKEN);
                            egui::Frame::new()
                                .inner_margin(egui::Margin::symmetric(16, 8))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 4.0);
                                    self.asset_lab_lineage_v4(ui, asset, &config, false);
                                });
                        });
                        strip.cell(|ui| {
                            ui.painter().rect_filled(ui.max_rect(), 0, kit::PANEL);
                            kit::paint_panel_edge(ui, ui.max_rect(), kit::PanelEdge::Left);
                            self.asset_lab_lineage_details_v4(ui, asset, &config);
                        });
                    });
            }
            AssetLabView::Compare => {
                (
                    self.asset_lab.preview_zoom,
                    self.asset_lab.preview_pan,
                    self.asset_lab.preview_auto_fit,
                ) = self.asset_lab.v4.compare_camera;
                self.asset_lab_compare_v4(ui, asset, &config);
                self.asset_lab.v4.compare_camera = (
                    self.asset_lab.preview_zoom,
                    self.asset_lab.preview_pan,
                    self.asset_lab.preview_auto_fit,
                );
            }
        }
        if let Some(version) = self.asset_lab.v4.pending_adopt.clone() {
            kit::nested_modal_scrim(ui.ctx(), "asset_lab_adopt_confirmation");
            egui::Window::new("Continue from result?")
                .order(egui::Order::Foreground)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(kit::modal_frame())
                .show(ui.ctx(), |ui| {
                    ui.label(
                        "This replaces your unsubmitted edits with the result’s submitted setup.",
                    );
                    if kit::primary_button(ui, &format!("Create from {version}"), 220.0).clicked() {
                        self.adopt_asset_lab_result(asset.id, &version);
                    }
                    if kit::secondary_button(ui, "Keep editing", 220.0).clicked() {
                        self.asset_lab.v4.pending_adopt = None;
                    }
                });
        }
    }

    pub(in crate::egui_app) fn dismiss_asset_lab_on_escape(&mut self, preview_before: Option<String>) {
        if self.asset_lab.v4.pending_adopt.take().is_some() {
            return;
        }
        // Escape can release the audition tile's focus before this frame renders.
        if let Some(preview) = self.asset_lab.v4.preview.take().or(preview_before) {
            self.asset_lab.v4.dismissed_preview = Some(preview);
        } else {
            self.close_asset_lab();
        }
    }

    fn asset_lab_create_v4(&mut self, ui: &mut Ui, asset: &Asset, config: &GenerativeConfig) {
        let provider = self
            .editor
            .provider_entries
            .iter()
            .find(|provider| Some(provider.id) == config.provider_id)
            .cloned();
        let condensed_shelf = provider.as_ref().is_some_and(|provider| {
            provider.resolved_workflow_kind()
                == crate::state::ProviderWorkflowKind::ReferenceToVideo
                || provider.inputs.iter().filter(|field| field.paired_video_input.is_none() && crate::core::media_binding::bound_media_type_for_input(field).is_some()).count() > 1
        });
        let expansion_id =
            egui::Id::new(("reference_slots_expanded", asset.id, config.provider_id));
        let mut expanded = ui.data(|data| data.get_temp::<bool>(expansion_id).unwrap_or(false));
        let media_fields: Vec<_> = provider
            .as_ref()
            .map(|provider| {
                reference_shelf_fields(
                    provider,
                    config,
                    &self.editor.project,
                    !condensed_shelf || expanded,
                )
                .into_iter()
                .cloned()
                .collect()
            })
            .unwrap_or_default();
        let ctx = ui.ctx().clone();
        let row_height = |row: &[ProviderInputField]| {
            let paired = provider.as_ref().is_some_and(|provider| {
                row.iter().any(|video| {
                    if !ctx.data(|data| data.get_temp::<bool>(Self::source_card_details_id(asset.id, provider.id, &video.name)).unwrap_or(false)) { return false; }
                    provider.inputs.iter().any(|input| {
                        input.paired_video_input.as_deref() == Some(video.name.as_str())
                    })
                })
            });
            let audio_open = provider.as_ref().is_some_and(|provider| row.iter().any(|field| field.input_type == ProviderInputType::Audio && ctx.data(|data| data.get_temp::<bool>(Self::source_card_details_id(asset.id, provider.id, &field.name)).unwrap_or(false))));
            kit::COMPACT_SOURCE_FIELD_H + if paired { 76.0 } else if audio_open { 36.0 } else { 0.0 }
        };
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        StripBuilder::new(ui)
            .size(Size::remainder())
            .size(Size::exact(340.0))
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    let columns = if media_fields.len() == 4 { 2 } else { media_fields.len().clamp(1, 3) };
                    let rows = media_fields.len().div_ceil(columns);
                    let inputs_height = if rows == 0 { if condensed_shelf { 128.0 } else { 0.0 } } else {
                        (media_fields.chunks(columns).map(|row| row_height(row) + 8.0).sum::<f32>() + 12.0 + if condensed_shelf { 112.0 } else { 0.0 }).min(if condensed_shelf { 240.0 } else { 220.0 })
                    };
                    let results_height = if self.asset_lab.v4.results.is_empty() { 0.0 } else { 126.0 };
                    StripBuilder::new(ui)
                        .size(Size::remainder().at_least(80.0))
                        .size(Size::exact(results_height))
                        .vertical(|mut strip| {
                            strip.cell(|ui| {
                                if let Some(provider) = provider.as_ref().filter(|_| condensed_shelf || !media_fields.is_empty()) {
                                    let maximum = (ui.available_height() - 160.0).max(72.0);
                                    egui::Panel::bottom(ui.id().with(("asset_lab_references", asset.id, config.provider_id)))
                                        .resizable(true)
                                        .default_size(inputs_height)
                                        .size_range(inputs_height.min(96.0).min(maximum)..=maximum)
                                        .frame(egui::Frame::new().fill(kit::PANEL).inner_margin(egui::Margin::symmetric(12, 10)))
                                        .show_inside(ui, |ui| {
                                        ui.spacing_mut().item_spacing = Vec2::splat(8.0);
                                        if condensed_shelf {
                                            kit::bounded_horizontal_row(ui, 32.0, |ui, _| {
                                                ui.label(kit::body("References"));
                                                let help = provider.inputs.iter().find(|input| input.name == "prompt")
                                                    .and_then(|input| input.description.as_deref()).unwrap_or("Choose reference sources for this recipe. Add opens the lowest unused slot of that type.");
                                                ui.label(kit::caption("(?)")).on_hover_text(help).on_hover_cursor(egui::CursorIcon::Help);
                                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                if kit::secondary_button(ui, if expanded { "Show used slots" } else { "Show all slots" }, 120.0).clicked() {
                                                    expanded = !expanded;
                                                    ui.data_mut(|data| data.insert_temp(expansion_id, expanded));
                                                }
                                                });
                                            });
                                            ui.separator();
                                        }
                                        kit::scroll_body(ui, |ui| {
                                            if media_fields.is_empty() {
                                                ui.label(kit::caption("No references selected."));
                                            }
                                            for row in media_fields.chunks(columns) {
                                                let row_height = row_height(row);
                                                kit::bounded_horizontal_row(ui, row_height, |ui, row_width| {
                                                    let width = (row_width - 8.0 * (columns - 1) as f32) / columns as f32;
                                                    for field in row {
                                                        ui.allocate_ui_with_layout(Vec2::new(width, row_height),
                                                            Layout::top_down(Align::Min), |ui| {
                                                                self.media_source_picker_field_sized(ui, asset.id, None, provider, field, true);
                                                            });
                                                    }
                                                });
                                            }
                                            if condensed_shelf && !expanded {
                                                ui.horizontal_wrapped(|ui| {
                                                    for field in next_reference_slots(provider, config, &self.editor.project) {
                                                        let label = match field.input_type { ProviderInputType::Image => "+ Add image", ProviderInputType::Video => "+ Add video", _ => "+ Add audio" };
                                                        if kit::secondary_button(ui, label, 112.0).on_hover_text(format!("Choose a source for {}. Existing references keep their slot identities.", field.label)).clicked() {
                                                            self.open_source_picker(asset.id, None, provider, field);
                                                        }
                                                    }
                                                });
                                            }
                                        });
                                    });
                                }
                                self.asset_lab_authoring_canvas(ui, asset, config, provider.as_ref());
                            });
                            strip.cell(|ui| {
                                if results_height == 0.0 {
                                    return;
                                }
                                ui.spacing_mut().item_spacing = Vec2::splat(8.0);
                                ui.painter().rect_filled(ui.max_rect(), 0, kit::PANEL);
                                egui::Frame::new().fill(kit::PANEL).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                                    self.asset_lab_results_v4(ui, asset);
                                });
                                kit::paint_panel_edge(ui, ui.max_rect(), kit::PanelEdge::Top);
                            });
                        });
                });
                strip.cell(|ui| {
                    let rect = ui.max_rect();
                    ui.painter().rect_filled(rect, 0, kit::PANEL);
                    kit::paint_panel_edge(ui, rect, kit::PanelEdge::Left);
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 16)).show(ui, |ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                        self.asset_lab_create_inspector_v4(ui, asset, config, provider.as_ref());
                    });
                });
            });
    }

    fn asset_lab_create_inspector_v4(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: &GenerativeConfig,
        provider: Option<&ProviderEntry>,
    ) {
        let mut setup = AssetLabSnapshot::from_config(config);
        let mut action = None;
        let mut generate = false;
        let blocker = provider.and_then(|provider| {
            crate::state::asset_lab_submission_blocker(config, provider)
                .map(str::to_string)
                .or_else(|| {
                    (!provider_is_available_for_generation(provider)).then(|| {
                        provider_unavailable_reason(provider)
                            .unwrap_or("Recipe unavailable.")
                            .to_string()
                    })
                })
                .or_else(|| {
                    match crate::core::media_binding::resolve_generation_context(
                        &self.editor.project,
                        asset.id,
                        None,
                        self.generation_context_by_asset.get(&asset.id).copied(),
                    ) {
                        Ok(context) => crate::core::generation::preflight_provider_config(
                            &self.editor.project,
                            Some(asset.id),
                            context,
                            provider,
                            config,
                        )
                        .into_iter()
                        .next()
                        .map(|issue| issue.message),
                        Err(error) => Some(error.message("Input")),
                    }
                })
        });
        let body_height =
            (ui.available_height() - if blocker.is_some() { 132.0 } else { 62.0 }).max(40.0);
        let body_bottom = ui.cursor().min.y + body_height;
        egui::ScrollArea::vertical()
            .id_salt("lab_create_inspector")
            .max_height(body_height)
            .min_scrolled_height(body_height)
            .show(ui, |ui| {
                ui.label(kit::caption(format!(
                    "Working from {}",
                    config
                        .lab_authoring
                        .working_version
                        .as_deref()
                        .unwrap_or(if asset.is_video() { "a new video" } else { "a new image" })
                )));
                ui.add_space(12.0);
                crate::egui_app::provider_identity::labeled_provider_combo_field(
                    ui,
                    "Recipe",
                    "lab_v4_recipe",
                    provider,
                    "Choose recipe",
                    |ui| {
                        for entry in &self.editor.provider_entries {
                            if asset_lab_provider_is_compatible(asset, entry)
                                && self.editor.provider_in_project_scope(entry.id)
                                && crate::egui_app::provider_identity::provider_selectable_value(
                                    ui,
                                    &mut setup.provider_id,
                                    Some(entry.id),
                                    entry,
                                )
                                .clicked()
                            {
                                setup.provider_id = Some(entry.id);
                                ui.close();
                            }
                        }
                    },
                );
                if let Some(provider) = provider {
                    let node = AssetLabNode {
                        id: asset.id,
                        parent_node_id: None,
                        provider_id: config.provider_id,
                        inputs: config.inputs.clone(),
                        media_bindings: config.media_bindings.clone(),
                        reference_sizing: config.reference_sizing.clone(),
                        output_version: None,
                    };
                    let prompt_fields: Vec<_> = provider
                        .inputs
                        .iter()
                        .filter(|field| {
                            field.input_type == ProviderInputType::Text
                                && !field.ui.as_ref().is_some_and(|ui| ui.advanced)
                        })
                        .collect();
                    for field in &prompt_fields {
                        self.asset_lab_node_input_field(
                            ui,
                            asset,
                            &node,
                            field,
                            &config.versions,
                            &mut action,
                        );
                    }
                    ui.add_space(12.0);
                    match crate::state::asset_lab_authoring_profile(provider) {
                        crate::state::AssetLabAuthoringProfile::Mask => {
                            ui.separator();
                            ui.label(kit::body("Mask"));
                            kit::bounded_horizontal_row(ui, 32.0, |ui, width| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if kit::tool_toggle_button(ui, "Use mask",
                                    kit::Tooltip::new("Use mask").description("Include the painted mask in generation. Turning this off keeps the painted content and its visibility unchanged. Mask execution is not connected yet."),
                                    setup.authoring.mask_enabled, (width - 40.0).max(0.0)).clicked() {
                                    setup.authoring.mask_enabled = !setup.authoring.mask_enabled;
                                }
                                let visible = self.asset_lab.v4.mask_visible;
                                if kit::tool_button(ui, if visible { kit::Icon::Eye } else { kit::Icon::EyeOff },
                                    kit::Tooltip::new("Show mask overlay").description("Show or hide the painted mask on the canvas. This does not change whether generation uses it."), visible).clicked() {
                                    self.asset_lab.v4.mask_visible = !visible;
                                }
                            });
                            ui.label(kit::caption("Mask authoring · execution not connected"));
                        }
                        crate::state::AssetLabAuthoringProfile::Regions => {
                            ui.separator();
                            ui.label(kit::body("Prompt regions"));
                            if kit::tool_toggle_button(ui, "Use prompt regions",
                                kit::Tooltip::new("Use prompt regions").description("Include the authored regions in generation. Turning this off keeps the regions available for editing. Region execution is not connected yet."),
                                setup.authoring.regions_enabled, ui.available_width()).clicked() {
                                setup.authoring.regions_enabled = !setup.authoring.regions_enabled;
                            }
                            let selected = &mut self.asset_lab.v4.canvas.selected;
                            if selected.is_some_and(|id| !setup.authoring.regions.iter().any(|r| r.id == id)) {
                                *selected = None;
                            }
                            for region in &setup.authoring.regions {
                                if kit::selectable_icon_row(ui, region.id,
                                    if region.text.is_some() { kit::Icon::Text } else { kit::Icon::Object },
                                    &region.name, *selected == Some(region.id)).clicked() {
                                    *selected = Some(region.id);
                                }
                            }
                            let mut delete = None;
                            if let Some(region) = setup.authoring.regions.iter_mut().find(|r| Some(r.id) == *selected) {
                                ui.add_space(8.0);
                                ui.push_id(region.id, |ui| {
                                    kit::bounded_horizontal_row(ui, 32.0, |ui, _| {
                                        ui.label(kit::body(if region.text.is_some() { "Text region" } else { "Object region" }));
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if kit::tool_button(ui, kit::Icon::Trash, "Delete selected region", false).clicked() {
                                                delete = Some(region.id);
                                            }
                                        });
                                    });
                                    kit::labeled_text_field(ui, "Name", &mut region.name);
                                    kit::field_label(ui, "Description");
                                    kit::multiline_text_field(ui, &mut region.description, ui.available_width(),
                                        kit::MultilineTextFieldOptions::rows(3));
                                    if let Some(text) = region.text.as_mut() {
                                        kit::field_label(ui, "Text");
                                        kit::multiline_text_field(ui, text, ui.available_width(),
                                            kit::MultilineTextFieldOptions::rows(2));
                                    }
                                });
                            } else {
                                ui.label(kit::caption("Select a region to edit it, or draw an object or text region on the canvas."));
                            }
                            if let Some(id) = delete {
                                setup.authoring.regions.retain(|r| r.id != id);
                                *selected = None;
                            }
                            ui.label(kit::caption("Region authoring · execution not connected"));
                        }
                        _ => {}
                    }
                    let profile = crate::state::asset_lab_authoring_profile(provider);
                    if profile != crate::state::AssetLabAuthoringProfile::Mask
                        && setup.authoring.mask.is_some()
                    {
                        ui.label(kit::caption("Painted mask kept · inactive for this recipe"));
                    }
                    if profile != crate::state::AssetLabAuthoringProfile::Regions
                        && !setup.authoring.regions.is_empty()
                    {
                        ui.label(kit::caption(format!(
                            "{} prompt regions kept · inactive for this recipe",
                            setup.authoring.regions.len()
                        )));
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    ui.label(kit::body("Generation settings"));
                    if let Some(seed_field) = provider.inputs.iter().find(|field| field.role == Some(InputRole::Seed)) {
                        ui.label(kit::caption(asset_lab_seed_preview(true,
                            asset_lab_node_seed_value(&node, seed_field), setup.batch.count, setup.batch.seed_strategy)));
                    }
                    ui.scope(|ui| {
                            self.asset_lab_canvas_field(ui, &node, provider, &mut action);
                            for field in &provider.inputs {
                                if crate::core::media_binding::bound_media_type_for_input(field)
                                    .is_some()
                                    || prompt_fields.iter().any(|prompt| prompt.name == field.name)
                                    || matches!(
                                        field.role,
                                        Some(InputRole::Width | InputRole::Height)
                                    )
                                {
                                    continue;
                                }
                                self.asset_lab_node_input_field(
                                    ui,
                                    asset,
                                    &node,
                                    field,
                                    &config.versions,
                                    &mut action,
                                );
                            }
                            kit::labeled_combo_field(
                                ui,
                                "Seed strategy",
                                "lab_v4_seed_policy",
                                format!("{:?}", setup.batch.seed_strategy),
                                |ui| {
                                    for strategy in [
                                        SeedStrategy::Keep,
                                        SeedStrategy::Increment,
                                        SeedStrategy::Random,
                                    ] {
                                        if ui
                                            .selectable_label(
                                                strategy == setup.batch.seed_strategy,
                                                format!("{strategy:?}"),
                                            )
                                            .clicked()
                                        {
                                            setup.batch.seed_strategy = strategy;
                                            ui.close();
                                        }
                                    }
                                },
                            );
                        });
                }
            });
        ui.add_space((body_bottom - ui.cursor().min.y).max(0.0));
        ui.separator();
        if let Some(reason) = blocker.as_ref() {
            ui.label(kit::caption(reason));
        }
        ui.add_space((ui.max_rect().bottom() - 36.0 - ui.cursor().min.y).max(0.0));
        kit::bounded_horizontal_row(ui, 36.0, |ui, width| {
            let mut count = setup.batch.count as i64;
            kit::integer_step_drag_sized(
                ui,
                &mut count,
                Vec2::new(48.0, 36.0),
                1,
                Some(1),
                Some(crate::egui_app::MAX_GENERATION_BATCH_COUNT as i64),
            );
            setup.batch.count = count.max(1) as u32;
            ui.add_enabled_ui(provider.is_some() && blocker.is_none(), |ui| {
                generate =
                    kit::primary_button_sized(ui, "Generate", (width - 60.0).max(60.0), 36.0)
                        .clicked();
            });
        });
        match action {
            Some(AssetLabAction::UpdateNodeInput {
                input_name, value, ..
            }) => {
                setup.inputs.insert(input_name, value);
            }
            Some(AssetLabAction::UpdateNodeInputs { values, .. }) => setup.inputs.extend(values),
            _ => {}
        }
        if setup.provider_id != config.provider_id {
            if let Some(next) = self
                .editor
                .provider_entries
                .iter()
                .find(|entry| Some(entry.id) == setup.provider_id)
            {
                crate::core::generation::reconcile_provider_switch_inputs(
                    &mut setup.inputs,
                    provider,
                    next,
                );
            }
        }
        if setup != AssetLabSnapshot::from_config(config) {
            if let Err(error) = self.editor.set_asset_lab_setup(asset.id, &setup) {
                self.editor.status = error;
            } else {
                self.asset_lab
                    .v4
                    .observe(setup, ui.memory(|memory| memory.focused()));
            }
        }
        if generate {
            self.submit_asset_lab_v4(asset.id);
        }
    }

    fn asset_lab_results_v4(&mut self, ui: &mut Ui, asset: &Asset) {
        self.asset_lab.v4.result_rect = Some(ui.max_rect());
        if self.asset_lab.v4.results.is_empty() {
            return;
        }
        for result in &mut self.asset_lab.v4.results {
            if let Some(job) = self
                .editor
                .generation_queue
                .iter()
                .find(|job| job.id == result.job_id)
            {
                result.status = job.status;
                result.version = job.version.clone();
            }
        }
        let ready = self
            .asset_lab
            .v4
            .results
            .iter()
            .filter(|result| {
                result.status == GenerationJobStatus::Succeeded && result.version.is_some()
            })
            .count();
        kit::Tooltip::new("Session results")
            .description("Hover or focus a result to preview it. Click to continue from it, or use its Compare shortcut. Escape dismisses the preview. Closing Asset Lab clears this strip, not your saved versions.")
            .apply(ui.label(kit::caption(format!("Results · {ready} ready"))));
        let session = self.asset_lab.v4.session_id;
        let mut preview = None;
        egui::ScrollArea::horizontal()
            .id_salt("lab_v4_results")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for result in self.asset_lab.v4.results.clone() {
                        ui.push_id(result.job_id, |ui| {
                            ui.vertical(|ui| {
                                if let Some(version) = result
                                    .version
                                    .filter(|_| result.status == GenerationJobStatus::Succeeded)
                                {
                                    let texture = self.asset_lab_node_preview_texture(
                                        ui.ctx(),
                                        asset,
                                        Some(&version),
                                        0.0,
                                    );
                                    let label = self
                                        .editor
                                        .project
                                        .generative_config(asset.id)
                                        .and_then(|config| {
                                            config
                                                .versions
                                                .iter()
                                                .find(|record| record.version == version)
                                        })
                                        .map(|record| record.label.as_str())
                                        .unwrap_or("");
                                    let can_compare = self
                                        .editor
                                        .project
                                        .generative_config(asset.id)
                                        .and_then(|config| config.active_version.as_ref())
                                        .is_some_and(|active| *active != version);
                                    let (tile, compare) = kit::source_tile_with_details(
                                        ui,
                                        "result",
                                        &version,
                                        texture,
                                        self.asset_lab.v4.preview.as_ref() == Some(&version),
                                        false,
                                        Vec2::new(104.0, 76.0),
                                        Some(kit::SourceTileDetails {
                                            caption: label,
                                            can_compare,
                                        }),
                                    );
                                    if tile.hovered()
                                        || tile.has_focus()
                                        || compare.as_ref().is_some_and(|response| {
                                            response.hovered() || response.has_focus()
                                        })
                                    {
                                        preview = Some(version.clone());
                                    }
                                    if compare.is_some_and(|response| response.clicked()) {
                                        self.enter_asset_lab_compare_v4(asset.id, &version);
                                    } else if tile.clicked() {
                                        self.request_asset_lab_adopt(asset.id, &version);
                                    }
                                } else {
                                    let status = match result.status {
                                        GenerationJobStatus::Queued => "Waiting",
                                        GenerationJobStatus::Running => "Generating…",
                                        GenerationJobStatus::Canceling => "Stopping…",
                                        GenerationJobStatus::Succeeded => "Result unavailable",
                                        GenerationJobStatus::Failed => "Failed",
                                        GenerationJobStatus::Canceled => "Canceled",
                                    };
                                    kit::readonly_value_box(ui, status, Vec2::new(104.0, 76.0));
                                }
                            });
                        });
                    }
                });
            });
        if self.asset_lab.v4.session_id != session || self.asset_lab.v4.view != AssetLabView::Create
        {
            return;
        }
        if preview != self.asset_lab.v4.dismissed_preview {
            self.asset_lab.v4.dismissed_preview = None;
        }
        self.asset_lab.v4.preview =
            preview.filter(|version| self.asset_lab.v4.dismissed_preview.as_ref() != Some(version));
    }

    fn request_asset_lab_adopt(&mut self, asset_id: Uuid, version: &str) {
        if self.asset_lab.v4.dirty() {
            self.asset_lab.v4.pending_adopt = Some(version.to_string());
        } else {
            self.adopt_asset_lab_result(asset_id, version);
        }
    }

    fn adopt_asset_lab_result(&mut self, asset_id: Uuid, version: &str) {
        match self
            .editor
            .asset_lab_setup_from_version(asset_id, version)
            .and_then(|setup| self.editor.set_asset_lab_setup(asset_id, &setup))
        {
            Ok(()) => {
                self.asset_lab.selected_version = Some(version.to_string());
                self.asset_lab.compare = None;
                self.asset_lab_preview_texture = None;
                self.reset_asset_lab_session();
            }
            Err(error) => self.editor.status = error,
        }
    }

    fn enter_asset_lab_compare_v4(&mut self, asset_id: Uuid, candidate: &str) {
        self.asset_lab.selected_version = Some(candidate.to_string());
        self.begin_asset_lab_compare(asset_id);
        if self.asset_lab.compare.is_some() {
            self.asset_lab.v4.view = AssetLabView::Compare;
            self.asset_lab.v4.preview = None;
        }
    }

    fn asset_lab_lineage_details_v4(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: &GenerativeConfig,
    ) {
        let Some(record) = config
            .versions
            .iter()
            .find(|record| Some(&record.version) == self.asset_lab.selected_version.as_ref())
        else {
            ui.label(kit::caption("Select a completed version."));
            return;
        };
        StripBuilder::new(ui)
            .clip(true)
            .size(Size::remainder())
            .size(Size::exact(if config.active_version.is_none() {
                144.0
            } else {
                104.0
            }))
            .vertical(|mut strip| {
                strip.cell(|ui| {
                    ui.spacing_mut().scroll.content_margin = egui::Margin::symmetric(20, 16);
                    kit::clipped_scroll_body(ui, "lineage_inspector", |ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                        ui.label(kit::body(&record.version).size(18.0).strong());
                        ui.push_id(("version_label", &record.version), |ui| {
                            let mut label = record.label.clone();
                            if kit::labeled_text_field(ui, "Version label (optional)", &mut label)
                                .on_hover_text("A short note to help you recognize this result. Does not change its submitted prompt or settings.")
                                .changed() {
                                if let Err(error) = self.editor.set_generation_version_label(asset.id, &record.version, &label) {
                                    self.editor.status = error;
                                }
                            }
                        });
                        if config.active_version.as_ref() == Some(&record.version) {
                            kit::bounded_horizontal_row(ui, 20.0, |ui, _| {
                                let (rect, _) =
                                    ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                                kit::paint_icon(ui, kit::Icon::Pin, rect, kit::PRIMARY);
                                ui.label(kit::caption("Current output").color(kit::PRIMARY));
                            });
                        }
                        if let Some(parent) = record
                            .lab_node_id
                            .and_then(|id| completed_parent(config, id))
                            .and_then(|id| {
                                config.versions.iter().find(|r| r.lab_node_id == Some(id))
                            })
                        {
                            ui.label(kit::caption(format!("Created from {}", parent.version)));
                        }
                        ui.separator();
                        ui.add_space(4.0);
                        kit::field_label(ui, "Recipe");
                        ui.label(kit::body(
                            self.editor
                                .provider_entries
                                .iter()
                                .find(|provider| provider.id == record.provider_id)
                                .map(|p| p.name.as_str())
                                .unwrap_or("Unavailable recipe"),
                        ));
                        let provider = self
                            .editor
                            .provider_entries
                            .iter()
                            .find(|p| p.id == record.provider_id)
                            .cloned();
                        let mut settings = Vec::new();
                        let mut values: Vec<_> = record.inputs_snapshot.iter().collect();
                        values.sort_by_key(|(name, _)| *name);
                        for (name, value) in values {
                            let field = provider
                                .as_ref()
                                .and_then(|p| p.inputs.iter().find(|f| f.name == *name));
                            let is_prompt = field.is_some_and(|f| {
                                f.input_type == ProviderInputType::Text
                                    && !f.ui.as_ref().is_some_and(|ui| ui.advanced)
                            });
                            if let InputValue::Literal { value } = value {
                                let text = value
                                    .as_str()
                                    .map(str::to_string)
                                    .unwrap_or_else(|| value.to_string());
                                let label = field.map(|f| f.label.as_str()).unwrap_or(name);
                                if is_prompt {
                                    ui.add_space(16.0);
                                    let authored = record.authoring_snapshot.as_ref().and_then(|snapshot| snapshot.inputs.get(name));
                                    let has_references = matches!(authored, Some(InputValue::Prompt { .. }));
                                    kit::field_label(ui, &if has_references { format!("{label} sent") } else { label.to_string() });
                                    ui.add(
                                        egui::Label::new(kit::body(text)).wrap().selectable(true),
                                    );
                                    if let Some(InputValue::Prompt { text, .. }) = authored {
                                        egui::CollapsingHeader::new("Authored prompt").id_salt((record.version.as_str(), name)).show(ui, |ui| {
                                            ui.add(egui::Label::new(text).wrap().selectable(true));
                                        });
                                    }
                                } else if !record.resolved_media_inputs.contains_key(name) {
                                    settings.push((label.to_string(), text));
                                }
                            }
                        }
                        if !record.resolved_media_inputs.is_empty() {
                            ui.add_space(16.0);
                            kit::field_label(ui, "Inputs used");
                        }
                        let mut sources: Vec<_> = record.resolved_media_inputs.iter().collect();
                        sources.sort_by_key(|(name, _)| *name);
                        for (name, source) in sources {
                            let path = self
                                .editor
                                .project
                                .project_path
                                .as_ref()
                                .map(|root| root.join(&source.materialized_path))
                                .unwrap_or_else(|| source.materialized_path.clone());
                            let source_asset = match source.media_type {
                                crate::state::BoundMediaType::Image => {
                                    Asset::new_image("Submitted input", path.clone())
                                }
                                crate::state::BoundMediaType::Video => {
                                    Asset::new_video("Submitted input", path.clone())
                                }
                                crate::state::BoundMediaType::Audio => {
                                    Asset::new_audio("Submitted input", path.clone())
                                }
                            };
                            let mut source_asset = source_asset;
                            source_asset.id = source.source_asset_id.unwrap_or(asset.id);
                            let thumbnail = self.asset_lab_path_preview_texture(
                                ui.ctx(),
                                &source_asset,
                                source.source_version.as_deref(),
                                0.0,
                                &path,
                            );
                            let filename = source.source_path.as_ref()
                                .and_then(|path| path.file_name()).and_then(|name| name.to_str())
                                .unwrap_or("Submitted media");
                            let title = source.source_version.as_ref()
                                .map(|version| format!("{version} · {filename}"))
                                .unwrap_or_else(|| filename.to_string());
                            let slot = provider.as_ref()
                                .and_then(|provider| provider.inputs.iter().find(|field| field.name == *name))
                                .map(|field| field.label.as_str()).unwrap_or(name);
                            let sample = if let Some(seconds) = source.source_frame_time {
                                format!("frame {}", crate::core::media_binding::format_timecode(seconds))
                            } else if let Some(range) = &source.source_range {
                                format!("{}–{}", crate::core::media_binding::format_timecode(range.start_seconds),
                                    crate::core::media_binding::format_timecode(range.end_seconds))
                            } else { "submitted media".into() };
                            kit::media_info_row(ui, &title, &format!("{slot} · {sample}"), thumbnail);
                        }
                        if !settings.is_empty() {
                            ui.add_space(12.0);
                            ui.separator();
                            let disclosure =
                                egui::CollapsingHeader::new(kit::body("Generation settings"))
                                    .id_salt("lineage_settings")
                                    .show(ui, |ui| {
                                        ui.label(kit::caption("Values used for this version"));
                                        kit::card_frame().show(ui, |ui| {
                                            for (index, (label, value)) in
                                                settings.iter().enumerate()
                                            {
                                                if index > 0 {
                                                    ui.separator();
                                                }
                                                kit::property_row(ui, label, value);
                                            }
                                        });
                                    });
                            crate::core::automation::instrument_response(
                                disclosure.header_response,
                                "disclosure",
                                Some("Generation settings".into()),
                                false,
                                false,
                            );
                        }
                    });
                });
                strip.cell(|ui| {
                    kit::paint_panel_edge(ui, ui.max_rect(), kit::PanelEdge::Top);
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(20, 16))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = Vec2::splat(8.0);
                            if kit::primary_button(
                                ui,
                                &format!("Create from {}", record.version),
                                ui.available_width(),
                            )
                            .clicked()
                            {
                                self.request_asset_lab_adopt(asset.id, &record.version);
                            }
                            ui.add_enabled_ui(
                                config
                                    .active_version
                                    .as_ref()
                                    .is_some_and(|active| *active != record.version),
                                |ui| {
                                    if kit::secondary_button(
                                        ui,
                                        "Compare with current",
                                        ui.available_width(),
                                    )
                                    .clicked()
                                    {
                                        self.enter_asset_lab_compare_v4(asset.id, &record.version);
                                    }
                                },
                            );
                            if config.active_version.is_none()
                                && kit::secondary_button(
                                    ui,
                                    "Use this output",
                                    ui.available_width(),
                                )
                                .clicked()
                            {
                                self.pin_asset_lab_result_v4(asset.id, &record.version);
                            }
                        });
                });
            });
    }

    pub(in crate::egui_app::asset_lab) fn pin_asset_lab_result_v4(
        &mut self,
        asset_id: Uuid,
        version: &str,
    ) {
        let previous = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(|config| config.active_version.clone());
        if let Err(error) = self.set_generative_active_version(asset_id, version) {
            self.editor.status = error;
            return;
        }
        self.asset_lab.v4.pin_undo = previous;
        self.asset_lab.compare = None;
        self.asset_lab.v4.view = AssetLabView::Lineage;
    }

    fn asset_lab_lineage_v4(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: &GenerativeConfig,
        compact: bool,
    ) {
        {
            kit::bounded_horizontal_row(ui, 32.0, |ui, _| {
                if compact {
                    ui.label(kit::body("Lineage"));
                }
                ui.label(kit::caption(format!("{} versions", config.versions.len())));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if kit::tool_button(
                        ui,
                        kit::Icon::Fit,
                        kit::Tooltip::new("Fit lineage")
                            .description("Show all completed versions in the lineage map."),
                        false,
                    )
                    .clicked()
                    {
                        if compact {
                            self.asset_lab.v4.compare_map_zoom = 1.0;
                            self.asset_lab.v4.compare_map_pan = Vec2::ZERO;
                        } else {
                            self.asset_lab.graph_zoom = 1.0;
                            self.asset_lab.graph_pan = Vec2::ZERO;
                        }
                    }
                });
            });
        }
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::splat(1.0)),
            Sense::click_and_drag(),
        );
        ui.painter().rect_filled(rect, 3, kit::PANEL_SUNKEN);
        if config.versions.is_empty() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Completed results will appear here.",
                FontId::proportional(12.0),
                kit::TEXT_MUTED,
            );
            return;
        }
        let positions = lineage_positions(config);
        let node_size = if compact {
            Vec2::new(76.0, 56.0)
        } else {
            Vec2::new(136.0, 108.0)
        };
        let step = node_size
            + if compact {
                Vec2::new(24.0, 14.0)
            } else {
                Vec2::new(34.0, 56.0)
            };
        let extent_x = positions.iter().map(|(_, x, _)| *x).fold(0.0_f32, f32::max) + 1.0;
        let extent_y = positions
            .iter()
            .map(|(_, _, depth)| *depth)
            .max()
            .unwrap_or(0) as f32
            + 1.0;
        let fit = ((rect.width() - 24.0) / (extent_x * step.x))
            .min((rect.height() - 30.0) / (extent_y * step.y))
            .min(1.0)
            .max(0.1);
        let (mut zoom, mut pan) = if compact {
            (
                self.asset_lab.v4.compare_map_zoom,
                self.asset_lab.v4.compare_map_pan,
            )
        } else {
            (self.asset_lab.graph_zoom, self.asset_lab.graph_pan)
        };
        let scroll = preview_scroll_delta(ui, rect);
        if scroll != 0.0 {
            let old_zoom = zoom;
            let factor = canvas_wheel_zoom_factor(scroll);
            zoom = (old_zoom * factor).clamp(0.2, 6.0);
            if let Some(pointer) = ui.ctx().pointer_hover_pos() {
                pan = pointer - rect.center() - (pointer - rect.center() - pan) * (zoom / old_zoom);
            }
        }
        if response.dragged_by(egui::PointerButton::Secondary) {
            pan += response.drag_delta();
        }
        if response.double_clicked() {
            zoom = 1.0;
            pan = Vec2::ZERO;
        }
        if compact {
            self.asset_lab.v4.compare_map_zoom = zoom;
            self.asset_lab.v4.compare_map_pan = pan;
        } else {
            self.asset_lab.graph_zoom = zoom;
            self.asset_lab.graph_pan = pan;
        }
        let scale = fit * zoom;
        let mut nodes = HashMap::new();
        for (id, x, depth) in &positions {
            let center = rect.center()
                + Vec2::new(
                    (*x - (extent_x - 1.0) * 0.5) * step.x,
                    ((extent_y - 1.0) * 0.5 - *depth as f32) * step.y,
                ) * scale
                + pan;
            nodes.insert(*id, Rect::from_center_size(center, node_size * scale));
        }
        let painter = ui.painter_at(rect);
        for node in &config.lab_graph.nodes {
            if let (Some(child), Some(parent)) = (
                nodes.get(&node.id),
                completed_parent(config, node.id).and_then(|id| nodes.get(&id)),
            ) {
                let from = parent.center_top();
                let to = child.center_bottom();
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [
                        from,
                        from - Vec2::new(0.0, step.y * scale * 0.3),
                        to + Vec2::new(0.0, step.y * scale * 0.3),
                        to,
                    ],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(1.0_f32, kit::IMAGE.gamma_multiply(0.5)),
                ));
                let arrow = (4.0 * scale).clamp(2.0, 5.0);
                for side in [-1.0, 1.0] {
                    painter.line_segment(
                        [to + Vec2::new(side * arrow, arrow * 1.5), to],
                        Stroke::new(1.0_f32, kit::IMAGE.gamma_multiply(0.5)),
                    );
                }
            }
        }
        for record in &config.versions {
            let Some(node_rect) = record.lab_node_id.and_then(|id| nodes.get(&id)).copied() else {
                continue;
            };
            if !node_rect.intersects(rect) {
                continue;
            }
            let preview =
                self.asset_lab_node_preview_texture(ui.ctx(), asset, Some(&record.version), 0.0);
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(node_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            child.set_clip_rect(rect.intersect(node_rect));
            let (response, compare) = kit::source_tile_with_details(
                &mut child,
                ("lineage", compact, &record.version),
                &record.version,
                preview,
                self.asset_lab.selected_version.as_ref() == Some(&record.version),
                config.active_version.as_ref() == Some(&record.version),
                node_rect.size(),
                Some(kit::SourceTileDetails {
                    caption: if compact { "" } else { &record.label },
                    can_compare: !compact
                        && config
                            .active_version
                            .as_ref()
                            .is_some_and(|active| *active != record.version),
                }),
            );
            let compare_clicked = compare.is_some_and(|response| response.clicked());
            if compare_clicked {
                self.enter_asset_lab_compare_v4(asset.id, &record.version);
            } else if response.clicked() {
                self.asset_lab.selected_version = Some(record.version.clone());
                if compact {
                    self.enter_asset_lab_compare_v4(asset.id, &record.version);
                }
            }
            if !compare_clicked && response.double_clicked() {
                self.request_asset_lab_adopt(asset.id, &record.version);
            }
            response.context_menu(|ui| {
                if kit::field_button(ui, &format!("Create from {}", record.version), 170.0)
                    .clicked()
                {
                    self.request_asset_lab_adopt(asset.id, &record.version);
                    ui.close();
                }
                ui.add_enabled_ui(
                    config
                        .active_version
                        .as_ref()
                        .is_some_and(|active| *active != record.version),
                    |ui| {
                        if kit::field_button(ui, "Compare with current", 170.0).clicked() {
                            self.enter_asset_lab_compare_v4(asset.id, &record.version);
                            ui.close();
                        }
                    },
                );
            });
        }
        if !compact {
            painter.text(
                rect.left_bottom() + Vec2::new(12.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                "Right-drag to pan · Double-click a version to continue",
                FontId::proportional(11.0),
                kit::TEXT_MUTED,
            );
        }
    }

    fn asset_lab_compare_v4(&mut self, ui: &mut Ui, asset: &Asset, config: &GenerativeConfig) {
        self.poll_asset_lab_compare_timing_requests(ui.ctx());
        self.poll_asset_lab_compare_video_requests(ui.ctx());
        let Some(compare) = self.asset_lab.compare.clone() else {
            self.asset_lab.v4.view = AssetLabView::Lineage;
            return;
        };
        let total = ui.available_height();
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        ui.painter()
            .rect_filled(ui.available_rect_before_wrap(), 0, kit::PANEL_SUNKEN);
        let map = egui::Panel::bottom(
            ui.id()
                .with(("compare_lineage", self.asset_lab.v4.session_id)),
        )
        .resizable(true)
        .default_size(self.asset_lab.v4.map_height)
        .size_range(100.0..=(total * 0.6).max(100.0))
        .frame(
            egui::Frame::new()
                .fill(kit::PANEL_SUNKEN)
                .inner_margin(egui::Margin::symmetric(16, 8)),
        )
        .show_inside(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 4.0);
            self.asset_lab_lineage_v4(ui, asset, config, true);
        });
        self.asset_lab.v4.map_height = map.response.rect.height();
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(12.0, 8.0);
                let mut timings = Vec::new();
                for version in [
                    Some(compare.baseline_version.as_str()),
                    compare.candidate_version.as_deref(),
                ] {
                    let record = asset_lab_record_for_version(Some(config), version);
                    let timing = if asset.is_video() {
                        version.and_then(|version| {
                            match self.asset_lab_compare_version_timing(
                                ui.ctx(),
                                asset,
                                version,
                                record,
                            ) {
                                AssetLabTimingLookup::Ready(timing) => Some(timing),
                                AssetLabTimingLookup::Pending => None,
                            }
                        })
                    } else {
                        Some(asset_lab_compare_resolve_timing(
                            asset,
                            record,
                            &self.editor.provider_entries,
                            None,
                            self.editor.project.settings.fps,
                        ))
                    };
                    timings.push(timing);
                }
                let max_duration = AssetLabCompareState::max_duration(
                    timings[0].map(|t| t.duration_seconds).unwrap_or(0.0),
                    timings[1].map(|t| t.duration_seconds),
                );
                if timings.iter().all(Option::is_some) {
                    self.advance_asset_lab_compare_playback(ui.ctx(), max_duration);
                }
                let height =
                    (ui.available_height() - if asset.is_video() { 84.0 } else { 44.0 }).max(80.0);
                let width = (ui.available_width() - 12.0) * 0.5;
                let mut action = None;
                kit::bounded_horizontal_row(ui, height, |ui, _| {
                    for (index, side) in [
                        AssetLabCompareSide::Baseline,
                        AssetLabCompareSide::Candidate,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let version = if index == 0 {
                            Some(compare.baseline_version.as_str())
                        } else {
                            compare.candidate_version.as_deref()
                        };
                        let duration = timings[index]
                            .map(|timing| timing.duration_seconds)
                            .unwrap_or(0.0);
                        let preview = self.asset_lab_compare_pane_preview(
                            ui.ctx(),
                            asset,
                            side,
                            version,
                            self.asset_lab
                                .compare
                                .as_ref()
                                .map(|compare| compare.side_time(duration))
                                .unwrap_or(0.0),
                            timings[index],
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, height),
                            Layout::top_down(Align::Min),
                            |ui| {
                                self.paint_asset_lab_compare_pane(
                                    ui,
                                    asset,
                                    Some(config),
                                    side,
                                    version,
                                    config.active_version.as_deref(),
                                    duration,
                                    compare.side_has_ended(duration, max_duration),
                                    preview,
                                    &mut action,
                                )
                            },
                        );
                    }
                });
                if let Some(action) = action {
                    self.handle_asset_lab_action(asset.id, action);
                }
                kit::bounded_horizontal_row(ui, 36.0, |ui, row_width| {
                    ui.label(kit::caption(
                        "Linked views · wheel to zoom · right-drag to pan",
                    ));
                    let controls = 108.0;
                    let start = ui.max_rect().left() + (row_width - controls) * 0.5;
                    ui.add_space((start - ui.cursor().min.x).max(0.0));
                    if kit::tool_button(ui, kit::Icon::Minus, "Zoom out", false).clicked() {
                        self.asset_lab.preview_zoom =
                            (self.asset_lab.preview_zoom / 1.2).clamp(0.1, PREVIEW_ZOOM_MAX);
                        self.asset_lab.preview_auto_fit = false;
                    }
                    if kit::tool_button(
                        ui,
                        kit::Icon::Fit,
                        kit::Tooltip::new("Fit both images")
                            .description("Fit and center both images in their panes."),
                        false,
                    )
                    .clicked()
                    {
                        self.asset_lab.preview_auto_fit = true;
                    }
                    if kit::tool_button(ui, kit::Icon::Plus, "Zoom in", false).clicked() {
                        self.asset_lab.preview_zoom =
                            (self.asset_lab.preview_zoom * 1.2).clamp(0.1, PREVIEW_ZOOM_MAX);
                        self.asset_lab.preview_auto_fit = false;
                    }
                });
                if asset.is_video() {
                    self.asset_lab_compare_transport(ui, max_duration);
                }
            });
    }

    pub(in crate::egui_app) fn submit_asset_lab_v4(&mut self, asset_id: Uuid) {
        let result = (|| {
            let config = self
                .editor
                .project
                .generative_config(asset_id)
                .cloned()
                .ok_or("Asset unavailable")?;
            let provider = self
                .editor
                .provider_entries
                .iter()
                .find(|provider| Some(provider.id) == config.provider_id)
                .cloned()
                .ok_or("Choose a recipe")?;
            let asset = self
                .editor
                .project
                .find_asset(asset_id)
                .cloned()
                .ok_or("Asset unavailable")?;
            let folder = self
                .editor
                .project
                .project_path
                .as_ref()
                .ok_or("Save the project first")?
                .join(generative_folder_for_asset(&asset).ok_or("Asset folder unavailable")?);
            let parent = config
                .lab_authoring
                .working_version
                .as_ref()
                .and_then(|version| {
                    config
                        .versions
                        .iter()
                        .find(|record| &record.version == version)
                })
                .and_then(|record| record.lab_node_id);
            let overlapping = self.editor.generation_queue.iter().any(|job| {
                job.asset_id == asset_id
                    && matches!(
                        job.status,
                        GenerationJobStatus::Queued
                            | GenerationJobStatus::Running
                            | GenerationJobStatus::Canceling
                    )
            });
            let submission = AssetLabSubmission {
                session_id: self.asset_lab.v4.session_id,
                revision: self.asset_lab.v4.revision,
                parent_node_id: parent,
                allow_advance: config.batch.count == 1 && !overlapping,
            };
            let context = crate::core::media_binding::resolve_generation_context(
                &self.editor.project,
                asset_id,
                None,
                self.generation_context_by_asset.get(&asset_id).copied(),
            )
            .map_err(|error| error.message("Input"))?;
            let mut status = self.enqueue_generation_jobs(
                asset_id,
                context,
                None,
                provider,
                config,
                folder,
                asset.name.clone(),
                Some(submission),
            )?;
            if overlapping {
                for job in &mut self.editor.generation_queue {
                    if job.asset_id == asset_id {
                        if let Some(submission) = &mut job.lab_submission {
                            submission.allow_advance = false;
                        }
                    }
                }
            }
            for job in &self.editor.generation_queue {
                if job
                    .lab_submission
                    .as_ref()
                    .is_some_and(|submission| submission.session_id == self.asset_lab.v4.session_id)
                    && !self
                        .asset_lab
                        .v4
                        .results
                        .iter()
                        .any(|result| result.job_id == job.id)
                {
                    self.asset_lab.v4.results.push(LabResult {
                        job_id: job.id,
                        version: None,
                        status: job.status,
                    });
                }
            }
            if let Some(advance) = self
                .editor
                .generation_queue
                .iter()
                .rev()
                .find(|job| {
                    job.asset_id == asset_id
                        && job.lab_submission.as_ref().is_some_and(|submission| {
                            submission.session_id == self.asset_lab.v4.session_id
                        })
                })
                .and_then(|job| job.seed_advance.clone())
            {
                self.editor
                    .project
                    .update_generative_config(asset_id, |config| {
                        config.inputs.insert(
                            advance.field,
                            InputValue::Literal {
                                value: crate::core::generation::seed_input_value(advance.next_seed),
                            },
                        );
                    });
                if let Err(error) = self.editor.project.save_generative_config(asset_id) {
                    status = format!("{status}; queued, but saving the next seed failed: {error}");
                }
            }
            self.asset_lab.v4.undo.clear();
            self.asset_lab.v4.edit_group = None;
            let setup = self
                .editor
                .project
                .generative_config(asset_id)
                .map(AssetLabSnapshot::from_config);
            self.asset_lab.v4.baseline = setup.clone();
            self.asset_lab.v4.observed = setup;
            Ok::<_, String>(status)
        })();
        self.editor.status = result.unwrap_or_else(|error| error);
    }

    pub(in crate::egui_app) fn asset_lab_v4_completed(
        &mut self,
        job: &GenerationJob,
        version: &str,
    ) {
        let Some(submission) = &job.lab_submission else {
            return;
        };
        if self.asset_lab.asset_id != Some(job.asset_id)
            || submission.session_id != self.asset_lab.v4.session_id
        {
            return;
        }
        if let Some(result) = self
            .asset_lab
            .v4
            .results
            .iter_mut()
            .find(|result| result.job_id == job.id)
        {
            result.version = Some(version.to_string());
            result.status = GenerationJobStatus::Succeeded;
        }
        if let Some(config) = self.editor.project.generative_config(job.asset_id) {
            self.asset_lab
                .v4
                .observe(AssetLabSnapshot::from_config(config), None);
        }
        if self
            .asset_lab
            .v4
            .can_advance(submission, self.source_picker.is_some())
        {
            let view = self.asset_lab.v4.view;
            self.adopt_asset_lab_result(job.asset_id, version);
            self.asset_lab.v4.view = view;
        }
    }
}

fn completed_parent(config: &GenerativeConfig, id: Uuid) -> Option<Uuid> {
    let mut seen = std::collections::HashSet::from([id]);
    let mut current = id;
    loop {
        let parent = config
            .lab_graph
            .nodes
            .iter()
            .find(|node| node.id == current)?
            .parent_node_id?;
        if !seen.insert(parent) {
            return None;
        }
        if config
            .versions
            .iter()
            .any(|record| record.lab_node_id == Some(parent))
        {
            return Some(parent);
        }
        current = parent;
    }
}

fn lineage_positions(config: &GenerativeConfig) -> Vec<(Uuid, f32, usize)> {
    fn place(
        id: Uuid,
        depth: usize,
        children: &HashMap<Uuid, Vec<Uuid>>,
        seen: &mut std::collections::HashSet<Uuid>,
        next_leaf: &mut f32,
        positions: &mut Vec<(Uuid, f32, usize)>,
    ) -> Option<f32> {
        if !seen.insert(id) {
            return None;
        }
        let child_positions: Vec<_> = children
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|child| place(*child, depth + 1, children, seen, next_leaf, positions))
            .collect();
        let x = if let (Some(first), Some(last)) = (child_positions.first(), child_positions.last())
        {
            (first + last) * 0.5
        } else {
            let x = *next_leaf;
            *next_leaf += 1.0;
            x
        };
        positions.push((id, x, depth));
        Some(x)
    }

    let ids: Vec<_> = config
        .versions
        .iter()
        .filter_map(|r| r.lab_node_id)
        .collect();
    let mut children: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    let mut roots = Vec::new();
    for id in &ids {
        if let Some(parent) = completed_parent(config, *id) {
            children.entry(parent).or_default().push(*id);
        } else {
            roots.push(*id);
        }
    }
    let mut positions = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut next_leaf = 0.0;
    // The second pass also keeps malformed cyclic legacy nodes visible, once each.
    for id in roots.into_iter().chain(ids) {
        place(id, 0, &children, &mut seen, &mut next_leaf, &mut positions);
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_lab_v4_lineage_keeps_continuations_above_parents() {
        let mut config = GenerativeConfig::default();
        let mut ids = Vec::new();
        // v1 -> v2 -> (v3, v4, v5), v3 -> v6, v6 -> (v7, v8), plus another root.
        for parent in [
            None,
            Some(0),
            Some(1),
            Some(1),
            Some(1),
            Some(2),
            Some(5),
            Some(5),
            None,
        ] {
            let mut node = AssetLabNode::new(None);
            node.parent_node_id = parent.map(|index| ids[index]);
            let version = format!("v{}", ids.len() + 1);
            node.output_version = Some(version.clone());
            config.versions.push(
                serde_json::from_value(serde_json::json!({
                    "version": version, "timestamp": chrono::Utc::now(),
                    "provider_id": Uuid::nil(), "inputs_snapshot": {}, "lab_node_id": node.id,
                }))
                .unwrap(),
            );
            ids.push(node.id);
            config.lab_graph.nodes.push(node);
        }
        let positions = lineage_positions(&config);
        let at = |index| {
            positions
                .iter()
                .find(|(id, _, _)| *id == ids[index])
                .unwrap()
        };
        assert_eq!(positions.len(), 9);
        assert_eq!(at(0).1, at(1).1);
        assert_eq!(at(2).1, at(5).1, "v6 must stay directly above v3");
        assert_eq!(at(5).2, at(2).2 + 1);
        assert!(at(2).1 < at(3).1 && at(3).1 < at(4).1);
        assert!(at(6).1 < at(5).1 && at(5).1 < at(7).1);
        for (i, (_, x, depth)) in positions.iter().enumerate() {
            for (_, other_x, other_depth) in positions.iter().skip(i + 1) {
                if depth == other_depth {
                    assert!((x - other_x).abs() >= 1.0, "branches must not overlap");
                }
            }
        }
        // Legacy unfinished nodes are skipped without changing visible ancestry.
        let mut unfinished = AssetLabNode::new(None);
        unfinished.parent_node_id = Some(ids[2]);
        config.lab_graph.nodes[5].parent_node_id = Some(unfinished.id);
        config.lab_graph.nodes.push(unfinished);
        assert_eq!(lineage_positions(&config), positions);
    }

    #[test]
    fn asset_lab_v4_late_completion_cannot_replace_edits_or_audition() {
        let setup = AssetLabSnapshot::from_config(&GenerativeConfig::default());
        let mut state = AssetLabV4State {
            baseline: Some(setup.clone()),
            observed: Some(setup.clone()),
            ..Default::default()
        };
        let submission = AssetLabSubmission {
            session_id: state.session_id,
            revision: 0,
            parent_node_id: None,
            allow_advance: true,
        };
        assert!(state.can_advance(&submission, false));
        state.mask_visible = false;
        assert!(!state.dirty());
        assert!(state.can_advance(&submission, false));
        state.preview = Some("v2".into());
        assert!(!state.can_advance(&submission, false));
        assert!(!state.dirty());
        state.preview = None;
        state.view = AssetLabView::Compare;
        assert!(!state.can_advance(&submission, false));
        state.view = AssetLabView::Create;
        assert!(!state.can_advance(&submission, true));
        state.interacting = true;
        assert!(!state.can_advance(&submission, false));
        state.interacting = false;
        let mut edited = setup.clone();
        edited.authoring.regions_enabled = false;
        state.observe(edited, None);
        assert!(!state.can_advance(&submission, false));
        state.observe(setup, None); // Editing and reverting still supersedes the pending request.
        assert!(!state.can_advance(&submission, false));
        state.revision = 0;
        state.session_id = Uuid::new_v4();
        assert!(!state.can_advance(&submission, false));
    }

    #[test]
    fn asset_lab_v4_undo_is_coalesced_and_bounded() {
        let mut setup = AssetLabSnapshot::from_config(&GenerativeConfig::default());
        let mut state = AssetLabV4State::default();
        state.observe(setup.clone(), None);
        let focus = Some(egui::Id::new("prompt"));
        for n in 0..8 {
            setup.inputs.insert(
                "prompt".into(),
                InputValue::Literal {
                    value: serde_json::json!(n),
                },
            );
            state.observe(setup.clone(), focus);
        }
        assert_eq!(state.undo.len(), 1);
        for n in 8..40 {
            setup.inputs.insert(
                "prompt".into(),
                InputValue::Literal {
                    value: serde_json::json!(n),
                },
            );
            state.observe(setup.clone(), None);
        }
        assert_eq!(state.undo.len(), 20);
        state.view = AssetLabView::Compare;
        state.view = AssetLabView::Create;
        assert_eq!(state.undo.len(), 20);
    }
}

/// Keep source identities and catalog order while progressively revealing optional slots.
fn next_reference_slots<'a>(
    provider: &'a ProviderEntry,
    config: &GenerativeConfig,
    project: &crate::state::Project,
) -> Vec<&'a ProviderInputField> {
    let used = reference_shelf_fields(provider, config, project, false);
    let mut kinds = Vec::new();
    reference_shelf_fields(provider, config, project, true)
        .into_iter()
        .filter(|field| {
            let kind = crate::core::media_binding::bound_media_type_for_input(field).unwrap();
            if used.iter().any(|used| used.name == field.name) || kinds.contains(&kind) {
                return false;
            }
            kinds.push(kind);
            true
        })
        .collect()
}

fn reference_shelf_fields<'a>(
    provider: &'a ProviderEntry,
    config: &GenerativeConfig,
    project: &crate::state::Project,
    expanded: bool,
) -> Vec<&'a ProviderInputField> {
    provider
        .inputs
        .iter()
        .filter(|field| {
            let Some(_) = crate::core::media_binding::bound_media_type_for_input(field) else {
                return false;
            };
            if field.paired_video_input.is_some() {
                return false;
            }
            let occupied = crate::core::media_binding::lookup_media_binding(config, field, project)
                .is_some()
                || provider.inputs.iter().any(|paired| {
                    paired.paired_video_input.as_deref() == Some(field.name.as_str())
                        && crate::core::media_binding::lookup_media_binding(config, paired, project)
                            .is_some()
                });
            expanded || occupied
        })
        .collect()
}

#[cfg(test)]
mod reference_shelf_tests {
    use super::*;
    #[test]
    fn reference_shelf_shows_only_used_slots_and_preserves_sparse_identities() {
        let mut provider = ProviderEntry::new(
            "Mixed references",
            crate::state::ProviderOutputType::Video,
            crate::state::ProviderConnection::CustomHttp {
                base_url: "http://localhost".into(),
                api_key: None,
            },
        );
        for (prefix, kind, count) in [
            ("image", ProviderInputType::Image, 9),
            ("video", ProviderInputType::Video, 3),
            ("audio", ProviderInputType::Audio, 3),
        ] {
            for index in 1..=count {
                provider.inputs.push(ProviderInputField {
                    name: format!("{prefix}_{index}"),
                    label: format!("{prefix} {index}"),
                    description: None,
                    input_type: kind.clone(),
                    required: false,
                    default: None,
                    role: None,
                    ui: None,
                    image_dimensions: None,
                    paired_video_input: None,
                    prompt_reference_token: None,
                    ordered_collection: false,
                });
            }
        }
        let mut paired = provider.inputs.last().unwrap().clone();
        paired.name = "soundtrack_2".into();
        paired.paired_video_input = Some("video_2".into());
        provider.inputs.push(paired);
        let project = crate::state::Project::new("Shelf");
        let mut config = GenerativeConfig::default();
        let names = |config: &GenerativeConfig, all| {
            reference_shelf_fields(&provider, config, &project, all)
                .into_iter()
                .map(|field| field.name.clone())
                .collect::<Vec<_>>()
        };
        assert!(names(&config, false).is_empty());
        for name in ["image_3", "image_6", "audio_3"] {
            config.media_bindings.insert(
                name.into(),
                crate::state::MediaBindingSpec {
                    source: crate::state::MediaBindingSource::WorkingOutput,
                    ..Default::default()
                },
            );
        }
        config.media_bindings.insert(
            "soundtrack_2".into(),
            crate::state::MediaBindingSpec {
                source: crate::state::MediaBindingSource::PairedVideoInput {
                    field: "video_2".into(),
                },
                ..Default::default()
            },
        );
        assert_eq!(
            names(&config, false),
            ["image_3", "image_6", "video_2", "audio_3"]
        );
        assert_eq!(names(&config, true).len(), 15);
        let next = |config: &GenerativeConfig| next_reference_slots(&provider, config, &project).iter().map(|field| field.name.clone()).collect::<Vec<_>>();
        assert_eq!(next(&config), ["image_1", "video_1", "audio_1"]);
        for index in 1..=9 {
            config.media_bindings.insert(format!("image_{index}"), crate::state::MediaBindingSpec { source: crate::state::MediaBindingSource::WorkingOutput, ..Default::default() });
            if index == 1 { assert_eq!(next(&config)[0], "image_2"); }
        }
        assert_eq!(next(&config), ["video_1", "audio_1"]);
        config.media_bindings.remove("image_3");
        assert_eq!(next(&config)[0], "image_3");
        assert!(names(&config, false).contains(&"image_6".into()));
    }
}
