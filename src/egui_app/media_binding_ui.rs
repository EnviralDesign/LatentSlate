use eframe::egui::{self, RichText, Ui};
use uuid::Uuid;

use crate::core::media_binding::{
    bound_media_type_for_input, default_sample_for_field, format_timecode, freeze_binding,
    generation_context_placements, lock_source_spec, lookup_media_binding, normalize_sample,
    resolve_generation_context, resolve_media_binding, resolved_now_summary, return_to_follow_spec,
    sample_matches_option, sample_options_for_field, source_compatible_with_field,
    source_menu_label, unfreeze_spec, MediaResolveContext,
};
use crate::core::timeline_bridge::provider_is_timeline_bridge;
use crate::state::{
    asset_display_name, ClipImageMode, GenerativeConfig, InputRole, MediaBindingSource,
    MediaBindingSpec, MediaBindingStability, MediaCoveragePolicy, MediaFramePoint, MediaSample,
    ProviderEntry, ProviderInputField, ProviderInputType, TimelineSourceQuery, TimelineTrackScope,
};
use crate::ui_kit as kit;

use super::{automation_selectable_value, LatentSlateApp};

impl LatentSlateApp {
    pub(super) fn media_binding_field(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        provider: &ProviderEntry,
        input: &ProviderInputField,
    ) {
        if provider_is_timeline_bridge(provider)
            && matches!(
                input.role,
                Some(InputRole::LeftVideo | InputRole::RightVideo)
            )
        {
            if let Some(update) =
                self.provider_asset_input_field(ui, asset_id, context_clip_id, input)
            {
                self.editor
                    .project
                    .update_generative_config(asset_id, |config| {
                        config.inputs.insert(input.name.clone(), update);
                    });
                let _ = self.editor.project.save_generative_config(asset_id);
            }
            return;
        }

        let Some(media_type) = bound_media_type_for_input(input) else {
            return;
        };
        let config = self
            .editor
            .project
            .generative_config(asset_id)
            .cloned()
            .unwrap_or_default();
        let spec = lookup_media_binding(&config, input, &self.editor.project);
        let context = resolve_generation_context(
            &self.editor.project,
            asset_id,
            context_clip_id,
            self.generation_context_by_asset.get(&asset_id).copied(),
        )
        .ok()
        .flatten()
        .or(context_clip_id);
        let plan = spec.as_ref().map(|spec| {
            resolve_media_binding(
                MediaResolveContext {
                    project: &self.editor.project,
                    target_asset_id: Some(asset_id),
                    context_clip_id: context,
                    field: input,
                    provider: Some(provider),
                    config: Some(&config),
                },
                spec,
            )
        });

        let source_label = spec
            .as_ref()
            .map(|spec| source_menu_label(spec, &self.editor.project))
            .unwrap_or_else(|| "None".to_string());
        let mut next_spec = spec.clone();
        let source_heading = if input.required {
            format!("{} *", input.label)
        } else {
            input.label.clone()
        };
        let sample_spec = spec.clone();
        ui.columns(if sample_spec.is_some() { 2 } else { 1 }, |columns| {
            kit::labeled_combo_field(
                &mut columns[0],
                &source_heading,
                ("media_source", &input.name),
                source_label.clone(),
                |ui| {
                    if !input.required
                        && automation_selectable_value(ui, &mut next_spec, None, "None").clicked()
                    {
                        ui.close();
                    }
                    ui.label(kit::caption("Follow Timeline"));
                    follow_choice(ui, &mut next_spec, TimelineTrackScope::Auto, "Auto");
                    follow_choice(
                        ui,
                        &mut next_spec,
                        TimelineTrackScope::SameTrack,
                        "Same Track",
                    );
                    follow_choice(
                        ui,
                        &mut next_spec,
                        TimelineTrackScope::Below,
                        "Tracks Below",
                    );
                    ui.separator();
                    ui.label(kit::caption("Specific Track"));
                    let tracks: Vec<_> = self.editor.project.tracks.clone();
                    for track in tracks {
                        follow_choice(
                            ui,
                            &mut next_spec,
                            TimelineTrackScope::SpecificTrack { track_id: track.id },
                            &track.name,
                        );
                    }
                    ui.separator();
                    ui.label(kit::caption("Timeline Clip"));
                    let clips: Vec<_> = self.editor.project.clips.clone();
                    for clip in clips {
                        let Some(asset) = self.editor.project.find_asset(clip.asset_id) else {
                            continue;
                        };
                        if !source_compatible_with_field(asset, media_type) {
                            continue;
                        }
                        let track = self
                            .editor
                            .project
                            .find_track(clip.track_id)
                            .map(|track| track.name.as_str())
                            .unwrap_or("Track");
                        let mode = if clip.image_mode == ClipImageMode::Keyframe && asset.is_image()
                        {
                            " · keyframe"
                        } else {
                            ""
                        };
                        let label = format!(
                            "{} · {} {}–{}{mode}",
                            asset_display_name(asset),
                            track,
                            format_timecode(clip.start_time),
                            format_timecode(clip.end_time())
                        );
                        let version = asset.active_version().map(str::to_string);
                        let candidate = Some(MediaBindingSpec {
                            source: MediaBindingSource::TimelineClip {
                                clip_id: clip.id,
                                version,
                            },
                            sample: spec
                                .as_ref()
                                .map(|spec| spec.sample.clone())
                                .unwrap_or_else(|| default_sample_for_field(input)),
                            coverage: MediaCoveragePolicy::Strict,
                        });
                        if automation_selectable_value(ui, &mut next_spec, candidate, &label)
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.label(kit::caption("Project Asset"));
                    let assets: Vec<_> = self.editor.project.assets.clone();
                    for asset in assets {
                        if !source_compatible_with_field(&asset, media_type) {
                            continue;
                        }
                        let candidate = Some(MediaBindingSpec {
                            source: MediaBindingSource::ProjectAsset {
                                asset_id: asset.id,
                                version: None,
                            },
                            sample: spec
                                .as_ref()
                                .map(|spec| spec.sample.clone())
                                .unwrap_or_else(|| default_sample_for_field(input)),
                            coverage: MediaCoveragePolicy::Strict,
                        });
                        if automation_selectable_value(
                            ui,
                            &mut next_spec,
                            candidate,
                            &asset_display_name(&asset),
                        )
                        .clicked()
                        {
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.label(kit::caption("Generated Version"));
                    for asset in self.editor.project.assets.clone() {
                        if !asset.is_generative()
                            || !source_compatible_with_field(&asset, media_type)
                        {
                            continue;
                        }
                        let Some(config) = self.editor.project.generative_config(asset.id).cloned()
                        else {
                            continue;
                        };
                        for record in config.versions {
                            let label = format!("{} ({})", asset.name, record.version);
                            let candidate = Some(MediaBindingSpec {
                                source: MediaBindingSource::ProjectAsset {
                                    asset_id: asset.id,
                                    version: Some(record.version),
                                },
                                sample: spec
                                    .as_ref()
                                    .map(|spec| spec.sample.clone())
                                    .unwrap_or_else(|| default_sample_for_field(input)),
                                coverage: MediaCoveragePolicy::Strict,
                            });
                            if automation_selectable_value(ui, &mut next_spec, candidate, &label)
                                .clicked()
                            {
                                ui.close();
                            }
                        }
                    }
                    if let Some(frozen) = spec.clone().filter(|spec| {
                        matches!(spec.source, MediaBindingSource::FrozenArtifact { .. })
                    }) {
                        ui.separator();
                        ui.label(kit::caption("Frozen Input"));
                        if automation_selectable_value(
                            ui,
                            &mut next_spec,
                            Some(frozen),
                            "Current frozen file",
                        )
                        .clicked()
                        {
                            ui.close();
                        }
                    }
                },
            );
            if let Some(spec) = sample_spec.as_ref() {
                let normalized = normalize_sample(&spec.sample, input);
                let options = sample_options_for_field(input);
                let sample_label = normalized.label();
                let mut next_sample = spec.sample.clone();
                kit::labeled_combo_field(
                    &mut columns[1],
                    "Sample",
                    ("media_sample", asset_id, &input.name),
                    sample_label,
                    |ui| {
                        for option in options {
                            let selected = sample_matches_option(&normalized, &option);
                            if ui.selectable_label(selected, option.label()).clicked() {
                                next_sample = option;
                                ui.close();
                            }
                        }
                        if matches!(input.input_type, ProviderInputType::Image) {
                            if ui
                                .selectable_label(
                                    matches!(
                                        normalized,
                                        MediaSample::Frame {
                                            at: MediaFramePoint::OutputOffset { .. }
                                        }
                                    ),
                                    "Output-relative time…",
                                )
                                .clicked()
                            {
                                next_sample = MediaSample::Frame {
                                    at: MediaFramePoint::OutputOffset { seconds: 0.0 },
                                };
                                ui.close();
                            }
                            if ui
                                .selectable_label(
                                    matches!(
                                        normalized,
                                        MediaSample::Frame {
                                            at: MediaFramePoint::SourceTime { .. }
                                        }
                                    ),
                                    "Explicit source time…",
                                )
                                .clicked()
                            {
                                next_sample = MediaSample::Frame {
                                    at: MediaFramePoint::SourceTime { seconds: 0.0 },
                                };
                                ui.close();
                            }
                        }
                        if matches!(
                            input.input_type,
                            ProviderInputType::Video | ProviderInputType::Audio
                        ) {
                            if ui
                                .selectable_label(
                                    matches!(normalized, MediaSample::SourceRange { .. }),
                                    "Explicit source range…",
                                )
                                .clicked()
                            {
                                next_sample = MediaSample::SourceRange {
                                    start_seconds: 0.0,
                                    duration_seconds: 1.0,
                                };
                                ui.close();
                            }
                        }
                    },
                );
                if next_sample != spec.sample {
                    let mut updated = spec.clone();
                    updated.sample = next_sample;
                    self.commit_media_binding(asset_id, &input.name, Some(updated));
                }
            }
        });

        if next_spec != spec {
            self.commit_media_binding(asset_id, &input.name, next_spec);
        }

        let spec = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(|config| lookup_media_binding(config, input, &self.editor.project));
        if let Some(spec) = spec.as_ref() {
            if let MediaSample::Frame {
                at: MediaFramePoint::OutputOffset { seconds },
            } = spec.sample
            {
                let mut value = seconds;
                ui.horizontal(|ui| {
                    ui.label(kit::caption("Offset (s)"));
                    if ui
                        .add(
                            egui::DragValue::new(&mut value)
                                .speed(0.05)
                                .range(0.0..=120.0),
                        )
                        .changed()
                    {
                        let mut updated = spec.clone();
                        updated.sample = MediaSample::Frame {
                            at: MediaFramePoint::OutputOffset { seconds: value },
                        };
                        self.commit_media_binding(asset_id, &input.name, Some(updated));
                    }
                });
            }
            if let MediaSample::Frame {
                at: MediaFramePoint::SourceTime { seconds },
            } = spec.sample
            {
                let mut value = seconds;
                ui.horizontal(|ui| {
                    ui.label(kit::caption("Source time (s)"));
                    if ui
                        .add(
                            egui::DragValue::new(&mut value)
                                .speed(0.05)
                                .range(0.0..=600.0),
                        )
                        .changed()
                    {
                        let mut updated = spec.clone();
                        updated.sample = MediaSample::Frame {
                            at: MediaFramePoint::SourceTime { seconds: value },
                        };
                        self.commit_media_binding(asset_id, &input.name, Some(updated));
                    }
                });
            }

            if let MediaSample::SourceRange {
                start_seconds,
                duration_seconds,
            } = spec.sample
            {
                let mut start = start_seconds;
                let mut duration = duration_seconds;
                ui.horizontal(|ui| {
                    ui.label(kit::caption("Start (s)"));
                    if ui
                        .add(
                            egui::DragValue::new(&mut start)
                                .speed(0.05)
                                .range(0.0..=600.0),
                        )
                        .changed()
                    {
                        let mut updated = spec.clone();
                        updated.sample = MediaSample::SourceRange {
                            start_seconds: start,
                            duration_seconds,
                        };
                        self.commit_media_binding(asset_id, &input.name, Some(updated));
                    }
                    ui.label(kit::caption("Duration (s)"));
                    if ui
                        .add(
                            egui::DragValue::new(&mut duration)
                                .speed(0.05)
                                .range(0.05..=600.0),
                        )
                        .changed()
                    {
                        let mut updated = spec.clone();
                        updated.sample = MediaSample::SourceRange {
                            start_seconds,
                            duration_seconds: duration,
                        };
                        self.commit_media_binding(asset_id, &input.name, Some(updated));
                    }
                });
            }

            ui.add_space(kit::FORM_ROW_GAP);
            let stability = spec.stability();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = kit::FORM_ROW_GAP;
                ui.label(
                    RichText::new(stability.label())
                        .color(match stability {
                            MediaBindingStability::Follow => kit::IMAGE,
                            MediaBindingStability::LockSource => kit::PRIMARY,
                            MediaBindingStability::FreezeInput => kit::AUDIO,
                        })
                        .size(11.5),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = kit::FORM_ROW_GAP;
                    let plan_ok = plan.as_ref().is_some_and(|plan| plan.is_ok());
                    if matches!(stability, MediaBindingStability::FreezeInput) {
                        if kit::field_button(ui, "Unfreeze", 90.0).clicked() {
                            match unfreeze_spec(spec) {
                                Ok(restored) => {
                                    self.commit_media_binding(
                                        asset_id,
                                        &input.name,
                                        Some(restored),
                                    );
                                }
                                Err(err) => self.editor.status = err.message(&input.label),
                            }
                        }
                    } else if plan_ok && kit::field_button(ui, "Freeze Input", 110.0).clicked() {
                        self.freeze_media_binding(asset_id, input, spec, context, provider);
                    }
                    if matches!(stability, MediaBindingStability::Follow) {
                        if kit::field_button(ui, "Lock Source", 110.0).clicked() {
                            if let Some(plan) = plan.as_ref() {
                                if let Ok(locked) = lock_source_spec(plan, spec) {
                                    self.commit_media_binding(asset_id, &input.name, Some(locked));
                                } else {
                                    self.editor.status =
                                        "Lock Source needs a valid current resolution.".to_string();
                                }
                            }
                        }
                    } else if matches!(stability, MediaBindingStability::LockSource) {
                        if kit::field_button(ui, "Return to Follow", 130.0).clicked() {
                            self.commit_media_binding(
                                asset_id,
                                &input.name,
                                Some(return_to_follow_spec(spec)),
                            );
                        }
                    }
                });
            });
            if matches!(stability, MediaBindingStability::LockSource)
                && plan.as_ref().is_some_and(|plan| !plan.is_ok())
            {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Locked source is missing. Choose a new source.")
                        .color(kit::DANGER)
                        .size(11.0),
                );
            }
        }

