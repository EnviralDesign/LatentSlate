use super::*;
use crate::state::{Asset, ReferenceSizing};
use eframe::egui::{Context, TextureId, Vec2};

#[derive(Clone)]
pub(in crate::egui_app) struct SourcePickerState {
    asset_id: Uuid,
    provider: ProviderEntry,
    field: ProviderInputField,
    spec: Option<MediaBindingSpec>,
    sizing: ReferenceSizing,
    context: Option<Uuid>,
    saved_context: Option<Uuid>,
    project_revision: u64,
    details: bool,
    error: Option<String>,
}

impl SourcePickerState {
    fn cancel_details(&mut self, config: &GenerativeConfig, project: &crate::state::Project) {
        self.spec = lookup_media_binding(config, &self.field, project);
        self.sizing = config
            .reference_sizing
            .get(&self.field.name)
            .copied()
            .unwrap_or_default();
        self.context = self.saved_context;
        self.details = false;
        self.error = None;
    }
}

impl LatentSlateApp {
    pub(in crate::egui_app) fn dismiss_source_picker_on_escape(&mut self) {
        if let Some(mut state) = self.source_picker.take() {
            if state.details {
                if let Some(config) = self.editor.project.generative_config(state.asset_id) {
                    state.cancel_details(config, &self.editor.project);
                    self.source_picker = Some(state);
                }
            }
        }
    }

    pub(in crate::egui_app) fn open_source_picker(
        &mut self,
        asset_id: Uuid,
        context: Option<Uuid>,
        provider: &ProviderEntry,
        field: &ProviderInputField,
    ) {
        let Some(config) = self.editor.project.generative_config(asset_id) else {
            return;
        };
        let context = resolve_generation_context(
            &self.editor.project,
            asset_id,
            context,
            self.generation_context_by_asset.get(&asset_id).copied(),
        )
        .ok()
        .flatten();
        self.source_picker = Some(SourcePickerState {
            asset_id,
            provider: provider.clone(),
            field: field.clone(),
            spec: lookup_media_binding(config, field, &self.editor.project),
            sizing: config
                .reference_sizing
                .get(&field.name)
                .copied()
                .unwrap_or_default(),
            context,
            saved_context: context,
            project_revision: self.editor.project_session_revision,
            details: false,
            error: None,
        });
    }

    pub(in crate::egui_app) fn media_source_picker_field(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context: Option<Uuid>,
        provider: &ProviderEntry,
        field: &ProviderInputField,
    ) {
        self.media_source_picker_field_sized(ui, asset_id, context, provider, field, false);
    }

