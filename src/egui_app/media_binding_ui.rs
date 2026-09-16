mod picker;
pub(super) use picker::SourcePickerState;

use eframe::egui::{self, Ui};
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
    asset_display_name, GenerativeConfig, InputRole, MediaBindingSource, MediaBindingSpec,
    MediaCoveragePolicy, MediaFramePoint, MediaSample, ProviderEntry, ProviderInputField,
    ProviderInputType, TimelineTrackScope,
};
use crate::ui_kit as kit;

use super::LatentSlateApp;

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
        self.media_source_picker_field(ui, asset_id, context_clip_id, provider, input);
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
        ui.horizontal_wrapped(|ui| {
            if kit::field_button(ui, "Lock all resolved", 140.0).clicked() {
                self.bulk_lock_media_bindings(asset_id, context_clip_id, provider, config);
            }
            if kit::field_button(ui, "Follow timeline for all", 100.0).clicked() {
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

    fn commit_media_binding(
        &mut self,
        asset_id: Uuid,
        field: &str,
        spec: Option<MediaBindingSpec>,
    ) {
        let input = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(|config| {
                self.editor
                    .provider_entries
                    .iter()
                    .find(|provider| Some(provider.id) == config.provider_id)
            })
            .and_then(|provider| provider.inputs.iter().find(|input| input.name == field))
            .cloned();
        if let Some(input) = input {
            if let Err(error) = self
                .editor
                .set_generation_source(asset_id, &input, spec, None)
            {
                self.editor.status = error;
            }
        }
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