        ui.add_space(4.0);
        let summary = match &plan {
            Some(plan) => resolved_now_summary(&self.editor.project, plan),
            None if input.required => format!("Unresolved\n{}: choose a source.", input.label),
            None => "No media source configured.".to_string(),
        };
        let color = if plan.as_ref().is_some_and(|plan| plan.is_ok()) {
            kit::TEXT_MUTED
        } else if spec.is_some() || input.required {
            kit::DANGER
        } else {
            kit::TEXT_DIM
        };
        let response =
            ui.add(egui::Label::new(RichText::new(summary).color(color).size(11.0)).wrap());
        if let Some(plan) = plan.as_ref() {
            let hover = crate::core::media_binding::binding_hover_text(&self.editor.project, plan);
            if !hover.is_empty() {
                response.on_hover_text(hover);
            }
        }
        ui.add_space(kit::FORM_ROW_GAP);
    }

    pub(super) fn media_binding_context_picker(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        selected_clip_id: Option<Uuid>,
    ) {
        let placements = generation_context_placements(&self.editor.project, asset_id);
        if placements.len() < 2 {
            if placements.len() == 1 {
                ui.label(kit::caption(format!(
                    "Generation context: the only timeline placement ({})",
                    format_timecode(
                        self.editor
                            .project
                            .clips
                            .iter()
                            .find(|clip| clip.id == placements[0])
                            .map(|clip| clip.start_time)
                            .unwrap_or(0.0)
                    )
                )));
            }
            return;
        }
        let current = resolve_generation_context(
            &self.editor.project,
            asset_id,
            selected_clip_id,
            self.generation_context_by_asset.get(&asset_id).copied(),
        )
        .ok()
        .flatten();
        let label = current
            .and_then(|clip_id| {
                self.editor
                    .project
                    .clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                    .map(|clip| format!("Placement at {}", format_timecode(clip.start_time)))
            })
            .unwrap_or_else(|| "Select a timeline placement…".to_string());
        kit::labeled_combo_field(
            ui,
            "Generation context",
            ("generation_context", asset_id),
            label,
            |ui| {
                for clip_id in placements {
                    let Some(clip) = self
                        .editor
                        .project
                        .clips
                        .iter()
                        .find(|clip| clip.id == clip_id)
                    else {
                        continue;
                    };
                    let track = self
                        .editor
                        .project
                        .find_track(clip.track_id)
                        .map(|track| track.name.as_str())
                        .unwrap_or("Track");
                    let text = format!(
                        "{track} · {}–{}",
                        format_timecode(clip.start_time),
                        format_timecode(clip.end_time())
                    );
                    if ui
                        .selectable_label(current == Some(clip_id), text)
                        .clicked()
                    {
                        self.generation_context_by_asset.insert(asset_id, clip_id);
                        ui.close();
                    }
                }
            },
        );
        ui.add_space(kit::FORM_ROW_GAP);
    }

    pub(super) fn media_binding_bulk_actions(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        provider: &ProviderEntry,
        config: &GenerativeConfig,
    ) {
        let media_fields: Vec<_> = provider
            .inputs
            .iter()
            .filter(|input| bound_media_type_for_input(input).is_some())
            .cloned()
            .collect();
        if media_fields.len() < 2 {
            return;
        }
        ui.horizontal(|ui| {
            if kit::field_button(ui, "Lock all resolved", 140.0).clicked() {
                self.bulk_lock_media_bindings(asset_id, context_clip_id, provider, config);
            }
            if kit::field_button(ui, "Follow all", 100.0).clicked() {
                for input in &media_fields {
                    if let Some(spec) = lookup_media_binding(config, input, &self.editor.project) {
                        if !matches!(spec.source, MediaBindingSource::FollowTimeline { .. }) {
                            self.commit_media_binding(
                                asset_id,
                                &input.name,
                                Some(return_to_follow_spec(&spec)),
                            );
                        }
                    }
                }
            }
        });
        ui.add_space(kit::FORM_ROW_GAP);
    }

    fn bulk_lock_media_bindings(
        &mut self,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        provider: &ProviderEntry,
        config: &GenerativeConfig,
    ) {
        let context = resolve_generation_context(
            &self.editor.project,
            asset_id,
            context_clip_id,
            self.generation_context_by_asset.get(&asset_id).copied(),
        )
        .ok()
        .flatten()
        .or(context_clip_id);
        let mut locked = 0usize;
        let mut skipped = 0usize;
        for input in provider.inputs.iter() {
            let Some(spec) = lookup_media_binding(config, input, &self.editor.project) else {
                continue;
            };
            if !matches!(spec.source, MediaBindingSource::FollowTimeline { .. }) {
                continue;
            }
            let plan = resolve_media_binding(
                MediaResolveContext {
                    project: &self.editor.project,
                    target_asset_id: Some(asset_id),
                    context_clip_id: context,
                    field: input,
                    provider: Some(provider),
                    config: Some(config),
                },
                &spec,
            );
            match lock_source_spec(&plan, &spec) {
                Ok(locked_spec) => {
                    self.commit_media_binding(asset_id, &input.name, Some(locked_spec));
                    locked += 1;
                }
                Err(_) => skipped += 1,
            }
        }
        self.editor.status = format!("Locked {locked} source(s); {skipped} could not be locked.");
    }

    fn freeze_media_binding(
        &mut self,
        asset_id: Uuid,
        input: &ProviderInputField,
        spec: &MediaBindingSpec,
        context_clip_id: Option<Uuid>,
        provider: &ProviderEntry,
    ) {
        let Some(config) = self.editor.project.generative_config(asset_id).cloned() else {
            return;
        };
        let plan = resolve_media_binding(
            MediaResolveContext {
                project: &self.editor.project,
                target_asset_id: Some(asset_id),
                context_clip_id,
                field: input,
                provider: Some(provider),
                config: Some(&config),
            },
            spec,
        );
        let Some(asset) = self.editor.project.find_asset(asset_id) else {
            return;
        };
        let Some(folder) = generative_folder_rel(asset) else {
            self.editor.status = "Generative folder is unavailable.".to_string();
            return;
        };
        let Some(root) = self.editor.project.project_path.clone() else {
            self.editor.status = "Project folder is unavailable.".to_string();
            return;
        };
        let folder_abs = root.join(folder);
        match freeze_binding(&self.editor.project, &folder_abs, &input.name, spec, &plan) {
            Ok(frozen) => {
                self.commit_media_binding(asset_id, &input.name, Some(frozen));
                self.editor.status = format!("Froze {} input.", input.label);
            }
            Err(err) => self.editor.status = err.message(&input.label),
        }
    }

    fn commit_media_binding(
        &mut self,
        asset_id: Uuid,
        field: &str,
        spec: Option<MediaBindingSpec>,
    ) {
        self.editor
            .project
            .update_generative_config(asset_id, |config| match spec {
                Some(spec) => {
                    config.media_bindings.insert(field.to_string(), spec);
                }
                None => {
                    config.media_bindings.remove(field);
                }
            });
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Failed to save media binding: {err}");
        }
    }
}

