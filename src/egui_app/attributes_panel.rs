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
        let header_generate_target = if self.editor.selection.clip_ids.len() == 1 {
            self.editor.selected_clip_id().and_then(|clip_id| {
                let asset_id = self
                    .editor
                    .project
                    .clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                    .map(|clip| clip.asset_id)?;
                generative_output_for_asset(&self.editor.project, asset_id)
                    .map(|_| (asset_id, Some(clip_id)))
            })
        } else if self.editor.selection.asset_ids.len() == 1 {
            self.editor.selected_asset_id().and_then(|asset_id| {
                generative_output_for_asset(&self.editor.project, asset_id)
                    .map(|_| (asset_id, None))
            })
        } else {
            None
        };

        let mut header_generate_clicked = false;
        ui.horizontal(|ui| {
            ui.label(kit::section_label("ATTRIBUTES"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if kit::icon_button(ui, "▶").clicked() {
                    self.editor.layout.right_collapsed = true;
                }
                if header_generate_target.is_some() {
                    if kit::primary_button_sized(ui, "Generate", 86.0, kit::ICON_BUTTON_H).clicked()
                    {
                        header_generate_clicked = true;
                    }
                }
            });
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

        if header_generate_clicked {
            if let Some((asset_id, context_clip_id)) = header_generate_target {
                self.start_generative_generation(asset_id, context_clip_id);
            }
        }
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
        let mut seam_bridge_provider_id: Option<Uuid> = None;
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
        let selected_video_clips: Vec<Clip> = clips
            .iter()
            .filter(|clip| {
                self.editor
                    .project
                    .find_asset(clip.asset_id)
                    .is_some_and(|asset| asset.is_video())
            })
            .cloned()
            .collect();
        if selected_video_clips.len() >= 2 {
            ui.add_space(kit::FORM_ROW_GAP);
            inspector_card(ui, "Seam Bridge", |ui| {
                ui.label(kit::caption(
                    "Create an anchored bridge overlay from the tail of the left video and head of the right video.",
                ));
                ui.add_space(kit::ACTION_GAP);
                ui.menu_button("Create Seam Bridge", |ui| {
                    if let Some(provider_id) = self.timeline_bridge_provider_menu(ui) {
                        seam_bridge_provider_id = Some(provider_id);
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
        if let Some(provider_id) = seam_bridge_provider_id {
            self.create_timeline_bridge_from_selected_clips(&selected_video_clips, provider_id);
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
            .filter(|provider| {
                provider.resolved_workflow_kind() == kind
                    && self.editor.provider_in_project_scope(provider.id)
            })
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

    pub(super) fn timeline_bridge_provider_menu(&self, ui: &mut Ui) -> Option<Uuid> {
        let providers: Vec<ProviderEntry> = self
            .editor
            .provider_entries
            .iter()
            .filter(|provider| {
                provider.output_type == ProviderOutputType::Video
                    && crate::core::timeline_bridge::provider_is_timeline_bridge(provider)
                    && self.editor.provider_in_project_scope(provider.id)
            })
            .cloned()
            .collect();
        if providers.is_empty() {
            ui.label(kit::caption("No timeline bridge providers configured."));
            return None;
        }
        for provider in providers {
            if provider_choice_menu_row(ui, &provider).clicked() {
                return Some(provider.id);
            }
        }
        None
    }

    fn validate_provider_override_in_project_scope(&mut self, provider_id: Option<Uuid>) -> bool {
        let Some(provider_id) = provider_id else {
            return true;
        };
        let Some(provider) = self
            .editor
            .provider_entries
            .iter()
            .find(|provider| provider.id == provider_id)
        else {
            self.editor.status = "Provider is unavailable.".to_string();
            return false;
        };
        if !self.editor.provider_in_project_scope(provider.id) {
            self.editor.status = "Provider is outside this project's provider scope.".to_string();
            return false;
        }
        true
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

    pub(super) fn create_timeline_bridge_from_selected_clips(
        &mut self,
        clips: &[Clip],
        provider_id: Uuid,
    ) {
        let Some(provider) = self
            .editor
            .provider_entries
            .iter()
            .find(|provider| provider.id == provider_id)
            .cloned()
        else {
            self.editor.status = "Bridge provider is unavailable.".to_string();
            return;
        };
        if !crate::core::timeline_bridge::provider_is_timeline_bridge(&provider)
            || provider.output_type != ProviderOutputType::Video
        {
            self.editor.status = "Provider is not a timeline bridge video provider.".to_string();
            return;
        }
        if !self.editor.provider_in_project_scope(provider.id) {
            self.editor.status = "Provider is outside this project's provider scope.".to_string();
            return;
        }
        let mut video_clips: Vec<Clip> = clips
            .iter()
            .filter(|clip| {
                self.editor
                    .project
                    .find_asset(clip.asset_id)
                    .is_some_and(|asset| asset.is_video())
            })
            .cloned()
            .collect();
        video_clips.sort_by(|a, b| {
            a.start_time
                .partial_cmp(&b.start_time)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        let (Some(left_clip), Some(right_clip)) = (video_clips.first(), video_clips.last()) else {
            self.editor.status = "Select left and right video clips first.".to_string();
            return;
        };
        if left_clip.id == right_clip.id {
            self.editor.status = "Select two different video clips.".to_string();
            return;
        }

        let asset_id = match self.editor.create_generative_video(
            default_generative_video_fps(),
            default_generative_video_frames(),
        ) {
            Ok(asset_id) => asset_id,
            Err(err) => {
                self.editor.status = err;
                return;
            }
        };

        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                *config = GenerativeConfig {
                    provider_id: Some(provider.id),
                    ..Default::default()
                };
            });
        self.seed_timeline_bridge_defaults(asset_id, &provider);
        let link = ClipBridgeLink::new(Some(left_clip.id), Some(right_clip.id));
        self.apply_timeline_bridge_media_inputs(asset_id, &provider, &link);

        let params = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(|config| {
                crate::core::timeline_bridge::timeline_bridge_parameters(&provider, config).ok()
            });
        let (fps, frame_count, start_time, duration) = if let Some(params) = params.as_ref() {
            (
                params.processing_fps,
                params.visible_frames().max(1),
                (left_clip.end_time() - params.left_seconds()).max(0.0),
                (right_clip.start_time + params.right_seconds()
                    - (left_clip.end_time() - params.left_seconds()).max(0.0))
                .max(0.1),
            )
        } else {
            (
                default_generative_video_fps(),
                default_generative_video_frames(),
                left_clip.end_time(),
                default_generative_video_frames() as f64 / default_generative_video_fps(),
            )
        };
        self.editor
            .project
            .set_generative_video_timing(asset_id, fps, frame_count);
        let target_track_id = self.bridge_target_track_id(left_clip.track_id);
        let mut clip = Clip::new(asset_id, target_track_id, start_time, duration);
        clip.label = Some("Seam bridge".to_string());
        clip.bridge = Some(link);
        let clip_id = self.editor.project.add_clip(clip);
        self.editor.sync_timeline_bridge_clip(clip_id);
        let config_save_error = self.editor.project.save_generative_config(asset_id).err();
        self.editor.selection.select_clip(clip_id);
        self.editor.preview_dirty = true;
        let resolution = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .and_then(|clip| self.timeline_bridge_resolution_for_clip(clip));
        self.editor.status = if let Some(err) = config_save_error {
            format!("Bridge created, but config save failed: {err}")
        } else if let Some(resolution) = resolution.filter(|resolution| !resolution.valid()) {
            resolution
                .tooltip()
                .map(|error| format!("Created timeline seam bridge; needs attention: {error}"))
                .unwrap_or_else(|| "Created timeline seam bridge; needs attention.".to_string())
        } else {
            "Created timeline seam bridge.".to_string()
        };
    }

    pub(super) fn create_i2v_from_single_clip(
        &mut self,
        clip_id: Uuid,
        reference: SingleI2VReference,
        provider_id: Option<Uuid>,
    ) {
        if !self.validate_provider_override_in_project_scope(provider_id) {
            return;
        }
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
        let target_track_id = if reference == SingleI2VReference::VideoLastFrame {
            source_clip.track_id
        } else {
            self.bridge_target_track_id(source_clip.track_id)
        };
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
        if !self.validate_provider_override_in_project_scope(provider_id) {
            return;
        }
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
        if !self.validate_provider_override_in_project_scope(provider_id) {
            return;
        }
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
        if !self.validate_provider_override_in_project_scope(provider_id) {
            return;
        }
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
            self.apply_continuation_source_dimensions(&mut config, source_asset_id);
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

        retain_literal_inputs(&mut config.inputs);
        if provider_override.is_some() {
            config.provider_id = provider_override;
        }
        self.retain_config_inputs_for_provider(&mut config);
        self.apply_continuation_source_dimensions(&mut config, source_asset_id);
        config
    }

    fn apply_continuation_source_dimensions(
        &self,
        config: &mut GenerativeConfig,
        source_asset_id: Uuid,
    ) {
        let Some(project_root) = self.editor.project.project_path.as_ref() else {
            return;
        };
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
        let Some(source_asset) = self.editor.project.find_asset(source_asset_id) else {
            return;
        };
        let Some(source_path) =
            self.resolve_asset_source_path_for_continuation(project_root, source_asset)
        else {
            return;
        };
        let Some((width, height)) = crate::core::media::probe_media_dimensions(&source_path) else {
            return;
        };
        let to_number = |value: u32| serde_json::Value::Number(value.into());

        for input in &provider.inputs {
            let Some(role) = input.role else {
                continue;
            };
            if !matches!(
                input.input_type,
                ProviderInputType::Number | ProviderInputType::Integer
            ) {
                continue;
            }
            let input_value = match role {
                crate::state::InputRole::Width => Some(to_number(width)),
                crate::state::InputRole::Height => Some(to_number(height)),
                crate::state::InputRole::Seed
                | crate::state::InputRole::DurationSeconds
                | crate::state::InputRole::Fps
                | crate::state::InputRole::FrameCount
                | crate::state::InputRole::LeftVideo
                | crate::state::InputRole::RightVideo
                | crate::state::InputRole::LeftReplaceFrames
                | crate::state::InputRole::RightReplaceFrames
                | crate::state::InputRole::EdgeBlendFrames => None,
            };
            if let Some(value) = input_value {
                config
                    .inputs
                    .insert(input.name.clone(), InputValue::Literal { value });
            }
        }
    }

    fn resolve_asset_source_path_for_continuation(
        &self,
        project_root: &std::path::Path,
        asset: &Asset,
    ) -> Option<std::path::PathBuf> {
        match &asset.kind {
            AssetKind::Image { path } | AssetKind::Video { path } => Some(project_root.join(path)),
            AssetKind::GenerativeImage {
                active_version,
                folder,
            }
            | AssetKind::GenerativeVideo {
                active_version,
                folder,
                ..
            } => {
                self.resolve_generative_source_path(project_root, folder, active_version.as_deref())
            }
            _ => None,
        }
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

    fn resolve_generative_source_path(
        &self,
        project_root: &std::path::Path,
        folder: &std::path::Path,
        active_version: Option<&str>,
    ) -> Option<std::path::PathBuf> {
        let active_version = active_version?;
        let folder_path = project_root.join(folder);
        for extension in ["png", "jpg", "jpeg", "webp", "mp4", "mov", "mkv", "webm"] {
            let candidate = folder_path.join(format!("{active_version}.{extension}"));
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
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

    pub(super) fn timeline_bridge_resolution_for_clip(
        &self,
        clip: &Clip,
    ) -> Option<crate::core::timeline_bridge::TimelineBridgeResolution> {
        let config = self.editor.project.generative_config(clip.asset_id)?;
        let provider = config.provider_id.and_then(|provider_id| {
            self.editor
                .provider_entries
                .iter()
                .find(|provider| provider.id == provider_id)
        })?;
        if !crate::core::timeline_bridge::provider_is_timeline_bridge(provider) {
            return None;
        }
        Some(crate::core::timeline_bridge::resolve_timeline_bridge_clip(
            &self.editor.project,
            Some(provider),
            Some(config),
            clip,
        ))
    }

    fn apply_timeline_bridge_provider_change(
        &mut self,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        next_provider: Option<&ProviderEntry>,
    ) {
        let Some(clip_id) = context_clip_id else {
            return;
        };
        let Some(clip_snapshot) = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
        else {
            return;
        };

        let Some(provider) = next_provider
            .filter(|provider| crate::core::timeline_bridge::provider_is_timeline_bridge(provider))
        else {
            if clip_snapshot.bridge.is_some() {
                self.editor.project.set_clip_bridge(clip_id, None);
                self.editor.status = "Unlinked bridge clip behavior.".to_string();
            }
            return;
        };

        self.seed_timeline_bridge_defaults(asset_id, provider);
        let existing_link = clip_snapshot.bridge.clone();
        let link = existing_link.unwrap_or_else(|| {
            crate::core::timeline_bridge::infer_timeline_bridge_link(
                &self.editor.project,
                &clip_snapshot,
            )
        });
        self.editor
            .project
            .set_clip_bridge(clip_id, Some(link.clone()));
        self.apply_timeline_bridge_media_inputs(asset_id, provider, &link);
        self.sync_timeline_bridge_asset_timing(asset_id, Some(clip_id));

        if let Some(left_clip_id) = link.left_clip_id {
            if let Some(left_track_id) = self
                .editor
                .project
                .clips
                .iter()
                .find(|clip| clip.id == left_clip_id)
                .map(|clip| clip.track_id)
            {
                let target_track_id = self.bridge_target_track_id(left_track_id);
                self.editor
                    .project
                    .move_clip_to_track(clip_id, target_track_id);
            }
        }
        self.editor.sync_timeline_bridge_clip(clip_id);
        let status = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .and_then(|clip| self.timeline_bridge_resolution_for_clip(clip));
        self.editor.status = match status {
            Some(resolution) if resolution.valid() => "Linked timeline bridge clip.".to_string(),
            Some(resolution) => resolution
                .tooltip()
                .map(|error| format!("Bridge needs attention: {error}"))
                .unwrap_or_else(|| "Bridge needs attention.".to_string()),
            None => "Bridge provider selected; link source clips before generating.".to_string(),
        };
    }

    fn seed_timeline_bridge_defaults(&mut self, asset_id: Uuid, provider: &ProviderEntry) {
        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                for input in provider.inputs.iter() {
                    if !matches!(
                        input.role,
                        Some(
                            InputRole::Fps
                                | InputRole::LeftReplaceFrames
                                | InputRole::RightReplaceFrames
                                | InputRole::EdgeBlendFrames
                        )
                    ) {
                        continue;
                    }
                    if config.inputs.contains_key(&input.name) {
                        continue;
                    }
                    if let Some(value) = input.default.clone() {
                        config
                            .inputs
                            .insert(input.name.clone(), InputValue::Literal { value });
                    }
                }
            });
    }

    fn apply_timeline_bridge_media_inputs(
        &mut self,
        asset_id: Uuid,
        provider: &ProviderEntry,
        link: &ClipBridgeLink,
    ) {
        let Ok(fields) = crate::core::timeline_bridge::timeline_bridge_fields(provider) else {
            return;
        };
        let left_value = link.left_clip_id.and_then(|clip_id| {
            self.editor
                .project
                .clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .map(|clip| InputValue::AssetRef {
                    asset_id: clip.asset_id,
                    source_clip_id: Some(clip.id),
                    pinned: true,
                    frame_reference: None,
                })
        });
        let right_value = link.right_clip_id.and_then(|clip_id| {
            self.editor
                .project
                .clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .map(|clip| InputValue::AssetRef {
                    asset_id: clip.asset_id,
                    source_clip_id: Some(clip.id),
                    pinned: true,
                    frame_reference: None,
                })
        });
        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                if let Some(value) = left_value.clone() {
                    config
                        .inputs
                        .insert(fields.left_video.clone(), value.clone());
                    config
                        .reference_slots
                        .insert("left_video".to_string(), value);
                }
                if let Some(value) = right_value.clone() {
                    config
                        .inputs
                        .insert(fields.right_video.clone(), value.clone());
                    config
                        .reference_slots
                        .insert("right_video".to_string(), value);
                }
            });
    }

    pub(super) fn clip_attributes(&mut self, ui: &mut Ui, clip_id: Uuid) {
        let Some(clip_snapshot) = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
        else {
            return;
        };
        let clip_asset_id = Some(clip_snapshot.asset_id);
        let clip_asset = self
            .editor
            .project
            .find_asset(clip_snapshot.asset_id)
            .cloned();
        let asset_name = clip_asset
            .as_ref()
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| "Unknown asset".to_string());
        let clip_asset_is_image = clip_asset.as_ref().is_some_and(|asset| asset.is_image());
        let clip_asset_is_video = clip_asset.as_ref().is_some_and(|asset| asset.is_video());
        let clip_asset_is_time_based = clip_asset
            .as_ref()
            .is_some_and(|asset| asset.is_video() || asset.is_audio());

        let mut next_label = clip_snapshot.label.clone().unwrap_or_default();
        let mut next_image_mode = clip_snapshot.image_mode;
        let mut next_time_mode = clip_snapshot.time_mode;
        let mut next_transform = clip_snapshot.transform;
        let mut next_start = clip_snapshot.start_time;
        let mut next_duration = clip_snapshot.duration;
        let mut next_trim = clip_snapshot.trim_in_seconds;
        let mut label_changed = false;
        let mut transform_changed = false;
        let mut timing_changed = false;
        let mut trim_changed = false;

        inspector_card(ui, "Clip", |ui| {
            kit::field_label(ui, "Source Asset");
            let source_w = ui.available_width();
            kit::readonly_value_box(ui, asset_name, Vec2::new(source_w, kit::FIELD_H));
            ui.add_space(kit::FORM_ROW_GAP);
            label_changed = inspector_text_field(ui, "Clip Label", &mut next_label);
            if clip_asset_is_image {
                ui.add_space(kit::FORM_ROW_GAP);
                kit::labeled_combo_field(
                    ui,
                    "Timeline Display",
                    ("clip_image_mode", clip_id),
                    clip_image_mode_label(next_image_mode),
                    |ui| {
                        automation_selectable_value(
                            ui,
                            &mut next_image_mode,
                            ClipImageMode::Still,
                            "Still Image",
                        );
                        automation_selectable_value(
                            ui,
                            &mut next_image_mode,
                            ClipImageMode::Keyframe,
                            "Keyframe Reference",
                        );
                    },
                );
            }
            if clip_asset_is_video {
                ui.add_space(kit::FORM_ROW_GAP);
                kit::labeled_combo_field(
                    ui,
                    "Time Mapping",
                    ("clip_time_mode", clip_id),
                    clip_time_mode_label(next_time_mode),
                    |ui| {
                        automation_selectable_value(
                            ui,
                            &mut next_time_mode,
                            ClipTimeMode::Crop,
                            "Crop",
                        );
                        automation_selectable_value(
                            ui,
                            &mut next_time_mode,
                            ClipTimeMode::Stretch,
                            "Stretch",
                        );
                    },
                );
            }
        });
        ui.add_space(kit::FORM_ROW_GAP);
        inspector_card(ui, "Transform", |ui| {
            transform_editor(ui, &mut next_transform, &mut transform_changed);
        });
        ui.add_space(kit::FORM_ROW_GAP);
        inspector_card(ui, "Timing", |ui| {
            if clip_asset_is_image && next_image_mode == ClipImageMode::Keyframe {
                timing_changed |=
                    inspector_drag_f64(ui, "Time", &mut next_start, 0.05, ui.available_width());
            } else {
                let before_start = next_start;
                let before_duration = next_duration;
                inspector_two_drag_f64(
                    ui,
                    ("Start", &mut next_start, 0.05),
                    ("Duration", &mut next_duration, 0.05),
                );
                timing_changed |= (next_start - before_start).abs() > f64::EPSILON
                    || (next_duration - before_duration).abs() > f64::EPSILON;
                if clip_asset_is_time_based {
                    ui.add_space(kit::FORM_ROW_GAP);
                    trim_changed |= inspector_drag_f64(
                        ui,
                        "Trim In",
                        &mut next_trim,
                        0.05,
                        ui.available_width(),
                    );
                }
            }
        });
        if clip_snapshot.bridge.is_some()
            || self
                .timeline_bridge_resolution_for_clip(&clip_snapshot)
                .is_some()
        {
            ui.add_space(kit::FORM_ROW_GAP);
            self.timeline_bridge_clip_card(ui, clip_id, &clip_snapshot);
        }

        let mut preview_dirty = false;
        if label_changed {
            let next_label = if next_label.trim().is_empty() {
                None
            } else {
                Some(next_label.trim().to_string())
            };
            if next_label != clip_snapshot.label {
                self.editor.project.set_clip_label(clip_id, next_label);
            }
        }
        if next_image_mode != clip_snapshot.image_mode {
            self.editor
                .project
                .set_clip_image_mode(clip_id, next_image_mode);
            preview_dirty = true;
        }
        if next_time_mode != clip_snapshot.time_mode {
            self.editor
                .project
                .set_clip_time_mode(clip_id, next_time_mode);
            preview_dirty = true;
        }
        if transform_changed && next_transform != clip_snapshot.transform {
            self.editor
                .project
                .set_clip_transform(clip_id, next_transform);
            preview_dirty = true;
        }
        if trim_changed && (next_trim - clip_snapshot.trim_in_seconds).abs() > f64::EPSILON {
            self.editor
                .project
                .set_clip_trim_in_seconds(clip_id, next_trim);
            preview_dirty = true;
        }
        if timing_changed {
            if clip_asset_is_image && next_image_mode == ClipImageMode::Keyframe {
                self.editor.project.move_clip(clip_id, next_start);
            } else {
                if (next_start - clip_snapshot.start_time).abs() > f64::EPSILON {
                    self.editor.project.move_clip(clip_id, next_start);
                }
                self.editor
                    .project
                    .resize_clip(clip_id, next_start, next_duration);
                if let Some(asset) = clip_asset.as_ref() {
                    if matches!(asset.kind, AssetKind::GenerativeVideo { .. }) {
                        self.sync_hollow_generative_video_timing_from_clip(
                            clip_id,
                            asset.id,
                            next_duration,
                        );
                    }
                }
            }
            preview_dirty = true;
        }
        if preview_dirty {
            self.editor.sync_timeline_bridge_clips();
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

    fn timeline_bridge_clip_card(&mut self, ui: &mut Ui, clip_id: Uuid, clip: &Clip) {
        let mut relink_from_selection = false;
        let mut unlink = false;
        inspector_card(ui, "Timeline Bridge", |ui| {
            let resolution = self.timeline_bridge_resolution_for_clip(clip);
            if let Some(resolution) = resolution.as_ref() {
                if resolution.valid() {
                    ui.label(kit::caption(
                        "Linked and ready. Regenerate after timing or source changes.",
                    ));
                } else {
                    ui.label(
                        RichText::new("Needs attention")
                            .color(kit::MARKER)
                            .size(12.0),
                    );
                    for error in resolution.errors.iter() {
                        ui.label(RichText::new(error).color(kit::TEXT_DIM).size(11.0));
                    }
                }
                if let Some(params) = resolution.parameters.as_ref() {
                    ui.add_space(kit::FORM_ROW_GAP);
                    inspector_meta_row(ui, "FPS", format!("{:.3}", params.processing_fps));
                    inspector_meta_row(
                        ui,
                        "Frames",
                        format!(
                            "{} left + {} right",
                            params.left_replace_frames, params.right_replace_frames
                        ),
                    );
                    inspector_meta_row(
                        ui,
                        "Edge Blend",
                        format!("{} frames", params.edge_blend_frames),
                    );
                }
            } else {
                ui.label(
                    RichText::new("Not linked to a bridge provider.")
                        .color(kit::TEXT_DIM)
                        .size(11.0),
                );
            }
            ui.add_space(kit::ACTION_GAP);
            kit::equal_width_action_row(
                ui,
                2,
                kit::SECONDARY_BUTTON_H,
                kit::FIELD_COMPOUND_GAP,
                |ui, index, width| match index {
                    0 => {
                        if kit::secondary_button(ui, "Relink Selection", width).clicked() {
                            relink_from_selection = true;
                        }
                    }
                    _ => {
                        if kit::secondary_button(ui, "Unlink", width).clicked() {
                            unlink = true;
                        }
                    }
                },
            );
        });
        if relink_from_selection {
            let selected_sources: Vec<Clip> = self
                .editor
                .selection
                .clip_ids
                .iter()
                .filter(|id| **id != clip_id)
                .filter_map(|id| {
                    self.editor
                        .project
                        .clips
                        .iter()
                        .find(|clip| clip.id == *id)
                        .cloned()
                })
                .collect();
            if selected_sources.len() >= 2 {
                let link = self.bridge_reference_video_link(&selected_sources);
                self.editor
                    .project
                    .set_clip_bridge(clip_id, Some(link.clone()));
                if let Some(provider) = self
                    .editor
                    .project
                    .generative_config(clip.asset_id)
                    .and_then(|config| config.provider_id)
                    .and_then(|provider_id| {
                        self.editor
                            .provider_entries
                            .iter()
                            .find(|provider| provider.id == provider_id)
                            .cloned()
                    })
                {
                    self.apply_timeline_bridge_media_inputs(clip.asset_id, &provider, &link);
                }
                self.editor.sync_timeline_bridge_clip(clip_id);
                self.editor.preview_dirty = true;
            } else {
                self.editor.status =
                    "Select the bridge clip plus left and right source video clips.".to_string();
            }
        }
        if unlink {
            self.editor.project.set_clip_bridge(clip_id, None);
            self.editor.preview_dirty = true;
            self.editor.status = "Unlinked bridge clip behavior.".to_string();
        }
    }

    fn bridge_reference_video_link(&self, clips: &[Clip]) -> ClipBridgeLink {
        let mut video_clips: Vec<Clip> = clips
            .iter()
            .filter(|clip| {
                self.editor
                    .project
                    .find_asset(clip.asset_id)
                    .is_some_and(|asset| asset.is_video())
            })
            .cloned()
            .collect();
        video_clips.sort_by(|a, b| {
            a.start_time
                .partial_cmp(&b.start_time)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        ClipBridgeLink::new(
            video_clips.first().map(|clip| clip.id),
            video_clips.last().map(|clip| clip.id),
        )
    }

    pub(super) fn set_timeline_bridge_edge_time(
        &mut self,
        clip_id: Uuid,
        left_edge: bool,
        edge_time: f64,
    ) -> bool {
        let Some(clip_snapshot) = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
        else {
            return false;
        };
        let Some(config_snapshot) = self
            .editor
            .project
            .generative_config(clip_snapshot.asset_id)
            .cloned()
        else {
            return false;
        };
        let Some(provider) = config_snapshot.provider_id.and_then(|provider_id| {
            self.editor
                .provider_entries
                .iter()
                .find(|provider| provider.id == provider_id)
                .cloned()
        }) else {
            return false;
        };
        if !crate::core::timeline_bridge::provider_is_timeline_bridge(&provider) {
            return false;
        }
        let Ok(fields) = crate::core::timeline_bridge::timeline_bridge_fields(&provider) else {
            return false;
        };
        let Ok(params) =
            crate::core::timeline_bridge::timeline_bridge_parameters(&provider, &config_snapshot)
        else {
            return false;
        };
        let Some(link) = clip_snapshot.bridge.as_ref() else {
            return false;
        };
        let (input_name, source_time) = if left_edge {
            let Some(left_clip) = link
                .left_clip_id
                .and_then(|id| self.editor.project.clips.iter().find(|clip| clip.id == id))
            else {
                return false;
            };
            (
                &fields.left_replace_frames,
                left_clip.end_time() - edge_time,
            )
        } else {
            let Some(right_clip) = link
                .right_clip_id
                .and_then(|id| self.editor.project.clips.iter().find(|clip| clip.id == id))
            else {
                return false;
            };
            (
                &fields.right_replace_frames,
                edge_time - right_clip.start_time,
            )
        };
        let Some(input) = provider
            .inputs
            .iter()
            .find(|input| input.name == input_name.as_str())
        else {
            return false;
        };
        let mut frames = (source_time.max(0.0) * params.processing_fps)
            .round()
            .max(0.0);
        if let Some(step) = input
            .ui
            .as_ref()
            .and_then(|ui| ui.step)
            .filter(|step| *step > 0.0)
        {
            frames = (frames / step).round() * step;
        }
        if let Some(min) = input.ui.as_ref().and_then(|ui| ui.min) {
            frames = frames.max(min);
        }
        if let Some(max) = input.ui.as_ref().and_then(|ui| ui.max) {
            frames = frames.min(max);
        }
        let value = if matches!(input.input_type, ProviderInputType::Integer) {
            serde_json::Value::Number((frames.round() as i64).into())
        } else {
            let Some(number) = serde_json::Number::from_f64(frames) else {
                return false;
            };
            serde_json::Value::Number(number)
        };
        self.editor
            .project
            .update_generative_config(clip_snapshot.asset_id, |config| {
                config
                    .inputs
                    .insert(input_name.clone(), InputValue::Literal { value });
            });
        if let Err(err) = self
            .editor
            .project
            .save_generative_config(clip_snapshot.asset_id)
        {
            self.editor.status = format!("Failed to save bridge timing: {err}");
        }
        self.sync_timeline_bridge_asset_timing(clip_snapshot.asset_id, Some(clip_id));
        self.editor.sync_timeline_bridge_clip(clip_id);
        self.editor.preview_dirty = true;
        true
    }

    pub(super) fn generative_asset_attributes(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
    ) {
        let Some((_, output_type)) = generative_output_for_asset(&self.editor.project, asset_id)
        else {
            return;
        };
        let config_snapshot = self
            .editor
            .project
            .generative_config(asset_id)
            .cloned()
            .unwrap_or_default();
        let compatible_providers: Vec<ProviderEntry> = self
            .editor
            .provider_entries
            .iter()
            .filter(|entry| {
                entry.output_type == output_type && self.editor.provider_in_project_scope(entry.id)
            })
            .cloned()
            .collect();
        let selected_provider_id = config_snapshot.provider_id;
        let selected_provider = selected_provider_id.and_then(|id| {
            self.editor
                .provider_entries
                .iter()
                .find(|entry| entry.id == id && entry.output_type == output_type)
                .cloned()
        });
        let show_missing_provider = selected_provider_id.is_some() && selected_provider.is_none();
        let selected_provider_out_of_scope = selected_provider
            .as_ref()
            .is_some_and(|provider| !self.editor.provider_in_project_scope(provider.id));

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
        let resolved_seed_field = selected_provider
            .as_ref()
            .and_then(|provider| resolve_seed_field(provider));
        let batch_hint = if batch.count > 1 {
            match batch.seed_strategy {
                SeedStrategy::Keep => {
                    Some("Identical inputs can be cached; use Increment or Random.".to_string())
                }
                _ if resolved_seed_field.is_none() => {
                    Some("No seed role detected. Assign a numeric input role in Provider Builder before generating batches.".to_string())
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
            } else if selected_provider_out_of_scope {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(
                    RichText::new("Selected provider is outside this project's provider scope.")
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

        if output_type == ProviderOutputType::Video
            && selected_provider.as_ref().is_none_or(|provider| {
                !crate::core::timeline_bridge::provider_is_timeline_bridge(provider)
            })
        {
            ui.add_space(kit::FORM_ROW_GAP);
            self.generative_video_timing_card(
                ui,
                asset_id,
                context_clip_id,
                selected_provider.as_ref(),
            );
        }

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
            draw_strategy(ui);
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
            let next_provider = next_provider_id.and_then(|provider_id| {
                compatible_providers
                    .iter()
                    .find(|provider| provider.id == provider_id)
            });
            self.editor
                .project
                .set_generative_provider_id(asset_id, next_provider_id);
            self.apply_timeline_bridge_provider_change(asset_id, context_clip_id, next_provider);
            config_dirty = true;
        }
        let clamped_batch_count =
            next_batch_count.clamp(1, MAX_GENERATION_BATCH_COUNT as i64) as u32;
        if clamped_batch_count != batch.count || next_seed_strategy != batch.seed_strategy {
            self.editor
                .project
                .update_generative_config(asset_id, |config| {
                    config.batch.count = clamped_batch_count;
                    config.batch.seed_strategy = next_seed_strategy;
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
            if selected_provider.as_ref().is_some_and(|provider| {
                crate::core::timeline_bridge::provider_is_timeline_bridge(provider)
            }) {
                self.sync_timeline_bridge_asset_timing(asset_id, context_clip_id);
            }
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
            self.start_generative_generation(asset_id, context_clip_id);
        }
    }

    fn generative_video_timing_card(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        selected_provider: Option<&ProviderEntry>,
    ) {
        let Some((duration, fps, frame_count)) =
            generative_video_timing(&self.editor.project, asset_id)
        else {
            return;
        };
        let bounds = provider_duration_bounds(selected_provider);
        let mut next_duration = duration;
        let mut next_fps = fps;
        let mut next_frame_count = frame_count as i64;
        let mut duration_changed = false;
        let mut fps_changed = false;
        let mut frames_changed = false;

        inspector_card(ui, "Target Timing", |ui| {
            duration_changed |= inspector_drag_f64(
                ui,
                "Seconds",
                &mut next_duration,
                0.05,
                ui.available_width(),
            );
            ui.add_space(kit::FORM_ROW_GAP);
            fps_changed |= inspector_drag_f64(ui, "FPS", &mut next_fps, 1.0, ui.available_width());
            ui.add_space(kit::FORM_ROW_GAP);
            frames_changed |= inspector_drag_i64(
                ui,
                "Frames",
                &mut next_frame_count,
                1.0,
                ui.available_width(),
            );
            if bounds.min.is_some() || bounds.max.is_some() {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(provider_duration_bounds_label(bounds)));
            }
        });

        if !(duration_changed || fps_changed || frames_changed) {
            return;
        }

        next_fps = next_fps.clamp(1.0, 240.0);
        if fps_changed {
            next_frame_count = (next_duration.max(1.0 / next_fps) * next_fps)
                .round()
                .max(1.0) as i64;
        }
        if duration_changed {
            next_duration = clamp_provider_duration(next_duration, bounds);
            next_frame_count = (next_duration.max(1.0 / next_fps) * next_fps)
                .round()
                .max(1.0) as i64;
        }
        if frames_changed {
            next_frame_count = next_frame_count.clamp(1, 1_000_000);
            next_duration = next_frame_count as f64 / next_fps;
            let clamped = clamp_provider_duration(next_duration, bounds);
            if (clamped - next_duration).abs() > f64::EPSILON {
                next_duration = clamped;
                next_frame_count = (next_duration * next_fps).round().max(1.0) as i64;
            }
        }

        let next_frame_count = next_frame_count.clamp(1, 1_000_000) as u32;
        if self
            .editor
            .project
            .set_generative_video_timing(asset_id, next_fps, next_frame_count)
        {
            self.sync_generative_video_timing_inputs(asset_id);
            self.sync_single_hollow_generative_video_clip(asset_id, context_clip_id);
            self.editor.preview_dirty = true;
        }
    }

    fn sync_hollow_generative_video_timing_from_clip(
        &mut self,
        clip_id: Uuid,
        asset_id: Uuid,
        clip_duration: f64,
    ) {
        if !self.hollow_generative_video_single_clip(asset_id, Some(clip_id)) {
            return;
        }
        let Some((_, fps, _)) = generative_video_timing(&self.editor.project, asset_id) else {
            return;
        };
        let provider = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(|config| config.provider_id)
            .and_then(|provider_id| {
                self.editor
                    .provider_entries
                    .iter()
                    .find(|provider| provider.id == provider_id)
            });
        let target_duration =
            clamp_provider_duration(clip_duration, provider_duration_bounds(provider));
        let frame_count = (target_duration.max(1.0 / fps) * fps).round().max(1.0) as u32;
        if self
            .editor
            .project
            .set_generative_video_timing(asset_id, fps, frame_count)
        {
            if (target_duration - clip_duration).abs() > f64::EPSILON {
                if let Some(clip) = self
                    .editor
                    .project
                    .clips
                    .iter_mut()
                    .find(|clip| clip.id == clip_id)
                {
                    clip.duration = target_duration.max(0.1);
                }
            }
            self.sync_generative_video_timing_inputs(asset_id);
        }
    }

    fn sync_single_hollow_generative_video_clip(
        &mut self,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
    ) {
        if !self.hollow_generative_video_single_clip(asset_id, context_clip_id) {
            return;
        }
        let Some((duration, _, _)) = generative_video_timing(&self.editor.project, asset_id) else {
            return;
        };
        let Some(clip_id) = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.asset_id == asset_id)
            .map(|clip| clip.id)
        else {
            return;
        };
        if let Some(clip) = self
            .editor
            .project
            .clips
            .iter_mut()
            .find(|clip| clip.id == clip_id)
        {
            clip.duration = duration.max(0.1);
        }
    }

    fn hollow_generative_video_single_clip(
        &self,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
    ) -> bool {
        let Some(asset) = self.editor.project.find_asset(asset_id) else {
            return false;
        };
        if !matches!(asset.kind, AssetKind::GenerativeVideo { .. }) {
            return false;
        }
        if self
            .editor
            .project
            .generative_config(asset_id)
            .is_some_and(|config| config.active_version.is_some() || !config.versions.is_empty())
        {
            return false;
        }
        let clip_ids: Vec<Uuid> = self
            .editor
            .project
            .clips
            .iter()
            .filter(|clip| clip.asset_id == asset_id)
            .map(|clip| clip.id)
            .collect();
        if clip_ids.len() != 1 {
            return false;
        }
        context_clip_id.is_none_or(|clip_id| clip_ids[0] == clip_id)
    }

    fn sync_generative_video_timing_inputs(&mut self, asset_id: Uuid) -> bool {
        let Some((duration, fps, frame_count)) =
            generative_video_timing(&self.editor.project, asset_id)
        else {
            return false;
        };
        let Some(provider_id) = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(|config| config.provider_id)
        else {
            return false;
        };
        let Some(provider) = self
            .editor
            .provider_entries
            .iter()
            .find(|provider| provider.id == provider_id)
            .cloned()
        else {
            return false;
        };

        let mut changed = false;
        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                for input in provider.inputs.iter() {
                    let Some(value) = provider_timing_role_value(input, duration, fps, frame_count)
                    else {
                        continue;
                    };
                    let next = InputValue::Literal { value };
                    if config.inputs.get(&input.name) != Some(&next) {
                        config.inputs.insert(input.name.clone(), next);
                        changed = true;
                    }
                }
            });
        if changed {
            if let Err(err) = self.editor.project.save_generative_config(asset_id) {
                self.editor.status = format!("Failed to save generative timing inputs: {err}");
            }
        }
        changed
    }

    fn sync_timeline_bridge_asset_timing(
        &mut self,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
    ) -> bool {
        let Some(config) = self.editor.project.generative_config(asset_id).cloned() else {
            return false;
        };
        let Some(provider) = config.provider_id.and_then(|provider_id| {
            self.editor
                .provider_entries
                .iter()
                .find(|provider| provider.id == provider_id)
                .cloned()
        }) else {
            return false;
        };
        if !crate::core::timeline_bridge::provider_is_timeline_bridge(&provider) {
            return false;
        }
        let Ok(params) =
            crate::core::timeline_bridge::timeline_bridge_parameters(&provider, &config)
        else {
            return false;
        };
        let mut changed = self.editor.project.set_generative_video_timing(
            asset_id,
            params.processing_fps,
            params.visible_frames().max(1),
        );
        if let Some(clip_id) = context_clip_id.or_else(|| {
            self.editor
                .project
                .clips
                .iter()
                .find(|clip| clip.asset_id == asset_id)
                .map(|clip| clip.id)
        }) {
            changed |= self.editor.sync_timeline_bridge_clip(clip_id);
        }
        changed
    }

    pub(super) fn start_generative_generation(
        &mut self,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
    ) {
        let Some((folder, output_type)) =
            generative_output_for_asset(&self.editor.project, asset_id)
        else {
            self.editor.status = "Selected asset is no longer generative.".to_string();
            return;
        };
        if output_type == ProviderOutputType::Video {
            self.sync_generative_video_timing_inputs(asset_id);
        }
        let config_for_generation = self
            .editor
            .project
            .generative_config(asset_id)
            .cloned()
            .unwrap_or_default();
        let Some(provider_id) = config_for_generation.provider_id else {
            self.editor.status = "Select a provider first.".to_string();
            return;
        };
        let Some(provider) = self
            .editor
            .provider_entries
            .iter()
            .find(|provider| provider.id == provider_id && provider.output_type == output_type)
            .cloned()
        else {
            self.editor.status = "Selected provider is unavailable.".to_string();
            return;
        };
        if !self.editor.provider_in_project_scope(provider.id) {
            self.editor.status =
                "Selected provider is outside this project's provider scope.".to_string();
            return;
        }
        let Some(folder_path) = self
            .editor
            .project
            .project_path
            .as_ref()
            .map(|root| root.join(&folder))
        else {
            self.editor.status = "Project folder is unavailable.".to_string();
            return;
        };
        let asset_label = self
            .editor
            .project
            .find_asset(asset_id)
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| "Generative Asset".to_string());

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
            Err(err) => {
                self.editor.status = err;
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

            let mut visible_index = 0usize;
            for input in provider.inputs.iter() {
                if is_timing_role(input.role)
                    && !(crate::core::timeline_bridge::provider_is_timeline_bridge(&provider)
                        && input.role == Some(InputRole::Fps))
                {
                    continue;
                }
                if visible_index > 0 {
                    ui.add_space(kit::FORM_ROW_GAP);
                }
                visible_index += 1;
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
                            provider_input_multiline_text_field(
                                ui,
                                &label,
                                input,
                                &mut value,
                                kit::MultilineTextFieldOptions::rows(3),
                            )
                        } else {
                            provider_input_text_field(ui, &label, input, &mut value)
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
                        if provider_input_drag_f64(ui, &label, input, &mut value, step, width) {
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
                        if provider_input_drag_i64(ui, &label, input, &mut value, step, width) {
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
                        if provider_input_bool_field(ui, &label, input, &mut value) {
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
                        provider_input_labeled_combo_field(
                            ui,
                            &label,
                            input,
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

        provider_input_labeled_combo_field(
            ui,
            &input.label,
            input,
            combo_id,
            current_label,
            |ui| {
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
            },
        );

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
            if !asset_source_available_for_provider_input(
                &self.editor.project,
                asset,
                &input.input_type,
            ) {
                continue;
            }
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
                || !asset_source_available_for_provider_input(
                    &self.editor.project,
                    asset,
                    &input.input_type,
                )
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

fn clip_time_mode_label(mode: ClipTimeMode) -> &'static str {
    match mode {
        ClipTimeMode::Crop => "Crop",
        ClipTimeMode::Stretch => "Stretch",
    }
}

fn generative_video_timing(project: &Project, asset_id: Uuid) -> Option<(f64, f64, u32)> {
    let asset = project.find_asset(asset_id)?;
    let AssetKind::GenerativeVideo {
        fps, frame_count, ..
    } = &asset.kind
    else {
        return None;
    };
    let fps = (*fps).max(1.0);
    let frame_count = (*frame_count).max(1);
    let duration = asset
        .duration_seconds
        .filter(|duration| *duration > 0.0)
        .unwrap_or(frame_count as f64 / fps);
    Some((duration, fps, frame_count))
}

#[derive(Clone, Copy)]
struct ProviderDurationBounds {
    min: Option<f64>,
    max: Option<f64>,
}

fn provider_duration_bounds(provider: Option<&ProviderEntry>) -> ProviderDurationBounds {
    let Some(input) = provider.and_then(|provider| {
        provider
            .inputs
            .iter()
            .find(|input| input.role == Some(InputRole::DurationSeconds))
    }) else {
        return ProviderDurationBounds {
            min: None,
            max: None,
        };
    };
    ProviderDurationBounds {
        min: input.ui.as_ref().and_then(|ui| ui.min),
        max: input.ui.as_ref().and_then(|ui| ui.max),
    }
}

fn clamp_provider_duration(duration: f64, bounds: ProviderDurationBounds) -> f64 {
    let mut duration = duration.max(0.001);
    if let Some(min) = bounds.min {
        duration = duration.max(min);
    }
    if let Some(max) = bounds.max {
        duration = duration.min(max);
    }
    duration
}

fn provider_duration_bounds_label(bounds: ProviderDurationBounds) -> String {
    match (bounds.min, bounds.max) {
        (Some(min), Some(max)) => format!(
            "Provider duration range {} - {}",
            format_duration(min),
            format_duration(max)
        ),
        (Some(min), None) => format!("Provider minimum duration {}", format_duration(min)),
        (None, Some(max)) => format!("Provider maximum duration {}", format_duration(max)),
        (None, None) => String::new(),
    }
}

fn provider_timing_role_value(
    input: &ProviderInputField,
    duration: f64,
    fps: f64,
    frame_count: u32,
) -> Option<serde_json::Value> {
    let role = input.role?;
    let raw = match role {
        InputRole::DurationSeconds => duration,
        InputRole::Fps => fps,
        InputRole::FrameCount => frame_count as f64,
        InputRole::Width
        | InputRole::Height
        | InputRole::Seed
        | InputRole::LeftVideo
        | InputRole::RightVideo
        | InputRole::LeftReplaceFrames
        | InputRole::RightReplaceFrames
        | InputRole::EdgeBlendFrames => return None,
    };
    let raw = clamp_provider_input_number(raw, input);
    match input.input_type {
        ProviderInputType::Integer => Some(serde_json::Value::Number((raw.round() as i64).into())),
        ProviderInputType::Number => {
            serde_json::Number::from_f64(raw).map(serde_json::Value::Number)
        }
        _ => None,
    }
}

fn is_timing_role(role: Option<InputRole>) -> bool {
    matches!(
        role,
        Some(InputRole::DurationSeconds | InputRole::Fps | InputRole::FrameCount)
    )
}

fn clamp_provider_input_number(value: f64, input: &ProviderInputField) -> f64 {
    let mut value = value;
    if let Some(min) = input.ui.as_ref().and_then(|ui| ui.min) {
        value = value.max(min);
    }
    if let Some(max) = input.ui.as_ref().and_then(|ui| ui.max) {
        value = value.min(max);
    }
    value
}

fn retain_literal_inputs(inputs: &mut HashMap<String, InputValue>) {
    inputs.retain(|_, value| matches!(value, InputValue::Literal { .. }));
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

    if let Some(description) = provider
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty())
    {
        response.on_hover_text(description)
    } else {
        response
    }
}
