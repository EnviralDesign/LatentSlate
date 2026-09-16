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
        let mut close = false;
        kit::bounded_horizontal_row(ui, 48.0, |ui, width| {
            let side = ((width - 268.0) * 0.5).max(100.0);
            ui.allocate_ui_with_layout(Vec2::new(side, 48.0), Layout::top_down(Align::Min), |ui| {
                ui.set_min_width(side);
                ui.label(kit::body("Asset Lab"));
                ui.label(kit::caption(&asset.name));
            });
            for (view, label) in [
                (AssetLabView::Create, "Create"),
                (AssetLabView::Lineage, "Lineage"),
                (AssetLabView::Compare, "Compare"),
            ] {
                let enabled = !self.asset_lab.v4.interacting
                    && (view != AssetLabView::Compare || can_compare);
                let response =
                    kit::workspace_tab(ui, label, self.asset_lab.v4.view == view, enabled);
                if !enabled {
                    response.clone().on_hover_text("Choose a result in Lineage, or use Compare on a completed result thumbnail.");
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

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = kit::icon_button(ui, "×")
                    .on_hover_text("Close Asset Lab")
                    .clicked();
                if let Some(pin) = &config.active_version {
                    ui.label(kit::caption(format!("◆ {pin}")));
                }
            });
        });
        ui.separator();
        close
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
                egui_extras::StripBuilder::new(ui)
                    .size(Size::remainder())
                    .size(Size::exact(300.0))
                    .horizontal(|mut strip| {
                        strip.cell(|ui| self.asset_lab_lineage_v4(ui, asset, &config, false));
                        strip.cell(|ui| self.asset_lab_lineage_details_v4(ui, asset, &config));
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
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.asset_lab.v4.dismissed_preview = self.asset_lab.v4.preview.take();
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

    fn asset_lab_create_v4(&mut self, ui: &mut Ui, asset: &Asset, config: &GenerativeConfig) {
        let provider = self
            .editor
            .provider_entries
            .iter()
            .find(|provider| Some(provider.id) == config.provider_id)
            .cloned();
        let media_fields: Vec<_> = provider
            .as_ref()
            .map(|provider| {
                provider
                    .inputs
                    .iter()
                    .filter(|field| {
                        crate::core::media_binding::bound_media_type_for_input(field).is_some()
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        StripBuilder::new(ui)
            .size(Size::remainder())
            .size(Size::exact(310.0))
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    let columns = if media_fields.len() == 4 {
                        2
                    } else {
                        media_fields.len().clamp(1, 3)
                    };
                    let rows = media_fields.len().div_ceil(columns);
                    let inputs_height = if rows == 0 {
                        0.0
                    } else {
                        rows as f32 * 70.0 + 8.0
                    };
                    let results_height = if self.asset_lab.v4.results.is_empty() {
                        0.0
                    } else {
                        126.0
                    };
                    StripBuilder::new(ui)
                        .size(Size::remainder().at_least(80.0))
                        .size(Size::exact(inputs_height))
                        .size(Size::exact(results_height))
                        .vertical(|mut strip| {
                            strip.cell(|ui| {
                                self.asset_lab_authoring_canvas(
                                    ui,
                                    asset,
                                    config,
                                    provider.as_ref(),
                                );
                            });
                            strip.cell(|ui| {
                                if let Some(provider) = &provider {
                                    let width = (ui.available_width() - 8.0 * (columns - 1) as f32)
                                        / columns as f32;
                                    for row in media_fields.chunks(columns) {
                                        kit::bounded_horizontal_row(ui, 64.0, |ui, _| {
                                            for field in row {
                                                ui.allocate_ui_with_layout(
                                                    Vec2::new(width, 64.0),
                                                    Layout::top_down(Align::Min),
                                                    |ui| {
                                                        self.media_source_picker_field(
                                                            ui, asset.id, None, provider, field,
                                                        )
                                                    },
                                                );
                                            }
                                        });
                                    }
                                }
                            });
                            strip.cell(|ui| self.asset_lab_results_v4(ui, asset));
                        });
                });
                strip.cell(|ui| {
                    self.asset_lab_create_inspector_v4(ui, asset, config, provider.as_ref())
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
                        .unwrap_or("a new image")
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
                    if let Some(seed_field) = provider
                        .inputs
                        .iter()
                        .find(|field| field.role == Some(InputRole::Seed))
                    {
                        ui.label(kit::caption(asset_lab_seed_preview(
                            true,
                            asset_lab_node_seed_value(&node, seed_field),
                            setup.batch.count,
                            setup.batch.seed_strategy,
                        )));
                    }
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
                            ui.checkbox(&mut setup.authoring.mask_enabled, "Use painted mask");
                            ui.checkbox(&mut self.asset_lab.v4.mask_visible, "Show mask overlay");
                            ui.label(kit::caption(
                                "Mask authoring only · generation support is not connected yet.",
                            ));
                        }
                        crate::state::AssetLabAuthoringProfile::Regions => {
                            ui.checkbox(&mut setup.authoring.regions_enabled, "Use prompt regions");
                            ui.checkbox(&mut self.asset_lab.v4.guide_visible, "Show guide image");
                            for region in &mut setup.authoring.regions {
                                kit::labeled_text_field(ui, "Region", &mut region.name);
                                kit::labeled_text_field(ui, "Description", &mut region.description);
                                if let Some(text) = region.text.as_mut() {
                                    kit::labeled_text_field(ui, "Text", text);
                                }
                            }
                            ui.label(kit::caption(
                                "Region authoring only · generation support is not connected yet.",
                            ));
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
                    egui::CollapsingHeader::new("Generation settings")
                        .id_salt("lab_v4_settings")
                        .show(ui, |ui| {
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
            kit::integer_step_drag(
                ui,
                &mut count,
                48.0,
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
        ui.label(kit::caption(
            "Results · hover to preview · click to continue",
        ));
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
                                    let tile = kit::source_tile(
                                        ui,
                                        "result",
                                        &version,
                                        texture,
                                        false,
                                        false,
                                        Vec2::new(90.0, 76.0),
                                    );
                                    if tile.hovered() || tile.has_focus() {
                                        preview = Some(version.clone());
                                    }
                                    if tile.clicked() {
                                        self.request_asset_lab_adopt(asset.id, &version);
                                    }
                                    let response =
                                        kit::field_button(ui, &format!("Compare {version}"), 90.0);
                                    if response.has_focus() || response.hovered() {
                                        preview = Some(version.clone());
                                    }
                                    if response.clicked() {
                                        self.enter_asset_lab_compare_v4(asset.id, &version);
                                    }
                                } else {
                                    kit::readonly_value_box(
                                        ui,
                                        format!("{:?}", result.status),
                                        Vec2::new(90.0, 76.0),
                                    );
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
        let body = (ui.available_height() - 90.0).max(80.0);
        let body_bottom = ui.cursor().min.y + body;
        egui::ScrollArea::vertical()
            .max_height(body)
            .min_scrolled_height(body)
            .show(ui, |ui| {
                ui.heading(&record.version);
                if config.active_version.as_ref() == Some(&record.version) {
                    ui.colored_label(kit::PRIMARY, "◆ Current output");
                }
                ui.add_space(18.0);
                kit::field_label(ui, "Recipe");
                ui.label(
                    self.editor
                        .provider_entries
                        .iter()
                        .find(|provider| provider.id == record.provider_id)
                        .map(|p| p.name.as_str())
                        .unwrap_or("Unavailable recipe"),
                );
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
                            kit::field_label(ui, label);
                            ui.label(text);
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
                    let title = source.source_version.as_deref().unwrap_or("Project source");
                    let detail = format!("{name} · resolved at submission");
                    kit::source_row(
                        ui,
                        ("submitted_source", &record.version, name),
                        title,
                        &detail,
                        thumbnail,
                        None,
                        false,
                        ui.available_width(),
                    );
                }
                ui.add_space(16.0);
                egui::CollapsingHeader::new("Settings")
                    .id_salt(("lineage_settings", &record.version))
                    .show(ui, |ui| {
                        for (label, value) in settings {
                            kit::field_label(ui, &label);
                            ui.label(value);
                        }
                    });
            });
        ui.add_space((body_bottom - ui.cursor().min.y).max(0.0));
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
                if kit::secondary_button(ui, "Compare with current", ui.available_width()).clicked()
                {
                    self.enter_asset_lab_compare_v4(asset.id, &record.version);
                }
            },
        );
        if config.active_version.is_none()
            && kit::secondary_button(ui, "Use this output", ui.available_width()).clicked()
        {
            self.pin_asset_lab_result_v4(asset.id, &record.version);
        }
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
        ui.label(kit::caption(format!("{} versions", config.versions.len())));
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
            Vec2::new(76.0, 52.0)
        } else {
            Vec2::new(136.0, 102.0)
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
            let factor = (1.0 + scroll * 0.015 * PREVIEW_WHEEL_ZOOM_MULTIPLIER).clamp(0.28, 1.88);
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
            let response = kit::source_tile(
                &mut child,
                ("lineage", compact, &record.version),
                &record.version,
                preview,
                self.asset_lab.selected_version.as_ref() == Some(&record.version),
                config.active_version.as_ref() == Some(&record.version),
                node_rect.size(),
            );
            if response.clicked() {
                self.asset_lab.selected_version = Some(record.version.clone());
                if compact {
                    self.enter_asset_lab_compare_v4(asset.id, &record.version);
                }
            }
            if response.double_clicked() {
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
    }

    fn asset_lab_compare_v4(&mut self, ui: &mut Ui, asset: &Asset, config: &GenerativeConfig) {
        self.poll_asset_lab_compare_timing_requests(ui.ctx());
        self.poll_asset_lab_compare_video_requests(ui.ctx());
        let Some(compare) = self.asset_lab.compare.clone() else {
            self.asset_lab.v4.view = AssetLabView::Lineage;
            return;
        };
        let total = ui.available_height();
        let map_height = self.asset_lab.v4.map_height.min((total * 0.5).max(100.0));
        StripBuilder::new(ui)
            .size(Size::remainder())
            .size(Size::exact(12.0))
            .size(Size::exact(map_height))
            .vertical(|mut strip| {
                strip.cell(|ui| {
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
                    let height = (ui.available_height()
                        - if asset.is_video() { 76.0 } else { 34.0 })
                    .max(80.0);
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
                    if asset.is_video() {
                        self.asset_lab_compare_transport(ui, max_duration);
                    } else {
                        kit::bounded_horizontal_row(ui, 28.0, |ui, _| {
                            ui.label(kit::caption(
                                "Linked views · wheel to zoom · right-drag to pan",
                            ));
                            if kit::field_button(ui, "Fit", 60.0).clicked() {
                                self.asset_lab.preview_auto_fit = true;
                            }
                        });
                    }
                });
                strip.cell(|ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(ui.available_size(), Sense::drag());
                    ui.painter().line_segment(
                        [
                            rect.center() - Vec2::new(16.0, 0.0),
                            rect.center() + Vec2::new(16.0, 0.0),
                        ],
                        Stroke::new(2.0_f32, kit::BORDER),
                    );
                    if response.dragged() {
                        self.asset_lab.v4.map_height = (self.asset_lab.v4.map_height
                            - response.drag_delta().y)
                            .clamp(100.0, total * 0.6);
                    }
                });
                strip.cell(|ui| self.asset_lab_lineage_v4(ui, asset, config, true));
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
    let mut rows: std::collections::BTreeMap<usize, Vec<Uuid>> = Default::default();
    for record in &config.versions {
        let Some(id) = record.lab_node_id else {
            continue;
        };
        let mut depth = 0;
        let mut current = id;
        let mut seen = std::collections::HashSet::new();
        while seen.insert(current) {
            let parent = completed_parent(config, current);
            let Some(parent) = parent else {
                break;
            };
            depth += 1;
            current = parent;
        }
        rows.entry(depth).or_default().push(id);
    }
    let width = rows.values().map(Vec::len).max().unwrap_or(1) as f32;
    rows.into_iter()
        .flat_map(|(depth, ids)| {
            let offset = (width - ids.len() as f32) * 0.5;
            ids.into_iter()
                .enumerate()
                .map(move |(index, id)| (id, index as f32 + offset, depth))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