fn follow_choice(
    ui: &mut Ui,
    current: &mut Option<MediaBindingSpec>,
    scope: TimelineTrackScope,
    label: &str,
) {
    let candidate = Some(MediaBindingSpec {
        source: MediaBindingSource::FollowTimeline {
            query: TimelineSourceQuery {
                scope,
                prefer_touching: true,
            },
        },
        sample: current
            .as_ref()
            .map(|spec| spec.sample.clone())
            .unwrap_or(MediaSample::Auto),
        coverage: MediaCoveragePolicy::Strict,
    });
    if automation_selectable_value(ui, current, candidate, label).clicked() {
        ui.close();
    }
}

fn generative_folder_rel(asset: &crate::state::Asset) -> Option<std::path::PathBuf> {
    match &asset.kind {
        crate::state::AssetKind::GenerativeVideo { folder, .. }
        | crate::state::AssetKind::GenerativeImage { folder, .. }
        | crate::state::AssetKind::GenerativeAudio { folder, .. } => Some(folder.clone()),
        _ => None,
    }
}

pub(super) fn seed_locked_clip_binding(
    config: &mut GenerativeConfig,
    provider: Option<&ProviderEntry>,
    field_hint: &str,
    clip_id: Uuid,
    asset_id: Uuid,
    version: Option<String>,
    sample: MediaSample,
) {
    let mut names = Vec::new();
    if let Some(provider) = provider {
        for input in provider.inputs.iter() {
            if bound_media_type_for_input(input).is_none() {
                continue;
            }
            let matches_hint = input.name == field_hint
                || (matches!(input.role, Some(InputRole::StartImage))
                    && matches!(field_hint, "start_image" | "image"))
                || (matches!(input.role, Some(InputRole::EndImage)) && field_hint == "end_image");
            if matches_hint {
                names.push(input.name.clone());
            }
        }
    }
    if names.is_empty() {
        names.push(field_hint.to_string());
    }
    names.sort();
    names.dedup();
    let spec = MediaBindingSpec {
        source: MediaBindingSource::TimelineClip { clip_id, version },
        sample,
        coverage: MediaCoveragePolicy::Strict,
    };
    let _ = asset_id;
    for name in names {
        config.media_bindings.insert(name, spec.clone());
    }
}

pub(super) fn sample_from_frame_reference(
    frame_reference: Option<crate::state::SourceFrameReference>,
    end_field: bool,
) -> MediaSample {
    match frame_reference {
        Some(crate::state::SourceFrameReference::Last) => MediaSample::Frame {
            at: MediaFramePoint::SourceEnd,
        },
        Some(crate::state::SourceFrameReference::First) => MediaSample::Frame {
            at: MediaFramePoint::SourceStart,
        },
        None if end_field => MediaSample::Frame {
            at: MediaFramePoint::OutputEnd,
        },
        None => MediaSample::Frame {
            at: MediaFramePoint::OutputStart,
        },
    }
}