    pub(in crate::egui_app) fn media_source_picker_field_sized(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context: Option<Uuid>,
        provider: &ProviderEntry,
        field: &ProviderInputField,
        compact: bool,
    ) {
        if field.paired_video_input.is_some() {
            return;
        }
        let Some(config) = self.editor.project.generative_config(asset_id).cloned() else {
            return;
        };
        let spec = lookup_media_binding(&config, field, &self.editor.project);
        let resolved_context = resolve_generation_context(
            &self.editor.project,
            asset_id,
            context,
            self.generation_context_by_asset.get(&asset_id).copied(),
        )
        .ok()
        .flatten();
        let (preview, summary) = self.source_choice_preview(
            ui,
            asset_id,
            provider,
            field,
            resolved_context,
            &config,
            spec.as_ref(),
        );
        let token = crate::core::media_binding::effective_reference_token(
            provider,
            &config,
            &self.editor.project,
            field,
        );
        let title = match token {
            Some(token) => format!("@{} · {token}", field.label),
            None => format!(
                "{}{}",
                field.label,
                if field.required { "" } else { " · optional" }
            ),
        };
        let mut label = spec
            .as_ref()
            .map(|spec| source_menu_label(spec, &self.editor.project))
            .unwrap_or_else(|| "None".into());
        if spec
            .as_ref()
            .is_some_and(|spec| matches!(spec.source, MediaBindingSource::WorkingOutput))
        {
            if let Some(version) = &config.lab_authoring.working_version {
                label.push_str(&format!(" · {version}"));
            }
        }
        let width = ui.available_width();
        if kit::source_field(
            ui,
            ("source_field", asset_id, &field.name),
            &title,
            &label,
            preview,
            source_badge(spec.as_ref()),
            compact,
            width,
        )
        .on_hover_text(match field.description.as_deref() {
            Some(help) => format!("{summary}\n\n{help}"),
            None => summary,
        })
        .clicked()
        {
            self.open_source_picker(asset_id, context, provider, field);
        }
        if field.input_type == ProviderInputType::Audio {
            self.audio_source_status(
                ui,
                asset_id,
                resolved_context,
                provider,
                &config,
                field,
                spec.as_ref(),
                "Audio file or video soundtrack",
            );
        }
        if let Some(soundtrack) = provider
            .inputs
            .iter()
            .find(|input| input.paired_video_input.as_deref() == Some(field.name.as_str()))
        {
            let soundtrack_spec = lookup_media_binding(&config, soundtrack, &self.editor.project);
            let mut enabled = soundtrack_spec.is_some();
            let token = crate::core::media_binding::effective_reference_token(
                provider,
                &config,
                &self.editor.project,
                soundtrack,
            );
            let label = token
                .map(|token| format!("Soundtrack {token}"))
                .unwrap_or_else(|| "Soundtrack".into());
            ui.push_id(("soundtrack", asset_id, &soundtrack.name), |ui| {
                kit::bounded_horizontal_row(ui, kit::SECONDARY_BUTTON_H, |ui, row_width| {
                    ui.add_enabled_ui(spec.is_some() || enabled, |ui| {
                        if kit::Tooltip::new("Paired soundtrack")
                            .description("Use audio together with this video as one paired reference. By default it follows this video's exact source, trim and retiming. Choose Source to use separately configured audio instead; its timing is left as selected.")
                            .apply(kit::checkbox(ui, &mut enabled, &label)).changed() {
                            let binding = enabled.then(|| MediaBindingSpec {
                                source: MediaBindingSource::PairedVideoInput { field: field.name.clone() },
                                sample: MediaSample::Whole, coverage: MediaCoveragePolicy::Strict,
                            });
                            if let Err(error) = self.editor.set_generation_source(asset_id, soundtrack, binding, None) { self.editor.status = error; }
                        }
                    });
                    if enabled {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if kit::secondary_button(ui, "Source…", row_width.min(72.0))
                                .on_hover_text("Choose a separate soundtrack source, configure its sample, or return to this video's own soundtrack.").clicked() {
                                self.open_source_picker(asset_id, context, provider, soundtrack);
                            }
                        });
                    }
                });
                let source = match soundtrack_spec.as_ref().map(|spec| &spec.source) {
                    Some(MediaBindingSource::PairedVideoInput { .. }) => "This video's soundtrack".into(),
                    Some(_) => format!("Separate · {}", source_menu_label(soundtrack_spec.as_ref().unwrap(), &self.editor.project)),
                    None => "Video only".into(),
                };
                self.audio_source_status(ui, asset_id, resolved_context, provider, &config, soundtrack, soundtrack_spec.as_ref(), &source);
            });
        }
    }

    fn audio_source_status(
        &self,
        ui: &mut Ui,
        asset_id: Uuid,
        context: Option<Uuid>,
        provider: &ProviderEntry,
        config: &GenerativeConfig,
        field: &ProviderInputField,
        spec: Option<&MediaBindingSpec>,
        fallback: &str,
    ) {
        let mut text = fallback.to_string();
        let mut color = kit::TEXT_MUTED;
        if let Some(spec) = spec {
            let plan = resolve_media_binding(
                MediaResolveContext {
                    project: &self.editor.project,
                    target_asset_id: Some(asset_id),
                    context_clip_id: context,
                    field,
                    provider: Some(provider),
                    config: Some(config),
                },
                spec,
            );
            if let Some(error) = plan.error_messages().first() {
                text = error.clone();
                color = kit::DANGER;
            } else if let Some(status) =
                crate::core::media_binding::audio_reference_inspection(&plan)
            {
                use crate::core::media_binding::AudioInspection;
                match &status {
                    AudioInspection::Checking | AudioInspection::Analyzing => {
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(200));
                        text = status.label().into();
                    }
                    AudioInspection::MissingTrack => {
                        text = status.label().into();
                        color = kit::DANGER;
                    }
                    AudioInspection::Invalid(error) => {
                        text = error.clone();
                        color = kit::DANGER;
                    }
                    AudioInspection::Quiet
                    | AudioInspection::LevelUnavailable
                    | AudioInspection::MultipleTracks => {
                        text = status.label().into();
                        color = kit::OperationSeverity::Warning.color();
                    }
                    AudioInspection::Ready => {
                        if field.paired_video_input.is_none() {
                            text = if plan.source_media_type
                                == Some(crate::state::BoundMediaType::Video)
                            {
                                "Using video's audio track"
                            } else {
                                "Audio ready"
                            }
                            .into();
                        }
                    }
                }
            }
        }
        ui.add_sized([ui.available_width(), 20.0], egui::Label::new(kit::caption(&text).color(color)).truncate())
            .on_hover_text(format!("{text}\n\nAudio is read from source media, not the timeline mix. Level checks cover the selected interval before retiming; very quiet audio is advisory and does not block generation."));
    }

    pub(in crate::egui_app) fn source_choice_preview(
        &mut self,
        ui: &Ui,
        asset_id: Uuid,
        provider: &ProviderEntry,
        field: &ProviderInputField,
        context: Option<Uuid>,
        config: &GenerativeConfig,
        spec: Option<&MediaBindingSpec>,
    ) -> (Option<(TextureId, Vec2)>, String) {
        let Some(spec) = spec else {
            return (None, "No media in this slot.".into());
        };
        let plan = resolve_media_binding(
            MediaResolveContext {
                project: &self.editor.project,
                target_asset_id: Some(asset_id),
                context_clip_id: context,
                field,
                provider: Some(provider),
                config: Some(config),
            },
            spec,
        );
        let summary = resolved_now_summary(&self.editor.project, &plan);
        if !plan.is_ok()
            || !ui.is_rect_visible(egui::Rect::from_min_size(
                ui.cursor().min,
                Vec2::new(80.0, 64.0),
            ))
        {
            return (None, summary);
        }
        let Some(path) = plan.source_path_absolute.as_ref() else {
            return (None, summary);
        };
        let mut asset = plan
            .source_asset_id
            .and_then(|id| self.editor.project.find_asset(id))
            .cloned();
        if let MediaBindingSource::FrozenArtifact { media_type, .. } = &spec.source {
            // Preview the captured bytes even if their original source was removed or changed.
            let mut captured = match media_type {
                crate::state::BoundMediaType::Image => {
                    Asset::new_image("Captured input", path.clone())
                }
                crate::state::BoundMediaType::Video => {
                    Asset::new_video("Captured input", path.clone())
                }
                crate::state::BoundMediaType::Audio => {
                    Asset::new_audio("Captured input", path.clone())
                }
            };
            captured.id = asset_id;
            asset = Some(captured);
        }
        let preview = asset.as_ref().and_then(|asset| {
            self.asset_lab_path_preview_texture(
                ui.ctx(),
                asset,
                plan.source_version.as_deref(),
                if matches!(spec.source, MediaBindingSource::FrozenArtifact { .. }) {
                    0.0
                } else {
                    plan.source_frame_time.unwrap_or(0.0)
                },
                path,
            )
        });
        let summary = if preview.is_none() && plan.media_type != crate::state::BoundMediaType::Audio
        {
            format!("{summary}\nPreview unavailable")
        } else {
            summary
        };
        (preview, summary)
    }

    pub(in crate::egui_app) fn source_picker_modal(&mut self, ctx: &Context) {
        let Some(mut state) = self.source_picker.take() else {
            return;
        };
        if state.project_revision != self.editor.project_session_revision {
            return;
        }
        let Some(config) = self
            .editor
            .project
            .generative_config(state.asset_id)
            .cloned()
        else {
            return;
        };
        let mut close = kit::dismissible_nested_modal_scrim(ctx, "source_picker", true);
        let mut cancel_details = false;
        let mut apply = false;
        let mut capture = false;
        let size = crate::egui_app::modal_size(ctx, [590.0, 820.0], [380.0, 300.0]);
        egui::Window::new("Source configuration")
            .id(egui::Id::new("source_picker_modal"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size(size)
            .frame(kit::modal_frame())
            .show(ctx, |ui| {
                close |= kit::modal_header_with_close(
                    ui,
                    &format!("{} source", state.field.label),
                    None,
                    true,
                );
                let details = state.details;
                ui.spacing_mut().item_spacing.y = 0.0;
                egui_extras::StripBuilder::new(ui)
                    .size(egui_extras::Size::remainder())
                    .size(egui_extras::Size::exact(if details {
                        kit::PRIMARY_BUTTON_H + 32.0
                    } else {
                        0.0
                    }))
                    .vertical(|mut strip| {
                        strip.cell(|ui| {
                            kit::modal_scroll_body(ui, ("source_picker_scroll", details), |ui| {
                                ui.spacing_mut().item_spacing = Vec2::splat(8.0);
                                if let Some(error) = &state.error {
                                    ui.colored_label(kit::DANGER, error);
                                }
                                if state.details {
                                    self.source_picker_details(
                                        ui,
                                        &mut state,
                                        &config,
                                        &mut capture,
                                    );
                                } else {
                                    if state.spec.is_some()
                                        && kit::secondary_button(
                                            ui,
                                            "Configure selected input",
                                            ui.available_width(),
                                        )
                                        .on_hover_text("Adjust sampling, timeline context, and sizing for this input. Changes remain local until Apply or a successful capture.")
                                        .clicked()
                                    {
                                        state.details = true;
                                    }
                                    if state.field.input_type == ProviderInputType::Audio { ui.label(kit::caption("Audio file or video soundtrack")); }
                                    self.source_picker_choices(ui, &mut state, &config, &mut apply);
                                }
                            });
                        });
                        strip.cell(|ui| {
                            if details {
                                let rect = ui.max_rect();
                                kit::modal_body(ui, |ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    kit::bounded_horizontal_row(
                                        ui,
                                        kit::PRIMARY_BUTTON_H,
                                        |ui, _| {
                                            if kit::secondary_button(ui, "Back", 80.0)
                                                .on_hover_text("Return to source choices, keeping these uncommitted settings while the picker stays open.").clicked() {
                                                state.details = false;
                                            }
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if kit::primary_button(ui, "Apply", 120.0)
                                                        .on_hover_text("Save this source and its settings, then return to source choices.")
                                                        .clicked()
                                                    {
                                                        apply = true;
                                                    }
                                                    if kit::secondary_button(ui, "Cancel", 80.0)
                                                        .on_hover_text("Discard these uncommitted settings and return to source choices.")
                                                        .clicked()
                                                    {
                                                        cancel_details = true;
                                                    }
                                                },
                                            );
                                        },
                                    );
                                });
                                kit::paint_panel_edge(ui, rect, kit::PanelEdge::Top);
                            }
                        });
                    });
            });
        if cancel_details {
            state.cancel_details(&config, &self.editor.project);
        }
        if capture {
            let result = (|| {
                let spec = state
                    .spec
                    .as_ref()
                    .ok_or("Choose a source before capturing it.")?;
                let plan = resolve_media_binding(
                    MediaResolveContext {
                        project: &self.editor.project,
                        target_asset_id: Some(state.asset_id),
                        context_clip_id: state.context,
                        field: &state.field,
                        provider: Some(&state.provider),
                        config: Some(&config),
                    },
                    spec,
                );
                let asset = self
                    .editor
                    .project
                    .find_asset(state.asset_id)
                    .ok_or("Asset unavailable")?;
                let folder = generative_folder_rel(asset).ok_or("Asset folder unavailable")?;
                let root = self
                    .editor
                    .project
                    .project_path
                    .as_ref()
                    .ok_or("Save the project first")?;
                freeze_binding(
                    &self.editor.project,
                    &root.join(folder),
                    &state.field.name,
                    spec,
                    &plan,
                )
                .map_err(|err| err.message(&state.field.label))
            })();
            match result {
                Ok(spec) => {
                    state.spec = Some(spec);
                    apply = true;
                }
                Err(error) => state.error = Some(error),
            }
        }
        if apply {
            match self.editor.set_generation_source(
                state.asset_id,
                &state.field,
                state.spec.clone(),
                Some(state.sizing),
            ) {
                Ok(()) => {
                    if let Some(context) = state.context {
                        self.generation_context_by_asset
                            .insert(state.asset_id, context);
                    }
                    if state.details {
                        state.details = false;
                        state.saved_context = state.context;
                        state.error = None;
                    } else {
                        close = true;
                    }
                }
                Err(error) => state.error = Some(error),
            }
        }
        if !close {
            self.source_picker = Some(state);
        }
    }

    fn source_picker_choice(
        &mut self,
        ui: &mut Ui,
        state: &mut SourcePickerState,
        config: &GenerativeConfig,
        title: &str,
        help: &str,
        candidate: Option<MediaBindingSpec>,
        apply: &mut bool,
    ) {
        let (preview, summary) = self.source_choice_preview(
            ui,
            state.asset_id,
            &state.provider,
            &state.field,
            state.context,
            config,
            candidate.as_ref(),
        );
        let width = ui.available_width();
        let id = (title, serde_json::to_string(&candidate).unwrap_or_default());
        let selected = state.spec == candidate;
        kit::bounded_horizontal_row(ui, 64.0, |ui, _| {
            if kit::source_row(
                ui,
                &id,
                title,
                help,
                preview,
                source_badge(candidate.as_ref()),
                selected,
                if candidate.is_some() {
                    (width - 88.0).max(1.0)
                } else {
                    width
                },
            )
            .on_hover_text(summary)
            .clicked()
            {
                state.spec = candidate.clone();
                *apply = true;
            }
            if candidate.is_some() {
                if kit::Tooltip::new("Configure source")
                    .description("Review sampling, timeline context, and sizing before applying this source.")
                    .apply(kit::icon_button_sized(ui, "Configure", Vec2::new(80.0, 64.0)))
                    .clicked()
                {
                    state.spec = candidate;
                    state.details = true;
                    state.error = None;
                }
            }
        });
    }

    fn source_picker_choices(
        &mut self,
        ui: &mut Ui,
        state: &mut SourcePickerState,
        config: &GenerativeConfig,
        apply: &mut bool,
    ) {
        let sample = state
            .spec
            .as_ref()
            .filter(|spec| !matches!(spec.source, MediaBindingSource::PairedVideoInput { .. }))
            .map(|spec| spec.sample.clone());
        let reference_workflow = state.provider.resolved_workflow_kind()
            == crate::state::ProviderWorkflowKind::ReferenceToVideo;
        let timeline_sample = default_sample_for_field(&state.field);
        let make = |source| {
            let default_sample = if reference_workflow
                && !matches!(
                    source,
                    MediaBindingSource::FollowTimeline { .. }
                        | MediaBindingSource::TimelineClip { .. }
                ) {
                MediaSample::Whole
            } else {
                timeline_sample.clone()
            };
            Some(MediaBindingSpec {
                source,
                sample: sample.clone().unwrap_or(default_sample),
                coverage: MediaCoveragePolicy::Strict,
            })
        };
        self.source_picker_choice(
            ui,
            state,
            config,
            "None",
            "No media in this slot.",
            None,
            apply,
        );
        if let Some(video) = state.field.paired_video_input.clone() {
            self.source_picker_choice(
                ui,
                state,
                config,
                "This video's soundtrack",
                "Paired with the video input; follows its source, trim and retiming.",
                Some(MediaBindingSpec {
                    source: MediaBindingSource::PairedVideoInput { field: video },
                    sample: MediaSample::Whole,
                    coverage: MediaCoveragePolicy::Strict,
                }),
                apply,
            );
        }
        let working = config
            .lab_authoring
            .working_version
            .as_deref()
            .unwrap_or("unavailable");
        if self
            .editor
            .project
            .find_asset(state.asset_id)
            .is_some_and(|asset| {
                source_compatible_with_field(
                    asset,
                    bound_media_type_for_input(&state.field)
                        .unwrap_or(crate::state::BoundMediaType::Image),
                )
            })
        {
            self.source_picker_choice(
                ui,
                state,
                config,
                &format!("Working output · {working}"),
                "Follows the result you continue from in Create.",
                make(MediaBindingSource::WorkingOutput),
                apply,
            );
        }
        self.source_picker_choice(
            ui,
            state,
            config,
            "Follow timeline · auto",
            "Resolves from the timeline when submitted.",
            make(MediaBindingSource::follow_auto()),
            apply,
        );
        if state
            .spec
            .as_ref()
            .is_some_and(|spec| matches!(spec.source, MediaBindingSource::FrozenArtifact { .. }))
        {
            self.source_picker_choice(
                ui,
                state,
                config,
                "Captured input",
                "Exact media, retained independently of the source.",
                state.spec.clone(),
                apply,
            );
        }
        ui.add_space(14.0);
        kit::Tooltip::new("Fixed versions")
            .description("These stay on the chosen version, even when it is also the working output. The pin marks this asset’s current output.")
            .apply(ui.label(kit::body("This asset · fixed versions").strong()));
        let count = ((ui.available_width() + 8.0) / 122.0).floor().max(1.0) as usize;
        let width = ((ui.available_width() - (count - 1) as f32 * 8.0) / count as f32).max(1.0);
        if let Some(asset) = self.editor.project.find_asset(state.asset_id).cloned() {
            for row in config.versions.chunks(count) {
                kit::bounded_horizontal_row(ui, width + 28.0, |ui, _| {
                    for record in row {
                        let candidate = make(MediaBindingSource::ProjectAsset {
                            asset_id: asset.id,
                            version: Some(record.version.clone()),
                        });
                        let preview = if ui.is_rect_visible(egui::Rect::from_min_size(
                            ui.cursor().min,
                            Vec2::splat(width),
                        )) {
                            self.asset_lab_node_preview_texture(
                                ui.ctx(),
                                &asset,
                                Some(&record.version),
                                0.0,
                            )
                        } else {
                            None
                        };
                        if kit::source_tile(
                            ui,
                            ("source_version", &record.version),
                            &record.version,
                            preview,
                            state.spec == candidate,
                            config.active_version.as_ref() == Some(&record.version),
                            Vec2::new(width, width + 28.0),
                        )
                        .clicked()
                        {
                            state.spec = candidate;
                            *apply = true;
                        }
                    }
                });
            }
        }
        ui.add_space(14.0);
        kit::Tooltip::new("Project sources")
            .description("Choose project media or another asset’s current output. Expand a generated asset to choose one fixed version instead.")
            .apply(ui.label(kit::body("Project sources").strong()));
        let media_type =
            bound_media_type_for_input(&state.field).unwrap_or(crate::state::BoundMediaType::Image);
        for asset in self.editor.project.assets.clone() {
            if asset.id == state.asset_id || !source_compatible_with_field(&asset, media_type) {
                continue;
            }
            self.source_picker_choice(
                ui,
                state,
                config,
                &asset_display_name(&asset),
                if asset.is_generative() {
                    "Follows this asset’s current output."
                } else {
                    "Project media"
                },
                make(MediaBindingSource::ProjectAsset {
                    asset_id: asset.id,
                    version: None,
                }),
                apply,
            );
            if let Some(versions) = self
                .editor
                .project
                .generative_config(asset.id)
                .map(|config| config.versions.clone())
            {
                egui::CollapsingHeader::new(format!("{} · fixed versions", asset.name))
                    .id_salt(("source_asset_versions", asset.id))
                    .show(ui, |ui| {
                        for record in versions {
                            self.source_picker_choice(
                                ui,
                                state,
                                config,
                                &format!("{} · {}", asset.name, record.version),
                                "Fixed version",
                                make(MediaBindingSource::ProjectAsset {
                                    asset_id: asset.id,
                                    version: Some(record.version),
                                }),
                                apply,
                            );
                        }
                    });
            }
        }
        ui.add_space(14.0);
        kit::field_label(ui, "Timeline clips");
        for clip in self.editor.project.clips.clone() {
            let Some(asset) = self.editor.project.find_asset(clip.asset_id).cloned() else {
                continue;
            };
            if asset.id == state.asset_id || !source_compatible_with_field(&asset, media_type) {
                continue;
            }
            let label = format!(
                "{} · {}–{}",
                asset.name,
                format_timecode(clip.start_time),
                format_timecode(clip.end_time())
            );
            self.source_picker_choice(
                ui,
                state,
                config,
                &label,
                "Uses this clip; sampling follows its timing.",
                make(MediaBindingSource::TimelineClip {
                    clip_id: clip.id,
                    version: asset.active_version().map(str::to_string),
                }),
                apply,
            );
        }
    }

    fn source_picker_details(
        &mut self,
        ui: &mut Ui,
        state: &mut SourcePickerState,
        config: &GenerativeConfig,
        capture: &mut bool,
    ) {
        let (preview, summary) = self.source_choice_preview(
            ui,
            state.asset_id,
            &state.provider,
            &state.field,
            state.context,
            config,
            state.spec.as_ref(),
        );
        let title = state
            .spec
            .as_ref()
            .map(|spec| source_menu_label(spec, &self.editor.project))
            .unwrap_or_else(|| "None".into());
        kit::source_row(
            ui,
            "source_details_preview",
            &title,
            &state.field.label,
            preview,
            source_badge(state.spec.as_ref()),
            true,
            ui.available_width(),
        );
        ui.add_space(10.0);
        let placements = generation_context_placements(&self.editor.project, state.asset_id);
        if placements.len() > 1 {
            let response = kit::labeled_combo_field(
                ui,
                "Target timeline placement",
                "source_context",
                state
                    .context
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "Choose placement".into()),
                |ui| {
                    for id in &placements {
                        if let Some(clip) =
                            self.editor.project.clips.iter().find(|clip| clip.id == *id)
                        {
                            let label = format!(
                                "{}–{}",
                                format_timecode(clip.start_time),
                                format_timecode(clip.end_time())
                            );
                            if ui
                                .selectable_label(state.context == Some(*id), label)
                                .clicked()
                            {
                                state.context = Some(*id);
                                ui.close();
                            }
                        }
                    }
                },
            );
            kit::Tooltip::new("Target timeline placement")
                .description("Choose which placement of this generated asset provides its output timing and track context. This matters when the asset appears more than once on the timeline.")
                .apply(response);
        }
        let Some(spec) = state.spec.as_mut() else {
            return;
        };
        if let MediaBindingSource::FollowTimeline { query } = &mut spec.source {
            let response = kit::labeled_combo_field(
                ui,
                "Timeline scope",
                "source_scope",
                query.scope.label(),
                |ui| {
                    for scope in [
                        TimelineTrackScope::Auto,
                        TimelineTrackScope::SameTrack,
                        TimelineTrackScope::Below,
                    ] {
                        if ui
                            .selectable_label(query.scope == scope, scope.label())
                            .clicked()
                        {
                            query.scope = scope;
                            ui.close();
                        }
                    }
                    for track in &self.editor.project.tracks {
                        let scope = TimelineTrackScope::SpecificTrack { track_id: track.id };
                        if ui
                            .selectable_label(query.scope == scope, &track.name)
                            .clicked()
                        {
                            query.scope = scope;
                            ui.close();
                        }
                    }
                },
            );
            kit::Tooltip::new("Timeline scope")
                .description("Choose which tracks may supply this input relative to the target placement. Auto considers all eligible tracks; the other choices restrict the search. The resolved source below shows the actual choice. Inputs use raw source media and clip timing, not the composited timeline.")
                .apply(response);
            kit::Tooltip::new("Prefer touching clips")
                .description("Prefer an eligible clip immediately before or after the target over one covering the requested time. Turn this off to prefer covering clips. Exact keyframes keep priority.")
                .apply(ui.checkbox(&mut query.prefer_touching, "Prefer touching clips"));
        }
        if !matches!(
            spec.source,
            MediaBindingSource::FrozenArtifact { .. } | MediaBindingSource::PairedVideoInput { .. }
        ) {
            let mut options = sample_options_for_field(&state.field);
            if state.field.input_type == ProviderInputType::Image {
                options.push(MediaSample::Whole);
                options.push(MediaSample::Frame {
                    at: MediaFramePoint::OutputOffset { seconds: 0.0 },
                });
                options.push(MediaSample::Frame {
                    at: MediaFramePoint::SourceTime { seconds: 0.0 },
                });
            }
            let response = kit::labeled_combo_field(
                ui,
                "Sample",
                "source_sample",
                normalize_sample(&spec.sample, &state.field).label(),
                |ui| {
                    for sample in options {
                        if kit::combo_option(
                            ui,
                            sample_matches_option(
                                &normalize_sample(&spec.sample, &state.field),
                                &sample,
                            ),
                            &sample.label(),
                        )
                        .clicked()
                        {
                            spec.sample = sample;
                            ui.close();
                        }
                    }
                },
            );
            let help = if state.field.input_type == ProviderInputType::Image {
                "Output means this generated asset's timeline placement. Output first/last or +seconds samples the input clip at that output time, using its trim and timing.\n\nSource means the chosen input. Source first/last uses its trimmed clip boundaries, or the file boundaries for a project asset. Source seconds is an absolute time in the media file.\n\nWhole source requests the full input instead of a specific frame. For a particular video frame, choose a frame sample."
            } else {
                "Aligned range uses the input clip over the generated asset's timeline interval, applying clip timing. The clip must cover the full interval.\n\nSource range uses a start time and duration in the input file. Whole source uses the selected clip's visible span, or the full file for a project asset.\n\nThese inputs use raw source media, not the composited timeline."
            };
            kit::Tooltip::new("Sample")
                .description(help)
                .apply(response);
            match &mut spec.sample {
                MediaSample::Frame {
                    at: MediaFramePoint::OutputOffset { seconds },
                } => source_time_field(ui, "Output offset (seconds)", seconds,
                    "Seconds after the start of this generated asset's timeline placement. The resolver finds the matching time in the chosen input clip."),
                MediaSample::Frame {
                    at: MediaFramePoint::SourceTime { seconds },
                } => source_time_field(ui, "Source time (seconds)", seconds,
                    "Absolute time from the beginning of the input media file. For a timeline clip, the time must fall within its visible source span."),
                MediaSample::SourceRange {
                    start_seconds,
                    duration_seconds,
                } => {
                    source_time_field(ui, "Start (source seconds)", start_seconds,
                        "Start time measured from the beginning of the input media file, not from the output placement.");
                    source_time_field(ui, "Duration (seconds)", duration_seconds,
                        "Length of the source segment to use. The entire range must fit within the selected source or clip's visible span.");
                }
                _ => {}
            }
        }
        if state.field.image_dimensions
            == Some(crate::state::ImageDimensionsRequirement::MatchOutputCanvas)
        {
            let response = kit::labeled_combo_field(
                ui,
                "Reference sizing",
                "source_sizing",
                state.sizing.label(),
                |ui| {
                    for sizing in [
                        ReferenceSizing::Exact,
                        ReferenceSizing::FitInside,
                        ReferenceSizing::Fill,
                        ReferenceSizing::Stretch,
                    ] {
                        if kit::combo_option(ui, state.sizing == sizing, sizing.label()).clicked() {
                            state.sizing = sizing;
                            ui.close();
                        }
                    }
                },
            );
            kit::Tooltip::new("Reference sizing")
                .description("Match the recipe's output canvas. Exact requires matching dimensions. Fit inside preserves proportions and pads with black; Fill preserves proportions and crops centrally; Stretch changes proportions. Original media is preserved.")
                .apply(response);
        }
        ui.add_space(10.0);
        let (heading, detail) = summary.split_once('\n').unwrap_or((&summary, ""));
        kit::card_frame().show(ui, |ui| {
            ui.label(kit::section_label(heading));
            if !detail.is_empty() {
                ui.add(egui::Label::new(kit::body(detail)).wrap().selectable(true));
            }
        });
        let plan = resolve_media_binding(
            MediaResolveContext {
                project: &self.editor.project,
                target_asset_id: Some(state.asset_id),
                context_clip_id: state.context,
                field: &state.field,
                provider: Some(&state.provider),
                config: Some(config),
            },
            spec,
        );
        if state.field.input_type == ProviderInputType::Audio {
            self.audio_source_status(
                ui,
                state.asset_id,
                state.context,
                &state.provider,
                config,
                &state.field,
                Some(spec),
                "Audio file or video soundtrack",
            );
        }
        if matches!(spec.source, MediaBindingSource::FrozenArtifact { .. }) {
            if kit::secondary_button(ui, "Restore original source binding", ui.available_width())
                .on_hover_text("Restore the source selection and sampling saved before capture. This is a draft change until you Apply it.")
                .clicked()
            {
                match unfreeze_spec(spec) {
                    Ok(restored) => *spec = restored,
                    Err(err) => state.error = Some(err.message(&state.field.label)),
                }
            }
        } else {
            ui.add_enabled_ui(plan.is_ok(), |ui| {
                if matches!(
                    spec.source,
                    MediaBindingSource::FollowTimeline { .. } | MediaBindingSource::WorkingOutput
                ) && kit::secondary_button(
                    ui,
                    "Use currently resolved source",
                    ui.available_width(),
                )
                .on_hover_text("Replace this following source with the specific clip or asset version resolved now. Sampling still uses that source; it does not copy the sampled media. Apply to save the change.")
                .clicked()
                {
                    match lock_source_spec(&plan, spec) {
                        Ok(locked) => *spec = locked,
                        Err(err) => state.error = Some(err.message(&state.field.label)),
                    }
                }
                if kit::Tooltip::new("Capture current input")
                    .description("Save an immutable project-local copy of the currently resolved frame or range. It stays the same if the original source changes. Capture commits only after the copy succeeds, then returns to source choices.")
                    .apply(kit::secondary_button(ui, "Capture current input", ui.available_width()))
                    .clicked()
                {
                    *capture = true;
                }
            });
        }
    }
}

fn source_badge(spec: Option<&MediaBindingSpec>) -> Option<&'static str> {
    match &spec?.source {
        MediaBindingSource::WorkingOutput => Some("↻"),
        MediaBindingSource::FollowTimeline { .. } => Some("↗"),
        _ => None,
    }
}

fn source_time_field(ui: &mut Ui, label: &str, value: &mut f64, help: &str) {
    kit::field_label(ui, label);
    let rect = crate::egui_app::inspector_numeric_rect(ui, ui.available_width());
    crate::egui_app::inspector_numeric_field(ui, rect, |ui, width| {
        let response = ui.add_sized(
            [width, kit::FIELD_H],
            egui::DragValue::new(value)
                .speed(0.01)
                .range(0.0..=f64::MAX),
        );
        kit::Tooltip::new(label).description(help).apply(response)
    });
}
