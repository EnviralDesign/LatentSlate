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

impl LatentSlateApp {
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
        let title = format!(
            "{}{}",
            field.label,
            if field.required { "" } else { " · optional" }
        );
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
        .on_hover_text(summary)
        .clicked()
        {
            self.open_source_picker(asset_id, context, provider, field);
        }
    }

    fn source_choice_preview(
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
        let escape =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut cancel_details = escape && state.details;
        close |= escape && !state.details;
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
                                        .clicked()
                                    {
                                        state.details = true;
                                    }
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
                                            if kit::secondary_button(ui, "Back", 80.0).clicked() {
                                                state.details = false;
                                            }
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if kit::primary_button(ui, "Apply", 120.0)
                                                        .clicked()
                                                    {
                                                        apply = true;
                                                    }
                                                    if kit::secondary_button(ui, "Cancel", 80.0)
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
            state.spec = lookup_media_binding(&config, &state.field, &self.editor.project);
            state.sizing = config
                .reference_sizing
                .get(&state.field.name)
                .copied()
                .unwrap_or_default();
            state.context = state.saved_context;
            state.details = false;
            state.error = None;
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
                (width - 88.0).max(1.0),
            )
            .on_hover_text(summary)
            .clicked()
            {
                state.spec = candidate.clone();
                *apply = true;
            }
            ui.add_enabled_ui(candidate.is_some(), |ui| {
                if kit::Tooltip::new("Configure source")
                    .description("Review sampling, timeline context, and sizing before applying this source.")
                    .apply(kit::icon_button_sized(ui, "Configure", Vec2::new(80.0, 64.0)))
                    .clicked()
                {
                    state.spec = candidate;
                    state.details = true;
                    state.error = None;
                }
            });
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
            .map(|spec| spec.sample.clone())
            .unwrap_or_else(|| default_sample_for_field(&state.field));
        let make = |source| {
            Some(MediaBindingSpec {
                source,
                sample: sample.clone(),
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
                &format!("↻ Working output · {working}"),
                "Follows the result you continue from in Create.",
                make(MediaBindingSource::WorkingOutput),
                apply,
            );
        }
        self.source_picker_choice(
            ui,
            state,
            config,
            "↗ Follow timeline · auto",
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
        kit::field_label(ui, "This asset · fixed version");
        ui.label(kit::caption(
            "These stay on the chosen version, even when it is also the working output.",
        ));
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
        kit::field_label(ui, "Project sources");
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
            kit::labeled_combo_field(
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
        }
        let Some(spec) = state.spec.as_mut() else {
            return;
        };
        if let MediaBindingSource::FollowTimeline { query } = &mut spec.source {
            kit::labeled_combo_field(
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
            ui.checkbox(&mut query.prefer_touching, "Prefer touching clips");
        }
        if !matches!(spec.source, MediaBindingSource::FrozenArtifact { .. }) {
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
            kit::labeled_combo_field(
                ui,
                "Sample",
                "source_sample",
                normalize_sample(&spec.sample, &state.field).label(),
                |ui| {
                    for sample in options {
                        if ui
                            .selectable_label(
                                sample_matches_option(
                                    &normalize_sample(&spec.sample, &state.field),
                                    &sample,
                                ),
                                sample.label(),
                            )
                            .clicked()
                        {
                            spec.sample = sample;
                            ui.close();
                        }
                    }
                },
            );
            match &mut spec.sample {
                MediaSample::Frame {
                    at:
                        MediaFramePoint::OutputOffset { seconds }
                        | MediaFramePoint::SourceTime { seconds },
                } => source_time_field(ui, "Seconds", seconds),
                MediaSample::SourceRange {
                    start_seconds,
                    duration_seconds,
                } => {
                    source_time_field(ui, "Start seconds", start_seconds);
                    source_time_field(ui, "Duration seconds", duration_seconds);
                }
                _ => {}
            }
        }
        if state.field.image_dimensions
            == Some(crate::state::ImageDimensionsRequirement::MatchOutputCanvas)
        {
            kit::labeled_combo_field(
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
                        if ui
                            .selectable_label(state.sizing == sizing, sizing.label())
                            .clicked()
                        {
                            state.sizing = sizing;
                            ui.close();
                        }
                    }
                },
            );
            ui.label(kit::caption(
                "Padding is black; cropping is centered. Original media is preserved.",
            ));
        }
        ui.add_space(10.0);
        ui.label(kit::caption(summary));
        ui.label(kit::caption(
            "Timeline inputs use source media and timing, not the composited timeline.",
        ));
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
        if matches!(spec.source, MediaBindingSource::FrozenArtifact { .. }) {
            if kit::secondary_button(ui, "Restore original source binding", ui.available_width())
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
                .clicked()
                {
                    match lock_source_spec(&plan, spec) {
                        Ok(locked) => *spec = locked,
                        Err(err) => state.error = Some(err.message(&state.field.label)),
                    }
                }
                if kit::secondary_button(ui, "Capture current input", ui.available_width())
                    .clicked()
                {
                    *capture = true;
                }
            });
            ui.label(kit::caption(
                "Capture retains the exact current frame or range even if its source changes.",
            ));
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

fn source_time_field(ui: &mut Ui, label: &str, value: &mut f64) {
    kit::field_label(ui, label);
    let rect = crate::egui_app::inspector_numeric_rect(ui, ui.available_width());
    crate::egui_app::inspector_numeric_field(ui, rect, |ui, width| {
        ui.add_sized(
            [width, kit::FIELD_H],
            egui::DragValue::new(value)
                .speed(0.01)
                .range(0.0..=f64::MAX),
        )
    });
}
