use super::*;

impl LatentSlateApp {
    pub(super) fn right_panel(&mut self, root: &mut Ui) {
        if self.editor.layout.right_collapsed {
            let response = egui::Panel::right(self.project_panel_id("attributes_collapsed"))
                .exact_size(kit::COLLAPSED_RAIL_W)
                .frame(kit::collapsed_dock_frame())
                .show_inside(root, |ui| {
                    if kit::collapsed_rail_button(ui, "◀").clicked() {
                        self.editor.layout.right_collapsed = false;
                    }
                });
            kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Left);
            return;
        }

        let response = egui::Panel::right(self.project_panel_id("attributes"))
            .resizable(true)
            .default_size(self.editor.layout.right_width)
            .size_range(200.0..=440.0)
            .frame(kit::dock_frame())
            .show_inside(root, |ui| {
                kit::fixed_panel_body(ui, |ui| self.attributes_panel(ui));
            });
        self.editor.layout.right_width = response.response.rect.width().clamp(200.0, 440.0);
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Left);
    }

    pub(super) fn attributes_panel(&mut self, ui: &mut Ui) {
        kit::panel_header(ui, "ATTRIBUTES", Some("▶"), || {
            self.editor.layout.right_collapsed = true;
        });
        ui.add_space(8.0);

        kit::scroll_body(ui, |ui| {
            ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
            if self.editor.selection.clip_ids.len() > 1 {
                self.multi_clip_attributes(ui);
            } else if let Some(clip_id) = self.editor.selected_clip_id() {
                self.clip_attributes(ui, clip_id);
            } else if self.editor.selection.asset_ids.len() > 1 {
                self.multi_asset_attributes(ui);
            } else if let Some(asset_id) = self.editor.selected_asset_id() {
                self.asset_attributes(ui, asset_id);
            } else if let Some(marker_id) = self.editor.selected_marker_id() {
                self.marker_attributes(ui, marker_id);
            } else if let Some(track_id) = self.editor.selected_track_id() {
                self.track_attributes(ui, track_id);
            } else {
                kit::sunken_frame().show(ui, |ui| {
                    kit::empty_state(
                        ui,
                        "Nothing selected",
                        "Select a clip, asset, marker, or track.",
                    );
                });
            }
        });
    }

    pub(super) fn multi_clip_attributes(&mut self, ui: &mut Ui) {
        let selected_ids = self.editor.selection.clip_ids.clone();
        let mut clips: Vec<Clip> = self
            .editor
            .project
            .clips
            .iter()
            .filter(|clip| selected_ids.contains(&clip.id))
            .cloned()
            .collect();
        clips.sort_by(|a, b| {
            a.start_time
                .partial_cmp(&b.start_time)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });

        inspector_card(ui, "Selection", |ui| {
            ui.label(kit::value(format!("{} clips selected", clips.len())));
            if clips.len() >= 2 {
                let first = clips.first().map(|clip| clip.start_time).unwrap_or(0.0);
                let last = clips.last().map(|clip| clip.start_time).unwrap_or(first);
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(format!(
                    "Start span {} - {}",
                    timecode(first),
                    timecode(last)
                )));
            }
        });

        ui.add_space(kit::FORM_ROW_GAP);
        let mut apply_spacing = false;
        let mut bridge_provider_id: Option<Option<Uuid>> = None;
        inspector_card(ui, "Spacing", |ui| {
            let fps = self.editor.project.settings.fps.max(1.0);
            let mut seconds = self.clip_spacing_seconds.max(0.0);
            let mut frames = self.clip_spacing_frames.max(1);
            let (left_rect, right_rect) = inspector_numeric_pair_rects(ui);
            if inspector_drag_f64_in_rect(ui, left_rect, "Seconds", &mut seconds, 0.05) {
                self.clip_spacing_seconds = seconds.max(0.0);
                self.clip_spacing_frames = frames_from_seconds(self.clip_spacing_seconds, fps)
                    .round()
                    .max(1.0) as i64;
            }
            if inspector_drag_i64_in_rect(ui, right_rect, "Frames", &mut frames, 1.0) {
                self.clip_spacing_frames = frames.max(1);
                self.clip_spacing_seconds =
                    seconds_from_frames(self.clip_spacing_frames as f64, fps);
            }
            ui.add_space(kit::FORM_ROW_GAP);
            automation_checkbox(
                ui,
                &mut self.clip_spacing_set_duration,
                "Set clip duration to interval",
            );
            ui.add_space(kit::ACTION_GAP);
            if kit::primary_button(ui, "Space Selected Clips", ui.available_width()).clicked() {
                apply_spacing = true;
            }
        });

        let visual_reference_clip_ids: Vec<Uuid> = clips
            .iter()
            .filter_map(|clip| {
                self.editor
                    .project
                    .find_asset(clip.asset_id)
                    .filter(|asset| asset.is_image() || asset.is_video())
                    .map(|_| clip.id)
            })
            .collect();
        if visual_reference_clip_ids.len() >= 2 {
            ui.add_space(kit::FORM_ROW_GAP);
            inspector_card(ui, "Generation", |ui| {
                ui.label(kit::caption(
                    "Create a new generative video asset using selected image keyframes or video boundaries as pinned references.",
                ));
                ui.add_space(kit::ACTION_GAP);
                ui.menu_button("Generate Between Keyframes", |ui| {
                    if let Some(provider_id) = self.provider_choice_menu(
                        ui,
                        ProviderWorkflowKind::FirstFrameLastFrameVideo,
                        "Configure provider later",
                    ) {
                        bridge_provider_id = Some(provider_id);
                        ui.close();
                    }
                });
            });
        }

        if apply_spacing && clips.len() >= 2 {
            self.space_selected_clips(&clips);
        }
        if let Some(provider_id) = bridge_provider_id {
            self.request_bridge_video_from_selected_clips(&clips, provider_id);
        }
    }

    pub(super) fn multi_asset_attributes(&mut self, ui: &mut Ui) {
        let count = self.editor.selection.asset_ids.len();
        inspector_card(ui, "Selection", |ui| {
            ui.label(kit::value(format!("{count} assets selected")));
            ui.add_space(kit::FORM_ROW_GAP);
            ui.label(kit::caption(
                "Timeline and generation actions are available after placing assets as clips.",
            ));
        });
    }

    pub(super) fn space_selected_clips(&mut self, clips: &[Clip]) {
        let interval = self.clip_spacing_seconds.max(0.0);
        if clips.len() < 2 || interval <= 0.0 {
            self.editor.status =
                "Select at least two clips and use a positive interval.".to_string();
            return;
        }

        let mut previous_anchor = None;
        for clip in clips.iter() {
            let start_time = previous_anchor
                .map(|anchor| anchor + interval)
                .unwrap_or(clip.start_time);
            let duration = if self.clip_spacing_set_duration {
                interval.max(0.1)
            } else {
                clip.duration
            };
            if self.clip_spacing_set_duration {
                self.editor
                    .project
                    .resize_clip(clip.id, start_time, duration);
            } else {
                self.editor.project.move_clip(clip.id, start_time);
            }
            previous_anchor = if self.clip_spacing_uses_point_anchor(clip.asset_id) {
                Some(start_time)
            } else {
                Some(start_time + duration)
            };
        }
        self.editor.preview_dirty = true;
        self.editor.status = format!(
            "Spaced {} clips by {}",
            clips.len(),
            format_duration(interval)
        );
    }

    pub(super) fn clip_spacing_uses_point_anchor(&self, asset_id: Uuid) -> bool {
        self.editor
            .project
            .find_asset(asset_id)
            .is_some_and(|asset| asset.is_image())
    }

    pub(super) fn provider_choices_for_kind(
        &self,
        kind: ProviderWorkflowKind,
    ) -> Vec<ProviderEntry> {
        self.editor
            .provider_entries
            .iter()
            .filter(|provider| provider.resolved_workflow_kind() == kind)
            .cloned()
            .collect()
    }

    pub(super) fn provider_choice_menu(
        &self,
        ui: &mut Ui,
        kind: ProviderWorkflowKind,
        configure_later_label: &str,
    ) -> Option<Option<Uuid>> {
        let providers = self.provider_choices_for_kind(kind);
        if automation_button(ui.button(configure_later_label), configure_later_label).clicked() {
            return Some(None);
        }
        if providers.is_empty() {
            ui.separator();
            ui.label(kit::caption(format!(
                "No {} providers configured.",
                kind.label()
            )));
            return None;
        }
        ui.separator();
        for provider in providers {
            if provider_choice_menu_row(ui, &provider).clicked() {
                return Some(Some(provider.id));
            }
        }
        None
    }

    pub(super) fn request_bridge_video_from_selected_clips(
        &mut self,
        clips: &[Clip],
        provider_id: Option<Uuid>,
    ) {
        let reference_clips = self.bridge_reference_clips(clips);
        let (Some(first), Some(last)) = (reference_clips.first(), reference_clips.last()) else {
            self.editor.status =
                "Select at least two image or video reference clips first.".to_string();
            return;
        };
        if first.clip.id == last.clip.id {
            self.editor.status = "Select two different reference clips.".to_string();
            return;
        }

        let convert_clip_ids: Vec<Uuid> = [first, last]
            .iter()
            .filter(|reference| {
                self.editor
                    .project
                    .find_asset(reference.clip.asset_id)
                    .is_some_and(|asset| {
                        asset.is_image() && !clip_is_keyframe_image(&reference.clip, Some(asset))
                    })
            })
            .map(|reference| reference.clip.id)
            .collect();
        if convert_clip_ids.is_empty() {
            self.create_bridge_video_from_selected_clips(clips, provider_id);
            return;
        }

        let sample_names = [first, last]
            .iter()
            .filter_map(|reference| {
                self.editor
                    .project
                    .find_asset(reference.clip.asset_id)
                    .map(|asset| {
                        if let Some(frame) = reference.frame_reference {
                            format!("{} ({})", asset.name, frame.label())
                        } else {
                            asset.name.clone()
                        }
                    })
            })
            .collect();
        self.bridge_keyframe_confirmation = Some(BridgeKeyframeConfirmation {
            clip_ids: reference_clips
                .iter()
                .map(|reference| reference.clip.id)
                .collect(),
            convert_clip_ids,
            sample_names,
            provider_id,
        });
    }

    pub(super) fn bridge_reference_clips(&self, clips: &[Clip]) -> Vec<BridgeReferenceClip> {
        let mut visual_clips: Vec<Clip> = clips
            .iter()
            .filter(|clip| {
                self.editor
                    .project
                    .find_asset(clip.asset_id)
                    .is_some_and(|asset| asset.is_image() || asset.is_video())
            })
            .cloned()
            .collect();
        visual_clips.sort_by(|a, b| {
            a.start_time
                .partial_cmp(&b.start_time)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        if visual_clips.len() < 2 {
            return Vec::new();
        }

        let first_clip = visual_clips.first().cloned();
        let last_clip = visual_clips.last().cloned();
        let (Some(first_clip), Some(last_clip)) = (first_clip, last_clip) else {
            return Vec::new();
        };
        let Some(first_asset) = self.editor.project.find_asset(first_clip.asset_id) else {
            return Vec::new();
        };
        let Some(last_asset) = self.editor.project.find_asset(last_clip.asset_id) else {
            return Vec::new();
        };

        let mut references = vec![
            bridge_reference_for_clip(first_clip, first_asset, true),
            bridge_reference_for_clip(last_clip, last_asset, false),
        ];
        references.sort_by(|a, b| {
            a.anchor_time
                .partial_cmp(&b.anchor_time)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.clip.id.cmp(&b.clip.id))
        });
        references
    }

    pub(super) fn create_bridge_video_from_clip_ids(
        &mut self,
        clip_ids: &[Uuid],
        provider_id: Option<Uuid>,
    ) {
        let clips: Vec<Clip> = self
            .editor
            .project
            .clips
            .iter()
            .filter(|clip| clip_ids.contains(&clip.id))
            .cloned()
            .collect();
        self.create_bridge_video_from_selected_clips(&clips, provider_id);
    }

    pub(super) fn create_i2v_from_single_clip(
        &mut self,
        clip_id: Uuid,
        reference: SingleI2VReference,
        provider_id: Option<Uuid>,
    ) {
        let Some(source_clip) = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
        else {
            self.editor.status = "Selected clip was not found.".to_string();
            return;
        };
        let Some(source_asset) = self
            .editor
            .project
            .find_asset(source_clip.asset_id)
            .cloned()
        else {
            self.editor.status = "Selected clip source asset was not found.".to_string();
            return;
        };

        let (start_time, frame_reference, status_reference) = match reference {
            SingleI2VReference::Image if source_asset.is_image() => {
                (source_clip.start_time, None, "image")
            }
            SingleI2VReference::VideoFirstFrame if source_asset.is_video() => (
                source_clip.start_time,
                Some(SourceFrameReference::First),
                "video first frame",
            ),
            SingleI2VReference::VideoLastFrame if source_asset.is_video() => (
                source_clip.end_time(),
                Some(SourceFrameReference::Last),
                "video last frame",
            ),
            _ => {
                self.editor.status =
                    "Selected clip is not compatible with that I2V action.".to_string();
                return;
            }
        };

        let fps = default_generative_video_fps();
        let frame_count = default_generative_video_frames();
        let duration = frame_count as f64 / fps.max(1.0);
        let target_track_id = self.bridge_target_track_id(source_clip.track_id);
        let asset_id = match self.editor.create_generative_video(fps, frame_count) {
            Ok(asset_id) => asset_id,
            Err(err) => {
                self.editor.status = err;
                return;
            }
        };

        let mut next_config =
            self.continuation_config_from_source_asset(source_clip.asset_id, provider_id);
        next_config.reference_slots.remove("end_image");
        let start_reference = InputValue::AssetRef {
            asset_id: source_clip.asset_id,
            source_clip_id: Some(source_clip.id),
            pinned: true,
            frame_reference,
        };
        next_config
            .reference_slots
            .insert("start_image".to_string(), start_reference.clone());
        next_config
            .reference_slots
            .insert("image".to_string(), start_reference);
        self.editor
            .project
            .update_generative_config(asset_id, move |config| {
                *config = next_config;
            });
        let config_save_error = self.editor.project.save_generative_config(asset_id).err();

        let mut clip = Clip::new(asset_id, target_track_id, start_time, duration);
        clip.label = Some("I2V".to_string());
        let new_clip_id = self.editor.project.add_clip(clip);
        self.editor.selection.select_clip(new_clip_id);
        self.editor.preview_dirty = true;
        self.editor.status = if let Some(err) = config_save_error {
            format!("I2V clip created from {status_reference}, but config save failed: {err}")
        } else {
            format!("Created I2V clip from {status_reference}.")
        };
    }

    pub(super) fn create_i2i_from_single_clip(&mut self, clip_id: Uuid, provider_id: Option<Uuid>) {
        let Some(source_clip) = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
        else {
            self.editor.status = "Selected clip was not found.".to_string();
            return;
        };
        let Some(source_asset) = self
            .editor
            .project
            .find_asset(source_clip.asset_id)
            .cloned()
        else {
            self.editor.status = "Selected clip source asset was not found.".to_string();
            return;
        };
        if !source_asset.is_image() {
            self.editor.status = "Select an image clip for I2I generation.".to_string();
            return;
        }

        let target_track_id = self.bridge_target_track_id(source_clip.track_id);
        let asset_id = match self.editor.create_generative_image() {
            Ok(asset_id) => asset_id,
            Err(err) => {
                self.editor.status = err;
                return;
            }
        };

        let mut next_config =
            self.continuation_config_from_source_asset(source_clip.asset_id, provider_id);
        next_config.reference_slots.insert(
            "image".to_string(),
            InputValue::AssetRef {
                asset_id: source_clip.asset_id,
                source_clip_id: Some(source_clip.id),
                pinned: true,
                frame_reference: None,
            },
        );
        self.editor
            .project
            .update_generative_config(asset_id, move |config| {
                *config = next_config;
            });
        let config_save_error = self.editor.project.save_generative_config(asset_id).err();

        let mut clip = Clip::new(
            asset_id,
            target_track_id,
            source_clip.start_time,
            source_clip.duration,
        );
        clip.label = Some("I2I".to_string());
        let new_clip_id = self.editor.project.add_clip(clip);
        self.editor.selection.select_clip(new_clip_id);
        self.editor.preview_dirty = true;
        self.editor.status = if let Some(err) = config_save_error {
            format!("I2I clip created, but config save failed: {err}")
        } else {
            "Created I2I clip from image.".to_string()
        };
    }

    pub(super) fn create_generative_image_clip_on_track(
        &mut self,
        track_id: Uuid,
        start_time: f64,
        provider_id: Option<Uuid>,
    ) {
        let Some(track) = self
            .editor
            .project
            .tracks
            .iter()
            .find(|track| track.id == track_id)
            .cloned()
        else {
            self.editor.status = "Timeline track was not found.".to_string();
            return;
        };
        if track.track_type != TrackType::Video {
            self.editor.status =
                "Generative images can only be placed on video tracks.".to_string();
            return;
        }

        let asset_id = match self.editor.create_generative_image() {
            Ok(asset_id) => asset_id,
            Err(err) => {
                self.editor.status = err;
                return;
            }
        };
        if provider_id.is_some() {
            self.editor
                .project
                .set_generative_provider_id(asset_id, provider_id);
            if let Err(err) = self.editor.project.save_generative_config(asset_id) {
                self.editor.status =
                    format!("Created generative image, but config save failed: {err}");
            }
        }

        match self
            .editor
            .add_asset_to_timeline_track(asset_id, track_id, Some(start_time))
        {
            Ok(_) => {
                self.editor.status = format!("Created generative image clip on {}", track.name);
            }
            Err(err) => {
                self.editor.status = err;
            }
        }
    }

    pub(super) fn create_bridge_video_from_selected_clips(
        &mut self,
        clips: &[Clip],
        provider_id: Option<Uuid>,
    ) {
        let reference_clips = self.bridge_reference_clips(clips);
        let (Some(first), Some(last)) = (reference_clips.first(), reference_clips.last()) else {
            self.editor.status =
                "Select at least two image or video reference clips first.".to_string();
            return;
        };
        if first.clip.id == last.clip.id {
            self.editor.status = "Select two different reference clips.".to_string();
            return;
        }

        let fallback_duration =
            default_generative_video_frames() as f64 / default_generative_video_fps();
        let duration = (last.anchor_time - first.anchor_time)
            .abs()
            .max(fallback_duration);
        let fps = default_generative_video_fps();
        let frame_count = frames_from_seconds(duration, fps).round().max(1.0) as u32;
        let start_time = first.anchor_time.min(last.anchor_time);
        let target_track_id = self.bridge_target_track_id(first.clip.track_id);

        let asset_id = match self.editor.create_generative_video(fps, frame_count) {
            Ok(asset_id) => asset_id,
            Err(err) => {
                self.editor.status = err;
                return;
            }
        };

        let seed_source_asset_id = [first.clip.asset_id, last.clip.asset_id]
            .into_iter()
            .find(|asset_id| self.editor.project.generative_config(*asset_id).is_some())
            .unwrap_or(first.clip.asset_id);
        let mut next_config =
            self.continuation_config_from_source_asset(seed_source_asset_id, provider_id);
        next_config.reference_slots.insert(
            "start_image".to_string(),
            InputValue::AssetRef {
                asset_id: first.clip.asset_id,
                source_clip_id: Some(first.clip.id),
                pinned: true,
                frame_reference: first.frame_reference,
            },
        );
        next_config.reference_slots.insert(
            "end_image".to_string(),
            InputValue::AssetRef {
                asset_id: last.clip.asset_id,
                source_clip_id: Some(last.clip.id),
                pinned: true,
                frame_reference: last.frame_reference,
            },
        );
        self.editor
            .project
            .update_generative_config(asset_id, move |config| {
                *config = next_config;
            });
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Bridge created, but config save failed: {err}");
        }

        let mut clip = Clip::new(asset_id, target_track_id, start_time, duration);
        clip.label = Some("I2V bridge".to_string());
        let clip_id = self.editor.project.add_clip(clip);
        self.editor.selection.select_clip(clip_id);
        self.editor.preview_dirty = true;
        self.editor.status =
            "Created generative video bridge from selected references.".to_string();
    }

    fn continuation_config_from_source_asset(
        &self,
        source_asset_id: Uuid,
        provider_override: Option<Uuid>,
    ) -> GenerativeConfig {
        let Some(source_config) = self.editor.project.generative_config(source_asset_id) else {
            let mut config = GenerativeConfig {
                provider_id: provider_override,
                ..Default::default()
            };
            self.retain_config_inputs_for_provider(&mut config);
            return config;
        };

        let mut config = GenerativeConfig {
            provider_id: source_config.provider_id,
            inputs: source_config.inputs.clone(),
            reference_slots: source_config.reference_slots.clone(),
            batch: source_config.batch.clone(),
            ..Default::default()
        };

        if let Some(active_version) = self
            .editor
            .project
            .find_asset(source_asset_id)
            .and_then(|asset| asset.active_version())
            .or(source_config.active_version.as_deref())
        {
            if let Some(record) = source_config
                .versions
                .iter()
                .find(|record| record.version == active_version)
            {
                config.provider_id = Some(record.provider_id);
                config.inputs = generation_record_source_inputs(source_config, record);
            }
        }

        if provider_override.is_some() {
            config.provider_id = provider_override;
        }
        self.retain_config_inputs_for_provider(&mut config);
        config
    }

    fn retain_config_inputs_for_provider(&self, config: &mut GenerativeConfig) {
        let Some(provider_id) = config.provider_id else {
            return;
        };
        let Some(provider) = self
            .editor
            .provider_entries
            .iter()
            .find(|provider| provider.id == provider_id)
        else {
            return;
        };
        let input_names: HashSet<&str> = provider
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect();
        config
            .inputs
            .retain(|name, _| input_names.contains(name.as_str()));
        if config
            .batch
            .seed_field
            .as_deref()
            .is_some_and(|seed_field| !input_names.contains(seed_field))
        {
            config.batch.seed_field = None;
        }
    }

    pub(super) fn bridge_target_track_id(&mut self, source_track_id: Uuid) -> Uuid {
        let source_index = self
            .editor
            .project
            .tracks
            .iter()
            .position(|track| track.id == source_track_id)
            .unwrap_or(0);
        if let Some(track) = self
            .editor
            .project
            .tracks
            .iter()
            .take(source_index)
            .rev()
            .find(|track| track.track_type == TrackType::Video)
        {
            return track.id;
        }

        let track_id = self.editor.project.add_video_track();
        while self
            .editor
            .project
            .tracks
            .iter()
            .position(|track| track.id == track_id)
            .is_some_and(|index| index > source_index)
        {
            if !self.editor.project.move_track_up(track_id) {
                break;
            }
        }
        track_id
    }

    pub(super) fn clip_attributes(&mut self, ui: &mut Ui, clip_id: Uuid) {
        let clip_asset_id = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .map(|clip| clip.asset_id);
        let asset_name = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .and_then(|clip| self.editor.project.find_asset(clip.asset_id))
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| "Unknown asset".to_string());
        let clip_asset_is_image = clip_asset_id
            .and_then(|asset_id| self.editor.project.find_asset(asset_id))
            .is_some_and(|asset| asset.is_image());
        let mut preview_dirty = false;
        if let Some(clip) = self
            .editor
            .project
            .clips
            .iter_mut()
            .find(|clip| clip.id == clip_id)
        {
            inspector_card(ui, "Clip", |ui| {
                kit::field_label(ui, "Source Asset");
                let source_w = ui.available_width();
                kit::readonly_value_box(ui, asset_name, Vec2::new(source_w, kit::FIELD_H));
                ui.add_space(kit::FORM_ROW_GAP);
                let mut label = clip.label.clone().unwrap_or_default();
                if inspector_text_field(ui, "Clip Label", &mut label) {
                    clip.label = if label.trim().is_empty() {
                        None
                    } else {
                        Some(label)
                    };
                }
                if clip_asset_is_image {
                    ui.add_space(kit::FORM_ROW_GAP);
                    let mut next_mode = clip.image_mode;
                    kit::labeled_combo_field(
                        ui,
                        "Timeline Display",
                        ("clip_image_mode", clip.id),
                        clip_image_mode_label(next_mode),
                        |ui| {
                            automation_selectable_value(
                                ui,
                                &mut next_mode,
                                ClipImageMode::Still,
                                "Still Image",
                            );
                            automation_selectable_value(
                                ui,
                                &mut next_mode,
                                ClipImageMode::Keyframe,
                                "Keyframe Reference",
                            );
                        },
                    );
                    if next_mode != clip.image_mode {
                        clip.image_mode = next_mode;
                    }
                }
            });
            ui.add_space(kit::FORM_ROW_GAP);
            inspector_card(ui, "Transform", |ui| {
                transform_editor(ui, &mut clip.transform, &mut preview_dirty);
            });
            ui.add_space(kit::FORM_ROW_GAP);
            inspector_card(ui, "Timing", |ui| {
                if clip_asset_is_image && clip.image_mode == ClipImageMode::Keyframe {
                    preview_dirty |= inspector_drag_f64(
                        ui,
                        "Time",
                        &mut clip.start_time,
                        0.05,
                        ui.available_width(),
                    );
                } else {
                    preview_dirty |= inspector_two_drag_f64(
                        ui,
                        ("Start", &mut clip.start_time, 0.05),
                        ("Duration", &mut clip.duration, 0.05),
                    );
                }
            });
        }
        if let Some(asset_id) = clip_asset_id {
            if generative_output_for_asset(&self.editor.project, asset_id).is_some() {
                ui.add_space(kit::FORM_ROW_GAP);
                self.generative_asset_attributes(ui, asset_id, Some(clip_id));
            }
        }
        if preview_dirty {
            self.editor.preview_dirty = true;
        }
    }

    pub(super) fn generative_asset_attributes(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
    ) {
        let Some((folder, output_type)) =
            generative_output_for_asset(&self.editor.project, asset_id)
        else {
            return;
        };
        let config_snapshot = self
            .editor
            .project
            .generative_config(asset_id)
            .cloned()
            .unwrap_or_default();
        let folder_path = self
            .editor
            .project
            .project_path
            .as_ref()
            .map(|root| root.join(&folder));
        let asset_label = self
            .editor
            .project
            .find_asset(asset_id)
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| "Generative Asset".to_string());

        let compatible_providers: Vec<ProviderEntry> = self
            .editor
            .provider_entries
            .iter()
            .filter(|entry| entry.output_type == output_type)
            .cloned()
            .collect();
        let selected_provider_id = config_snapshot.provider_id;
        let selected_provider = selected_provider_id.and_then(|id| {
            compatible_providers
                .iter()
                .find(|entry| entry.id == id)
                .cloned()
        });
        let show_missing_provider = selected_provider_id.is_some() && selected_provider.is_none();

        let mut version_options: Vec<String> = config_snapshot
            .versions
            .iter()
            .map(|record| record.version.clone())
            .collect();
        if let Some(active) = config_snapshot.active_version.as_ref() {
            if !active.trim().is_empty() && !version_options.contains(active) {
                version_options.push(active.clone());
            }
        }
        version_options.sort_by(
            |a, b| match (parse_version_index(a), parse_version_index(b)) {
                (Some(a_num), Some(b_num)) => b_num.cmp(&a_num),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b.cmp(a),
            },
        );
        version_options.dedup();

        let selected_version_value = config_snapshot.active_version.clone().unwrap_or_default();
        let provider_label = selected_provider
            .as_ref()
            .map(|provider| provider.name.clone())
            .unwrap_or_else(|| "None selected".to_string());

        let batch = config_snapshot.batch.clone();
        let seed_field_options = selected_provider
            .as_ref()
            .map(seed_field_options_for_provider)
            .unwrap_or_default();
        let seed_field_missing = batch
            .seed_field
            .as_ref()
            .map(|field| !seed_field_options.iter().any(|(name, _)| name == field))
            .unwrap_or(false);
        let resolved_seed_field = selected_provider
            .as_ref()
            .and_then(|provider| resolve_seed_field(provider, batch.seed_field.as_deref()));
        let seed_hint = if seed_field_missing {
            batch
                .seed_field
                .as_ref()
                .map(|field| format!("Seed field '{field}' not found in provider inputs."))
        } else if batch.seed_field.is_none() && selected_provider.is_some() {
            Some(match resolved_seed_field.as_ref() {
                Some(field) => format!("Auto-detect: {field}"),
                None => "Auto-detect: none".to_string(),
            })
        } else {
            None
        };
        let batch_hint = if batch.count > 1 {
            match batch.seed_strategy {
                SeedStrategy::Keep => {
                    Some("Identical inputs can be cached; use Increment or Random.".to_string())
                }
                _ if resolved_seed_field.is_none() => {
                    Some("No numeric seed field detected. Pick one to offset seeds.".to_string())
                }
                _ => None,
            }
        } else {
            None
        };

        let mut next_version = selected_version_value.clone();
        let mut next_provider_id = selected_provider_id;
        let mut next_batch_count = batch.count.max(1).min(MAX_GENERATION_BATCH_COUNT) as i64;
        let mut next_seed_strategy = batch.seed_strategy;
        let mut next_seed_field = batch.seed_field.clone().unwrap_or_default();
        let mut open_asset_lab = false;
        let mut generate_clicked = false;

        inspector_card(ui, "Generative", |ui| {
            kit::field_label(ui, "Version");
            let row_w = ui.available_width();
            let (row_rect, _) =
                ui.allocate_exact_size(Vec2::new(row_w, kit::FIELD_H), Sense::hover());
            let mut row_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(row_rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            row_ui.shrink_clip_rect(row_rect);
            row_ui.spacing_mut().item_spacing.x = kit::FIELD_COMPOUND_GAP;
            StripBuilder::new(&mut row_ui)
                .clip(true)
                .size(Size::remainder().at_least(86.0))
                .size(Size::exact(kit::BROWSE_BUTTON_W))
                .horizontal(|mut strip| {
                    strip.cell(|ui| {
                        let selected_text = if next_version.trim().is_empty() {
                            "No versions yet".to_string()
                        } else {
                            next_version.clone()
                        };
                        kit::combo_field(
                            ui,
                            ("gen_version", asset_id),
                            selected_text,
                            ui.available_width(),
                            |ui| {
                                if version_options.is_empty() {
                                    ui.label(kit::caption("No versions yet"));
                                } else {
                                    for version in version_options.iter() {
                                        automation_selectable_value(
                                            ui,
                                            &mut next_version,
                                            version.clone(),
                                            version,
                                        );
                                    }
                                }
                            },
                        );
                    });
                    strip.cell(|ui| {
                        let button_w = ui.available_width();
                        if kit::field_button(ui, "Manage", button_w).clicked() {
                            open_asset_lab = true;
                        }
                    });
                });

            ui.add_space(kit::FORM_ROW_GAP);
            kit::field_label(ui, "Provider");
            kit::combo_field(
                ui,
                ("gen_provider", asset_id),
                provider_label,
                ui.available_width(),
                |ui| {
                    automation_selectable_value(ui, &mut next_provider_id, None, "None selected");
                    for provider in compatible_providers.iter() {
                        automation_selectable_value(
                            ui,
                            &mut next_provider_id,
                            Some(provider.id),
                            provider.name.as_str(),
                        );
                    }
                },
            );

            if show_missing_provider {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(
                    RichText::new("Selected provider is missing from local providers.")
                        .color(kit::MARKER)
                        .size(11.0),
                );
            } else if compatible_providers.is_empty() {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(format!(
                    "No {:?} providers configured.",
                    output_type
                )));
            }

            ui.add_space(kit::ACTION_GAP);
            let generate_w = ui.available_width();
            if kit::primary_button(ui, "Generate", generate_w).clicked() {
                generate_clicked = true;
            }
            if let Some(status) = self.generation_status_for_asset(asset_id) {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(status));
            }
        });

        ui.add_space(kit::FORM_ROW_GAP);
        inspector_card(ui, "Batch", |ui| {
            if inspector_drag_i64(
                ui,
                "Count",
                &mut next_batch_count,
                1.0,
                ui.available_width(),
            ) {
                next_batch_count = next_batch_count.clamp(1, MAX_GENERATION_BATCH_COUNT as i64);
            }
            ui.add_space(kit::FORM_ROW_GAP);
            let mut draw_strategy = |ui: &mut Ui| {
                kit::labeled_combo_field(
                    ui,
                    "Seed Strategy",
                    ("seed_strategy", asset_id),
                    seed_strategy_label(next_seed_strategy),
                    |ui| {
                        automation_selectable_value(
                            ui,
                            &mut next_seed_strategy,
                            SeedStrategy::Increment,
                            "Increment",
                        );
                        automation_selectable_value(
                            ui,
                            &mut next_seed_strategy,
                            SeedStrategy::Random,
                            "Random",
                        );
                        automation_selectable_value(
                            ui,
                            &mut next_seed_strategy,
                            SeedStrategy::Keep,
                            "Keep",
                        );
                    },
                );
            };
            let mut draw_seed_field = |ui: &mut Ui| {
                let selected_text = if next_seed_field.trim().is_empty() {
                    "Auto-detect".to_string()
                } else {
                    seed_field_options
                        .iter()
                        .find(|(name, _)| name == &next_seed_field)
                        .map(|(_, label)| label.clone())
                        .unwrap_or_else(|| next_seed_field.clone())
                };
                kit::labeled_combo_field(
                    ui,
                    "Seed Field",
                    ("seed_field", asset_id),
                    selected_text,
                    |ui| {
                        automation_selectable_value(
                            ui,
                            &mut next_seed_field,
                            String::new(),
                            "Auto-detect",
                        );
                        for (name, label) in seed_field_options.iter() {
                            automation_selectable_value(
                                ui,
                                &mut next_seed_field,
                                name.clone(),
                                label,
                            );
                        }
                    },
                );
            };
            if ui.available_width() >= 210.0 {
                ui.columns(2, |columns| {
                    draw_strategy(&mut columns[0]);
                    draw_seed_field(&mut columns[1]);
                });
            } else {
                draw_strategy(ui);
                ui.add_space(kit::FORM_ROW_GAP);
                draw_seed_field(ui);
            }
            if let Some(hint) = seed_hint {
                ui.add_space(kit::FORM_ROW_GAP);
                let color = if seed_field_missing {
                    kit::MARKER
                } else {
                    kit::TEXT_DIM
                };
                ui.label(RichText::new(hint).color(color).size(11.0));
            }
            if let Some(hint) = batch_hint {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(RichText::new(hint).color(kit::MARKER).size(11.0));
            }
        });

        ui.add_space(kit::FORM_ROW_GAP);
        let input_updates = self.provider_inputs_card(
            ui,
            asset_id,
            context_clip_id,
            selected_provider.clone(),
            &config_snapshot,
        );

        let mut config_dirty = false;
        let mut preview_dirty = false;
        if next_version != selected_version_value {
            let next_active = if next_version.trim().is_empty() {
                None
            } else {
                Some(next_version.trim().to_string())
            };
            self.editor
                .project
                .update_generative_config(asset_id, |config| {
                    config.active_version = next_active.clone();
                    if let Some(version) = next_active.as_ref() {
                        if let Some(record) = config
                            .versions
                            .iter()
                            .find(|record| record.version == *version)
                        {
                            config.inputs = record.inputs_snapshot.clone();
                            config.provider_id = Some(record.provider_id);
                        }
                    }
                });
            config_dirty = true;
            preview_dirty = true;
        }
        if next_provider_id != selected_provider_id {
            self.editor
                .project
                .set_generative_provider_id(asset_id, next_provider_id);
            config_dirty = true;
        }
        let clamped_batch_count =
            next_batch_count.clamp(1, MAX_GENERATION_BATCH_COUNT as i64) as u32;
        let next_seed_field_opt = if next_seed_field.trim().is_empty() {
            None
        } else {
            Some(next_seed_field.trim().to_string())
        };
        if clamped_batch_count != batch.count
            || next_seed_strategy != batch.seed_strategy
            || next_seed_field_opt != batch.seed_field
        {
            self.editor
                .project
                .update_generative_config(asset_id, |config| {
                    config.batch.count = clamped_batch_count;
                    config.batch.seed_strategy = next_seed_strategy;
                    config.batch.seed_field = next_seed_field_opt;
                });
            config_dirty = true;
        }
        if !input_updates.is_empty() {
            self.editor
                .project
                .update_generative_config(asset_id, |config| {
                    for (name, value) in input_updates {
                        config.inputs.insert(name, value);
                    }
                });
            config_dirty = true;
        }
        if config_dirty {
            if let Err(err) = self.editor.project.save_generative_config(asset_id) {
                self.editor.status = format!("Failed to save generative config: {err}");
            }
        }
        if preview_dirty {
            self.invalidate_generative_asset_runtime(asset_id);
        }

        if open_asset_lab {
            let local_time = context_clip_id.and_then(|clip_id| {
                self.editor.project.clips.iter().find_map(|clip| {
                    (clip.id == clip_id).then(|| {
                        (self.editor.current_time - clip.start_time + clip.trim_in_seconds).max(0.0)
                    })
                })
            });
            self.open_asset_lab_at_time(asset_id, local_time);
        }

        if generate_clicked {
            let config_for_generation = self
                .editor
                .project
                .generative_config(asset_id)
                .cloned()
                .unwrap_or(config_snapshot);
            let Some(provider_id) = config_for_generation.provider_id else {
                self.editor.status = "Select a provider first.".to_string();
                return;
            };
            let Some(provider) = compatible_providers
                .into_iter()
                .find(|provider| provider.id == provider_id)
            else {
                self.editor.status = "Selected provider is unavailable.".to_string();
                return;
            };
            let Some(folder_path) = folder_path else {
                self.editor.status = "Project folder is unavailable.".to_string();
                return;
            };
            match self.enqueue_generation_jobs(
                asset_id,
                context_clip_id,
                None,
                provider,
                config_for_generation,
                folder_path,
                asset_label,
            ) {
                Ok(status) => {
                    self.editor.status = status;
                }
                Err(err) => self.editor.status = err,
            }
        }
    }

    pub(super) fn provider_inputs_card(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        selected_provider: Option<ProviderEntry>,
        config_snapshot: &GenerativeConfig,
    ) -> Vec<(String, InputValue)> {
        let mut updates = Vec::new();
        inspector_card(ui, "Provider Inputs", |ui| {
            let Some(provider) = selected_provider else {
                ui.label(kit::caption("Select a provider to configure inputs."));
                return;
            };
            if provider.inputs.is_empty() {
                ui.label(kit::caption("No inputs defined."));
                return;
            }

            for (index, input) in provider.inputs.iter().enumerate() {
                if index > 0 {
                    ui.add_space(kit::FORM_ROW_GAP);
                }
                let label = if input.required {
                    format!("{} *", input.label)
                } else {
                    input.label.clone()
                };
                let current_value = literal_config_input(config_snapshot, &input.name)
                    .or_else(|| input.default.clone());
                match &input.input_type {
                    ProviderInputType::Text => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_string)
                            .unwrap_or_default();
                        let multiline = input.ui.as_ref().map(|ui| ui.multiline).unwrap_or(false);
                        let changed = if multiline {
                            inspector_multiline_text_field(
                                ui,
                                &label,
                                &mut value,
                                kit::MultilineTextFieldOptions::rows(3),
                            )
                        } else {
                            inspector_text_field(ui, &label, &mut value)
                        };
                        if changed {
                            updates.push((
                                input.name.clone(),
                                InputValue::Literal {
                                    value: serde_json::Value::String(value),
                                },
                            ));
                        }
                    }
                    ProviderInputType::Number => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_f64)
                            .unwrap_or(0.0);
                        let step = input.ui.as_ref().and_then(|ui| ui.step).unwrap_or(0.1);
                        let width = ui.available_width();
                        if inspector_drag_f64(ui, &label, &mut value, step, width) {
                            if let Some(number) = serde_json::Number::from_f64(value) {
                                updates.push((
                                    input.name.clone(),
                                    InputValue::Literal {
                                        value: serde_json::Value::Number(number),
                                    },
                                ));
                            }
                        }
                    }
                    ProviderInputType::Integer => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_i64)
                            .unwrap_or(0);
                        let step = input.ui.as_ref().and_then(|ui| ui.step).unwrap_or(1.0);
                        let width = ui.available_width();
                        if inspector_drag_i64(ui, &label, &mut value, step, width) {
                            updates.push((
                                input.name.clone(),
                                InputValue::Literal {
                                    value: serde_json::Value::Number(value.into()),
                                },
                            ));
                        }
                    }
                    ProviderInputType::Boolean => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_bool)
                            .unwrap_or(false);
                        if inspector_bool_field(ui, &label, &mut value) {
                            updates.push((
                                input.name.clone(),
                                InputValue::Literal {
                                    value: serde_json::Value::Bool(value),
                                },
                            ));
                        }
                    }
                    ProviderInputType::Enum { options } => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_string)
                            .or_else(|| options.first().cloned())
                            .unwrap_or_default();
                        let before = value.clone();
                        kit::labeled_combo_field(
                            ui,
                            &label,
                            ("provider_input_enum", asset_id, &input.name),
                            empty_dash(&value).to_string(),
                            |ui| {
                                for option in options {
                                    automation_selectable_value(
                                        ui,
                                        &mut value,
                                        option.clone(),
                                        option,
                                    );
                                }
                            },
                        );
                        if value != before {
                            updates.push((
                                input.name.clone(),
                                InputValue::Literal {
                                    value: serde_json::Value::String(value),
                                },
                            ));
                        }
                    }
                    ProviderInputType::Image
                    | ProviderInputType::Video
                    | ProviderInputType::Audio => {
                        if let Some(update) =
                            self.provider_asset_input_field(ui, asset_id, context_clip_id, input)
                        {
                            updates.push((input.name.clone(), update));
                        }
                    }
                }
            }
        });
        updates
    }

    pub(super) fn provider_asset_input_field(
        &self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        input: &ProviderInputField,
    ) -> Option<InputValue> {
        let config = self.editor.project.generative_config(asset_id);
        let field_value = config
            .and_then(|config| config.inputs.get(&input.name))
            .cloned();
        let reference_slot = semantic_reference_slot(input);
        let slot_value = reference_slot.and_then(|slot| {
            config
                .and_then(|config| config.reference_slots.get(slot))
                .cloned()
        });
        let current_binding = field_value.clone().or(slot_value.clone());
        let candidates = self.asset_input_candidates(input, context_clip_id);
        let auto_candidate = candidates
            .iter()
            .find(|candidate| candidate.contextual)
            .cloned();
        let resolved_binding = match current_binding.clone() {
            Some(InputValue::AssetRef { pinned: false, .. }) => auto_candidate
                .as_ref()
                .map(|candidate| InputValue::AssetRef {
                    asset_id: candidate.asset_id,
                    source_clip_id: candidate.source_clip_id,
                    pinned: false,
                    frame_reference: candidate.frame_reference,
                })
                .or(current_binding.clone()),
            Some(value) => Some(value),
            None => auto_candidate
                .as_ref()
                .map(|candidate| InputValue::AssetRef {
                    asset_id: candidate.asset_id,
                    source_clip_id: candidate.source_clip_id,
                    pinned: false,
                    frame_reference: candidate.frame_reference,
                }),
        };

        let current_label = resolved_binding
            .as_ref()
            .and_then(|value| self.asset_input_label(value, context_clip_id))
            .unwrap_or_else(|| {
                if context_clip_id.is_some() {
                    "Auto: no match".to_string()
                } else {
                    "None selected".to_string()
                }
            });
        let combo_id = ("provider_asset_input", asset_id, &input.name);
        let before = current_binding.clone();
        let mut next = current_binding.clone();

        kit::labeled_combo_field(ui, &input.label, combo_id, current_label, |ui| {
            if let Some(candidate) = auto_candidate.as_ref() {
                let label = format!("Auto: {}  {}", candidate.label, candidate.detail);
                if ui
                    .selectable_label(
                        matches!(next, Some(InputValue::AssetRef { pinned: false, .. })),
                        label,
                    )
                    .clicked()
                {
                    next = Some(InputValue::AssetRef {
                        asset_id: candidate.asset_id,
                        source_clip_id: candidate.source_clip_id,
                        pinned: false,
                        frame_reference: candidate.frame_reference,
                    });
                    ui.close();
                }
                ui.separator();
            } else if context_clip_id.is_some() {
                ui.label(kit::caption("No timeline match"));
                ui.separator();
            }

            let mut drew_context_header = false;
            let mut drew_other_header = false;
            for candidate in candidates.iter() {
                if candidate.contextual && !drew_context_header {
                    ui.label(kit::caption("Timeline context"));
                    drew_context_header = true;
                } else if !candidate.contextual && !drew_other_header {
                    if drew_context_header {
                        ui.separator();
                    }
                    ui.label(kit::caption("Other project assets"));
                    drew_other_header = true;
                }
                let selected = binding_matches_candidate(next.as_ref(), candidate, true);
                if ui
                    .selectable_label(
                        selected,
                        format!("{}  {}", candidate.label, candidate.detail),
                    )
                    .clicked()
                {
                    next = Some(InputValue::AssetRef {
                        asset_id: candidate.asset_id,
                        source_clip_id: candidate.source_clip_id,
                        pinned: true,
                        frame_reference: candidate.frame_reference,
                    });
                    ui.close();
                }
            }
        });

        if let Some(InputValue::AssetRef {
            asset_id,
            source_clip_id,
            pinned,
            frame_reference,
        }) = resolved_binding.as_ref()
        {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let hint = if *pinned {
                    if source_clip_id.is_some() {
                        "Pinned to timeline clip"
                    } else {
                        "Pinned to asset"
                    }
                } else if context_clip_id.is_some() {
                    "Auto from timeline proximity"
                } else {
                    "No timeline context; using saved asset"
                };
                ui.label(kit::caption(hint));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let action = if *pinned { "Unpin" } else { "Pin" };
                    if ui.small_button(action).clicked() {
                        next = Some(InputValue::AssetRef {
                            asset_id: *asset_id,
                            source_clip_id: *source_clip_id,
                            pinned: !*pinned,
                            frame_reference: *frame_reference,
                        });
                    }
                });
            });
        }

        if next != before {
            next
        } else {
            None
        }
    }

    pub(super) fn asset_input_label(
        &self,
        value: &InputValue,
        context_clip_id: Option<Uuid>,
    ) -> Option<String> {
        match value {
            InputValue::AssetRef {
                asset_id,
                source_clip_id,
                pinned,
                frame_reference,
            } => {
                let asset = self.editor.project.find_asset(*asset_id)?;
                let prefix = if *pinned {
                    "Pinned"
                } else if context_clip_id.is_some() {
                    "Auto"
                } else {
                    "Saved"
                };
                let clip_suffix = source_clip_id
                    .and_then(|clip_id| {
                        self.editor
                            .project
                            .clips
                            .iter()
                            .find(|clip| clip.id == clip_id)
                    })
                    .map(|clip| {
                        let anchor = if *frame_reference == Some(SourceFrameReference::Last) {
                            clip.end_time()
                        } else {
                            clip.start_time
                        };
                        format!(" @ {}", timecode(anchor))
                    })
                    .unwrap_or_default();
                let frame_suffix = frame_reference
                    .map(|frame| format!(" ({})", frame.label()))
                    .unwrap_or_default();
                Some(format!(
                    "{prefix}: {}{}{}",
                    asset_display_name(asset),
                    clip_suffix,
                    frame_suffix
                ))
            }
            InputValue::GenerationRef {
                asset_id,
                version,
                frame_reference,
            } => {
                let asset = self.editor.project.find_asset(*asset_id)?;
                let frame_suffix = frame_reference
                    .map(|frame| format!(" ({})", frame.label()))
                    .unwrap_or_default();
                Some(format!(
                    "Internal: {} {}{}",
                    asset.name, version, frame_suffix
                ))
            }
            InputValue::Literal { .. } => None,
        }
    }

    pub(super) fn asset_input_candidates(
        &self,
        input: &ProviderInputField,
        context_clip_id: Option<Uuid>,
    ) -> Vec<AssetInputCandidate> {
        let mut candidates = Vec::new();
        let context_clip = context_clip_id
            .and_then(|id| self.editor.project.clips.iter().find(|clip| clip.id == id));
        let slot = semantic_reference_slot(input).unwrap_or("asset");
        let target_time = context_clip.map(|clip| {
            if slot.starts_with("end") {
                clip.end_time()
            } else {
                clip.start_time
            }
        });
        let context_track_index = context_clip.and_then(|clip| {
            self.editor
                .project
                .tracks
                .iter()
                .position(|track| track.id == clip.track_id)
        });

        for clip in self.editor.project.clips.iter() {
            if Some(clip.id) == context_clip_id {
                continue;
            }
            let Some(asset) = self.editor.project.find_asset(clip.asset_id) else {
                continue;
            };
            let Some((time_distance, detail, frame_reference)) =
                self.asset_input_clip_candidate(input, slot, target_time, clip, asset)
            else {
                continue;
            };
            let track_score = match (
                context_track_index,
                self.editor
                    .project
                    .tracks
                    .iter()
                    .position(|track| track.id == clip.track_id),
            ) {
                (Some(context_index), Some(index)) if index == context_index + 1 => 0.0,
                (Some(context_index), Some(index)) if index == context_index => 0.05,
                (Some(context_index), Some(index)) if index > context_index => {
                    0.1 + (index - context_index - 1) as f64 * 0.15
                }
                (Some(context_index), Some(index)) => (context_index - index) as f64 * 0.5,
                _ => 0.0,
            };
            candidates.push(AssetInputCandidate {
                asset_id: asset.id,
                source_clip_id: Some(clip.id),
                frame_reference,
                label: asset_display_name(asset),
                detail,
                contextual: context_clip_id.is_some(),
                score: time_distance + track_score,
            });
        }

        let mut seen_assets: HashSet<Uuid> = candidates
            .iter()
            .map(|candidate| candidate.asset_id)
            .collect();
        for asset in self.editor.project.assets.iter() {
            if seen_assets.contains(&asset.id)
                || !compatible_asset_for_provider_input(asset, &input.input_type)
            {
                continue;
            }
            seen_assets.insert(asset.id);
            candidates.push(AssetInputCandidate {
                asset_id: asset.id,
                source_clip_id: None,
                frame_reference: None,
                label: asset_display_name(asset),
                detail: asset_kind_label(&asset.kind).to_string(),
                contextual: false,
                score: f64::MAX / 2.0,
            });
        }

        candidates.sort_by(|a, b| {
            b.contextual
                .cmp(&a.contextual)
                .then_with(|| {
                    a.score
                        .partial_cmp(&b.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.label.cmp(&b.label))
        });
        candidates
    }

    pub(super) fn asset_input_clip_candidate(
        &self,
        input: &ProviderInputField,
        slot: &str,
        target_time: Option<f64>,
        clip: &Clip,
        asset: &Asset,
    ) -> Option<(f64, String, Option<SourceFrameReference>)> {
        match input.input_type {
            ProviderInputType::Image => {
                if asset.is_image() {
                    let anchor = clip.start_time;
                    let distance = target_time
                        .map(|target| (anchor - target).abs())
                        .unwrap_or(0.0);
                    return Some((distance, timecode(anchor), None));
                }
                if asset.is_video() {
                    let target = target_time?;
                    let start_distance = (clip.start_time - target).abs();
                    let end_distance = (clip.end_time() - target).abs();
                    let prefer_last_for_start =
                        slot.starts_with("start") && end_distance <= start_distance;
                    let prefer_first_for_end =
                        slot.starts_with("end") && start_distance <= end_distance;
                    let frame = if prefer_last_for_start {
                        SourceFrameReference::Last
                    } else if prefer_first_for_end || start_distance <= end_distance {
                        SourceFrameReference::First
                    } else {
                        SourceFrameReference::Last
                    };
                    let (anchor, distance) = match frame {
                        SourceFrameReference::First => (clip.start_time, start_distance),
                        SourceFrameReference::Last => (clip.end_time(), end_distance),
                    };
                    let slot_hint = if slot.starts_with("end") {
                        "end ref"
                    } else if slot.starts_with("start") {
                        "start ref"
                    } else {
                        "image ref"
                    };
                    return Some((
                        distance,
                        format!("{} {} @ {}", slot_hint, frame.label(), timecode(anchor)),
                        Some(frame),
                    ));
                }
                None
            }
            ProviderInputType::Video | ProviderInputType::Audio => {
                if !compatible_asset_for_provider_input(asset, &input.input_type) {
                    return None;
                }
                let anchor = clip.start_time;
                let distance = target_time
                    .map(|target| (anchor - target).abs())
                    .unwrap_or(0.0);
                Some((distance, timecode(anchor), None))
            }
            _ => None,
        }
    }

    pub(super) fn asset_attributes(&mut self, ui: &mut Ui, asset_id: Uuid) {
        let mut add_to_timeline = false;
        let mut duplicate_asset = false;
        let mut extract_generation = false;
        let mut open_asset_lab = false;
        let mut rename_to: Option<String> = None;
        let Some(asset_snapshot) = self
            .editor
            .project
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .cloned()
        else {
            return;
        };
        let thumbnail = self.asset_thumbnail(ui.ctx(), &asset_snapshot);
        let kind_label = asset_kind_label(&asset_snapshot.kind).to_string();
        let duration = asset_snapshot.duration_seconds;
        let source = asset_source_label(&asset_snapshot);
        let active_version = asset_snapshot.active_version().map(str::to_string);
        let is_generative = asset_snapshot.is_generative();
        inspector_card(ui, "Asset", |ui| {
            let accent = asset_accent(&asset_snapshot);
            ui.horizontal(|ui| {
                let (thumb_rect, _) =
                    ui.allocate_exact_size(INSPECTOR_THUMBNAIL_SIZE, Sense::hover());
                paint_asset_thumbnail(ui, thumb_rect, &asset_snapshot, accent, thumbnail);
                ui.add_space(2.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(kit::caption("Type"));
                    ui.label(kit::value(&kind_label));
                    if let Some(duration) = duration {
                        ui.label(kit::caption(format_duration(duration)));
                    }
                });
            });
            ui.add_space(kit::FORM_ROW_GAP);
            kit::field_label(ui, "Name");
            let mut name = asset_snapshot.name.clone();
            if kit::singleline_text_field(ui, &mut name, ui.available_width()).changed() {
                rename_to = Some(name);
            }
            ui.add_space(kit::FORM_ROW_GAP);
            if let Some(active_version) = active_version {
                inspector_meta_row(ui, "Version", active_version);
            }
            if let Some(source) = source {
                inspector_meta_row(ui, "Source", source);
            }
            ui.add_space(kit::ACTION_GAP);
            kit::equal_width_action_row(
                ui,
                2,
                kit::SECONDARY_BUTTON_H,
                kit::FIELD_COMPOUND_GAP,
                |ui, index, width| match index {
                    0 => {
                        if kit::secondary_button(ui, "Add to timeline", width).clicked() {
                            add_to_timeline = true;
                        }
                    }
                    _ => {
                        if kit::secondary_button(ui, "Duplicate", width).clicked() {
                            duplicate_asset = true;
                        }
                    }
                },
            );
            if is_generative {
                ui.add_space(kit::FORM_ROW_GAP);
                kit::equal_width_action_row(
                    ui,
                    2,
                    kit::SECONDARY_BUTTON_H,
                    kit::FIELD_COMPOUND_GAP,
                    |ui, index, width| match index {
                        0 => {
                            if kit::secondary_button(ui, "Extract", width).clicked() {
                                extract_generation = true;
                            }
                        }
                        _ => {
                            if kit::secondary_button(ui, "Asset Lab", width).clicked() {
                                open_asset_lab = true;
                            }
                        }
                    },
                );
            }
        });
        if let Some(name) = rename_to {
            if let Err(err) = self.editor.rename_asset(asset_id, name) {
                self.editor.status = err;
            }
        }
        if add_to_timeline {
            if let Err(err) = self.editor.add_asset_to_timeline(asset_id, None) {
                self.editor.status = err;
            }
        }
        if duplicate_asset {
            self.duplicate_assets(&[asset_id]);
        }
        if extract_generation {
            self.extract_active_generation(asset_id);
        }
        if open_asset_lab {
            self.open_asset_lab(asset_id);
        }
        if is_generative {
            ui.add_space(kit::FORM_ROW_GAP);
            self.generative_asset_attributes(ui, asset_id, None);
        }
    }

    pub(super) fn marker_attributes(&mut self, ui: &mut Ui, marker_id: Uuid) {
        let mut should_sort = false;
        let mut delete_marker = false;
        let mut marker_changed = false;
        if let Some(marker) = self
            .editor
            .project
            .markers
            .iter_mut()
            .find(|marker| marker.id == marker_id)
        {
            inspector_card(ui, "Marker", |ui| {
                let mut changed = false;
                let time_w = ui.available_width();
                changed |= inspector_drag_f64(ui, "Time", &mut marker.time, 0.05, time_w);
                ui.add_space(kit::FORM_ROW_GAP);
                let mut label = marker.label.clone().unwrap_or_default();
                if inspector_text_field(ui, "Label", &mut label) {
                    marker.label = if label.trim().is_empty() {
                        None
                    } else {
                        Some(label)
                    };
                    marker_changed = true;
                }
                ui.add_space(kit::FORM_ROW_GAP);
                let mut description = marker.description.clone().unwrap_or_default();
                if inspector_multiline_text_field(
                    ui,
                    "Description",
                    &mut description,
                    kit::MultilineTextFieldOptions::rows(3),
                ) {
                    marker.description = if description.trim().is_empty() {
                        None
                    } else {
                        Some(description)
                    };
                    marker_changed = true;
                }
                ui.add_space(kit::FORM_ROW_GAP);
                let mut color = marker
                    .color
                    .as_deref()
                    .and_then(parse_hex_color)
                    .unwrap_or(kit::MARKER);
                if inspector_color_field(ui, "Color", &mut color) {
                    marker.color = Some(color_to_hex(color));
                    marker_changed = true;
                }
                if changed {
                    should_sort = true;
                    marker_changed = true;
                }
                ui.add_space(kit::ACTION_GAP);
                let delete_w = ui.available_width();
                if kit::danger_button(ui, "Delete Marker", delete_w).clicked() {
                    delete_marker = true;
                }
            });
        }
        if delete_marker {
            self.editor.project.remove_marker(marker_id);
            self.editor.selection.clear();
            self.editor.preview_dirty = true;
            return;
        }
        if marker_changed {
            self.editor.preview_dirty = true;
        }
        if should_sort {
            self.editor
                .project
                .markers
                .sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
            self.editor.preview_dirty = true;
        }
    }

    pub(super) fn track_attributes(&mut self, ui: &mut Ui, track_id: Uuid) {
        let mut track_mute_changed = false;
        let mut preview_dirty = false;
        let mut delete_track = false;
        if let Some(track) = self
            .editor
            .project
            .tracks
            .iter_mut()
            .find(|track| track.id == track_id)
        {
            inspector_card(ui, "Track", |ui| {
                kit::field_label(ui, "Name");
                let name_w = ui.available_width();
                kit::singleline_text_field(ui, &mut track.name, name_w);
                ui.add_space(kit::FORM_ROW_GAP);
                inspector_meta_row(ui, "Type", format!("{:?}", track.track_type));
                if track.track_type != TrackType::Marker {
                    ui.add_space(kit::FORM_ROW_GAP);
                    let before = track.muted;
                    automation_checkbox(ui, &mut track.muted, "Muted");
                    if track.muted != before {
                        track_mute_changed = true;
                        preview_dirty = true;
                    }
                    ui.add_space(kit::FORM_ROW_GAP);
                    let volume_w = ui.available_width();
                    let _ = inspector_drag_f32(ui, "Volume", &mut track.volume, 0.01, volume_w);
                }
                ui.add_space(kit::ACTION_GAP);
                let delete_w = ui.available_width();
                if kit::danger_button(ui, "Delete Track", delete_w).clicked() {
                    delete_track = true;
                }
            });
        }
        if delete_track {
            self.request_delete_tracks(&[track_id]);
        }
        if preview_dirty {
            self.editor.preview_dirty = true;
        }
        if track_mute_changed {
            self.refresh_audio_playback_items();
        }
    }
}

fn provider_choice_menu_row(ui: &mut Ui, provider: &ProviderEntry) -> egui::Response {
    let accent = provider_output_color(provider.output_type);
    let height = 30.0;
    let width = ui.available_width().max(210.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let response = crate::core::automation::instrument_response(
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
        "button",
        Some(provider.name.clone()),
        true,
        false,
    );

    let fill = if response.hovered() {
        kit::PANEL_RAISED
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(3), fill);

    let accent_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 2.0, rect.top() + 6.0),
        Vec2::new(3.0, rect.height() - 12.0),
    );
    ui.painter()
        .rect_filled(accent_rect, egui::CornerRadius::same(2), accent);

    let kind = provider.resolved_workflow_kind().short_label();
    let content = rect.shrink2(Vec2::new(12.0, 3.0));
    let kind_w = 46.0_f32.min(content.width() * 0.32);
    let name_w = (content.width() - kind_w - 8.0).max(24.0);
    paint_truncated_row_text_top(
        ui,
        Pos2::new(content.left(), content.center().y - 6.5),
        kit::value(&provider.name),
        12.0,
        name_w,
        kit::TEXT,
    );
    paint_truncated_row_text_top(
        ui,
        Pos2::new(content.right() - kind_w, content.center().y - 6.5),
        kit::caption(kind).color(accent.gamma_multiply(0.95)),
        11.0,
        kind_w,
        accent,
    );

    response
}
