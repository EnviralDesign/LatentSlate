use super::*;
use crate::core::media_binding::{
    bound_media_type_for_input, next_media_reference_slots, visible_media_reference_fields,
    MediaReferenceVisibility,
};
use crate::state::{
    generation_record_source_inputs, input_value_as_i64, AssetKind, InputRole, InputValue,
    ProviderInputType, SeedStrategy,
};

const HEADER_THUMB: Vec2 = Vec2::new(52.0, 40.0);

impl LatentSlateApp {
    pub(super) fn generated_inspector_header(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
    ) {
        let Some(asset) = self
            .editor
            .project
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .cloned()
        else {
            return;
        };
        let thumbnail = self.asset_thumbnail(ui.ctx(), &asset);
        let accent = asset_accent(&asset);
        let kind_label = asset_kind_label(&asset.kind);
        let config = self
            .editor
            .project
            .generative_config(asset_id)
            .cloned()
            .unwrap_or_default();
        let probed = self.asset_source_dimensions(&asset);
        let probed_fps = match asset.kind {
            AssetKind::GenerativeVideo { .. } | AssetKind::Video { .. } => {
                self.asset_source_fps(&asset)
            }
            _ => None,
        };
        let facts = existing_output_facts(
            &self.editor.project,
            &self.editor.provider_entries,
            &asset,
            &config,
            probed,
            probed_fps,
        );
        let mut version_options: Vec<String> = config
            .versions
            .iter()
            .map(|record| record.version.clone())
            .collect();
        if let Some(active) = config.active_version.as_ref() {
            if !active.trim().is_empty() && !version_options.contains(active) {
                version_options.push(active.clone());
            }
        }
        version_options.sort_by(|a, b| match (parse_version_index(a), parse_version_index(b)) {
            (Some(a_num), Some(b_num)) => b_num.cmp(&a_num),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b.cmp(a),
        });
        version_options.dedup();
        let mut next_version = config.active_version.clone().unwrap_or_default();
        let details_id = ui.make_persistent_id(("attr_details_open", asset_id));
        let mut details_open = ui
            .data(|data| data.get_temp::<bool>(details_id))
            .unwrap_or(false);

        kit::bounded_horizontal_row(ui, HEADER_THUMB.y.max(kit::FIELD_H + 18.0), |ui, row_w| {
            let (thumb_rect, _) = ui.allocate_exact_size(HEADER_THUMB, Sense::hover());
            paint_asset_thumbnail(ui, thumb_rect, &asset, accent, thumbnail);
            ui.add_space(8.0);
            let pin_leading = 14.0;
            let combo_w = kit::combo_field_width_for_text(ui, "V123", pin_leading, 0.0);
            let name_w =
                (row_w - HEADER_THUMB.x - 8.0 - combo_w - ui.spacing().item_spacing.x).max(80.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.set_width(name_w);
                let mut name = asset.name.clone();
                if kit::singleline_text_field(ui, &mut name, name_w).changed() {
                    if let Err(err) = self.editor.rename_asset(asset_id, name) {
                        self.editor.status = err;
                    }
                }
                ui.label(kit::caption(kind_label));
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                let selected_text = if next_version.trim().is_empty() {
                    "—".to_string()
                } else {
                    next_version.clone()
                };
                kit::combo_field_with_leading(
                    ui,
                    ("gen_version", asset_id),
                    selected_text,
                    combo_w,
                    pin_leading,
                    |ui, rect| {
                        kit::paint_icon(ui, kit::Icon::Pin, rect, kit::IMAGE);
                    },
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
        });
        kit::bounded_horizontal_row(ui, 18.0, |ui, row_w| {
            ui.set_width((row_w - 22.0).max(1.0));
            if facts.is_empty() {
                ui.label(kit::caption("No generated output yet"));
            } else {
                ui.add(egui::Label::new(kit::caption(&facts)).truncate());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                let color = if response.hovered() {
                    kit::TEXT
                } else {
                    kit::TEXT_MUTED
                };
                paint_header_info_mark(ui, rect, color);
                if response
                    .on_hover_text("Show file path and less frequently used metadata.")
                    .clicked()
                {
                    details_open = !details_open;
                }
            });
        });
        ui.add_space(kit::FORM_ROW_GAP);
        kit::bounded_horizontal_row(ui, kit::SECONDARY_BUTTON_H, |ui, row_w| {
            ui.spacing_mut().item_spacing.x = kit::FIELD_COMPOUND_GAP;
            let lab_w = (row_w - kit::ICON_BUTTON_W - kit::FIELD_COMPOUND_GAP).max(80.0);
            if kit::secondary_button(ui, "Open Asset Lab", lab_w)
                .on_hover_text("Open Asset Lab for focused creating, lineage, and comparison.")
                .clicked()
            {
                let local_time = context_clip_id.and_then(|clip_id| {
                    self.editor.project.clips.iter().find_map(|clip| {
                        (clip.id == clip_id).then(|| {
                            (self.editor.current_time - clip.start_time + clip.trim_in_seconds)
                                .max(0.0)
                        })
                    })
                });
                self.open_asset_lab_at_time(asset_id, local_time);
            }
            let more = kit::icon_button(ui, "···").on_hover_text("More asset actions");
            egui::Popup::menu(&more)
                .id(ui.id().with(("attr_more", asset_id)))
                .show(|ui| {
                    if ui.button("Add to timeline").clicked() {
                        if let Err(err) = self.editor.add_asset_to_timeline(asset_id, None) {
                            self.editor.status = err;
                        }
                        ui.close();
                    }
                    if ui.button("Duplicate").clicked() {
                        self.duplicate_assets(&[asset_id]);
                        ui.close();
                    }
                    if ui
                        .button("Extract")
                        .on_hover_text("Extract the chosen generated output as a new imported asset.")
                        .clicked()
                    {
                        self.extract_active_generation(asset_id);
                        ui.close();
                    }
                    if ui.button("Manage in Asset Lab").clicked() {
                        self.open_asset_lab(asset_id);
                        ui.close();
                    }
                });
        });
        ui.data_mut(|data| data.insert_temp(details_id, details_open));
        if details_open {
            ui.add_space(kit::FORM_ROW_GAP);
            if let Some(source) = asset_source_label(&asset) {
                ui.label(kit::caption("Source"));
                ui.add(
                    egui::Label::new(kit::caption(&source).monospace())
                        .wrap()
                        .selectable(true),
                );
                if kit::secondary_button(ui, "Copy path", 88.0).clicked() {
                    ui.ctx().copy_text(source);
                    self.editor.status = "Copied source path.".to_string();
                }
            }
            self.media_binding_context_picker(ui, asset_id, context_clip_id);
            if let Some(provider) = config.provider_id.and_then(|provider_id| {
                self.editor
                    .provider_entries
                    .iter()
                    .find(|provider| provider.id == provider_id)
                    .cloned()
            }) {
                self.media_binding_bulk_actions(
                    ui,
                    asset_id,
                    context_clip_id,
                    &provider,
                    &config,
                );
            }
        }
        if next_version != config.active_version.clone().unwrap_or_default() {
            self.apply_generated_asset_version(asset_id, next_version);
        }
        ui.add_space(kit::FORM_ROW_GAP);
        ui.separator();
        ui.add_space(4.0);
        ui.label(kit::body("Next generation").size(13.0));
    }

    pub(super) fn generated_inspector_footer(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        can_generate: bool,
    ) -> bool {
        let config = self
            .editor
            .project
            .generative_config(asset_id)
            .cloned()
            .unwrap_or_default();
        let provider = config.provider_id.and_then(|provider_id| {
            self.editor
                .provider_entries
                .iter()
                .find(|provider| provider.id == provider_id)
                .cloned()
        });
        let issues = provider
            .as_ref()
            .map(|provider| {
                crate::core::generation::preflight_provider_config(
                    &self.editor.project,
                    Some(asset_id),
                    context_clip_id,
                    provider,
                    &config,
                )
            })
            .unwrap_or_default();
        let summary = footer_blocker_summary(&issues);
        if let Some(summary) = summary.as_ref() {
            let response = ui.add(
                egui::Label::new(RichText::new(summary).color(kit::MARKER).size(11.0))
                    .wrap()
                    .sense(Sense::click()),
            );
            let response = crate::core::automation::instrument_response(
                response,
                "button",
                Some(summary.clone()),
                true,
                false,
            );
            if response.clicked() {
                if let (Some(provider), Some(issue)) = (provider.as_ref(), issues.first()) {
                    if let Some(field) = provider.inputs.iter().find(|input| {
                        issue.message.starts_with(&input.label)
                            || issue.message.contains(&input.label)
                            || issue.message.contains(&input.name)
                    }) {
                        ui.data_mut(|data| {
                            data.insert_temp(
                                egui::Id::new(("attr_scroll_field", asset_id)),
                                field.name.clone(),
                            );
                        });
                    }
                }
            }
        }
        if let Some(status) = self.generation_status_for_asset(asset_id) {
            ui.label(kit::caption(status));
        }
        ui.label(kit::caption("Attempts"));
        let mut generate = false;
        kit::bounded_horizontal_row(ui, 36.0, |ui, width| {
            let mut count = config.batch.count.max(1) as i64;
            if kit::integer_step_drag_sized(
                ui,
                &mut count,
                Vec2::new(72.0, 36.0),
                1,
                Some(1),
                Some(MAX_GENERATION_BATCH_COUNT as i64),
            ) {
                let count = count.clamp(1, MAX_GENERATION_BATCH_COUNT as i64) as u32;
                self.editor
                    .project
                    .update_generative_config(asset_id, |config| {
                        config.batch.count = count;
                    });
                if let Err(err) = self.editor.project.save_generative_config(asset_id) {
                    self.editor.status = format!("Failed to save generative config: {err}");
                }
            }
            ui.add_enabled_ui(can_generate, |ui| {
                generate = kit::primary_button_sized(
                    ui,
                    "Generate",
                    (width - 80.0).max(80.0),
                    36.0,
                )
                .on_hover_text(
                    "Generate with the current settings. Ctrl+Enter works while editing Attributes.",
                )
                .clicked();
            });
        });
        generate
    }

    fn apply_generated_asset_version(&mut self, asset_id: Uuid, next_version: String) {
        let config = self
            .editor
            .project
            .generative_config(asset_id)
            .cloned()
            .unwrap_or_default();
        let next_active = if next_version.trim().is_empty() {
            None
        } else {
            Some(next_version.trim().to_string())
        };
        let restored_provider = next_active.as_ref().and_then(|version| {
            config
                .versions
                .iter()
                .find(|record| record.version == *version)
                .and_then(|record| {
                    self.editor
                        .provider_entries
                        .iter()
                        .find(|provider| provider.id == record.provider_id)
                })
                .cloned()
        });
        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                config.active_version = next_active.clone();
                if let Some(version) = next_active.as_ref() {
                    if let Some(record) = config.versions.iter().find(|record| record.version == *version)
                    {
                        config.inputs = generation_record_source_inputs(config, record);
                        config.provider_id = Some(record.provider_id);
                    }
                }
            });
        if let Some(provider) = restored_provider.as_ref() {
            self.editor
                .project
                .update_generative_config(asset_id, |config| {
                    migrate_legacy_size_input(config, provider);
                });
        }
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Failed to save generative config: {err}");
        }
        self.invalidate_generative_asset_runtime(asset_id);
    }

    pub(super) fn generated_next_form(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        selected_provider: Option<&ProviderEntry>,
        config_snapshot: &crate::state::GenerativeConfig,
        output_type: crate::state::ProviderOutputType,
        compatible_providers: &[crate::state::ProviderEntry],
        next_provider_id: &mut Option<Uuid>,
        next_seed_strategy: &mut SeedStrategy,
        batch_hint: Option<&str>,
    ) -> Vec<(String, InputValue)> {
        let mut updates = Vec::new();
        kit::field_label(ui, "Recipe");
        provider_combo_field(
            ui,
            ("gen_provider", asset_id),
            selected_provider,
            "None selected",
            ui.available_width(),
            |ui| {
                automation_selectable_value(ui, next_provider_id, None, "None selected");
                for provider in compatible_providers {
                    provider_selectable_value(ui, next_provider_id, Some(provider.id), provider);
                }
            },
        );

        if let Some(provider) = selected_provider {
            ui.add_space(kit::FORM_ROW_GAP);
            if crate::core::ideogram4_caption::is_ideogram4_text_to_image(provider) {
                let mut authoring = config_snapshot.lab_authoring.clone();
                let before = authoring.caption_mode;
                self.ideogram_caption_controls(ui, &mut authoring);
                if authoring.caption_mode != before {
                    self.editor
                        .project
                        .update_generative_config(asset_id, |config| {
                            config.lab_authoring.caption_mode = authoring.caption_mode;
                        });
                    if let Err(err) = self.editor.project.save_generative_config(asset_id) {
                        self.editor.status = format!("Failed to save generative config: {err}");
                    }
                }
                ui.add_space(kit::FORM_ROW_GAP);
            }
            let sections = crate::core::generation::generation_control_inputs(provider);
            let mut standard_inputs = sections.normal;
            if crate::core::timeline_bridge::provider_is_timeline_bridge(provider) {
                standard_inputs.extend(
                    sections
                        .timing
                        .iter()
                        .copied()
                        .filter(|input| input.role == Some(InputRole::Fps)),
                );
            }
            let prompt_inputs: Vec<_> = standard_inputs
                .iter()
                .copied()
                .filter(|input| is_prompt_like_input(input, config_snapshot))
                .collect();
            let other_inputs: Vec<_> = standard_inputs
                .iter()
                .copied()
                .filter(|input| {
                    bound_media_type_for_input(input).is_none()
                        && !is_prompt_like_input(input, config_snapshot)
                })
                .collect();

            for input in prompt_inputs {
                if crate::state::uses_magic_prompt(provider, &config_snapshot.lab_authoring)
                    && input.name == crate::core::ideogram4_caption::BACKGROUND_FIELD
                {
                    continue;
                }
                if let Some(value) = self.prompt_reference_field_inspector(
                    ui,
                    asset_id,
                    context_clip_id,
                    provider,
                    config_snapshot,
                    input,
                    &input.label,
                ) {
                    updates.push((input.name.clone(), value));
                }
                ui.add_space(kit::FORM_ROW_GAP);
            }

            self.generated_references_section(
                ui,
                asset_id,
                context_clip_id,
                provider,
                config_snapshot,
            );

            if !other_inputs.is_empty() {
                ui.add_space(kit::FORM_ROW_GAP);
                self.provider_input_controls(
                    ui,
                    asset_id,
                    context_clip_id,
                    provider,
                    config_snapshot,
                    &other_inputs,
                    &mut updates,
                );
            }
        }

        if self.should_show_provider_output_card(output_type, selected_provider) {
            ui.add_space(kit::ACTION_GAP);
            let mut drew_canvas = false;
            if let Some(provider) = selected_provider {
                drew_canvas = self.provider_canvas_controls(
                    ui,
                    asset_id,
                    provider,
                    config_snapshot,
                    &mut updates,
                    true,
                );
            }
            if !drew_canvas {
                ui.label(kit::body("Output settings").size(13.0));
                ui.add_space(kit::FIELD_LABEL_GAP);
            }
            let draw_timing = output_type == crate::state::ProviderOutputType::Video
                && selected_provider.is_none_or(|provider| {
                    !crate::core::timeline_bridge::provider_is_timeline_bridge(provider)
                });
            if draw_timing {
                ui.add_space(kit::FORM_ROW_GAP);
                self.generative_video_timing_controls(
                    ui,
                    asset_id,
                    context_clip_id,
                    selected_provider,
                );
            }
        }

        if let Some(provider) = selected_provider {
            let sections = crate::core::generation::generation_control_inputs(provider);
            if !sections.variation.is_empty() {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.separator();
                egui::CollapsingHeader::new(RichText::new("Seed").color(kit::TEXT_MUTED).size(12.0))
                    .id_salt(("generated_seed", asset_id, provider.id))
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.add_space(kit::FORM_ROW_GAP);
                        self.provider_input_controls(
                            ui,
                            asset_id,
                            context_clip_id,
                            provider,
                            config_snapshot,
                            &sections.variation,
                            &mut updates,
                        );
                        let batch = config_snapshot.batch.clone();
                        if batch.count > 1 {
                            ui.add_space(kit::FORM_ROW_GAP);
                            kit::labeled_combo_field(
                                ui,
                                "Seed behavior",
                                ("seed_strategy", asset_id),
                                seed_strategy_label(*next_seed_strategy),
                                |ui| {
                                    automation_selectable_value(
                                        ui,
                                        next_seed_strategy,
                                        SeedStrategy::Increment,
                                        "Increment",
                                    );
                                    automation_selectable_value(
                                        ui,
                                        next_seed_strategy,
                                        SeedStrategy::Random,
                                        "Random",
                                    );
                                    automation_selectable_value(
                                        ui,
                                        next_seed_strategy,
                                        SeedStrategy::Keep,
                                        "Keep",
                                    );
                                },
                            );
                            if let Some(hint) = batch_hint {
                                ui.add_space(kit::FORM_ROW_GAP);
                                ui.label(RichText::new(hint).color(kit::MARKER).size(11.0));
                            }
                        }
                    });
            }
            if !sections.advanced.is_empty() {
                ui.add_space(kit::ACTION_GAP);
                egui::CollapsingHeader::new(
                    RichText::new("Advanced").color(kit::TEXT_MUTED).size(11.0),
                )
                .id_salt(("provider_inputs_advanced", asset_id, provider.id))
                .default_open(false)
                .show(ui, |ui| {
                    ui.add_space(kit::FORM_ROW_GAP);
                    self.provider_input_controls(
                        ui,
                        asset_id,
                        context_clip_id,
                        provider,
                        config_snapshot,
                        &sections.advanced,
                        &mut updates,
                    );
                });
            }
        }
        updates
    }

    fn generated_references_section(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        provider: &crate::state::ProviderEntry,
        config: &crate::state::GenerativeConfig,
    ) {
        let expansion_id = egui::Id::new(("attr_refs_expanded", asset_id, provider.id));
        let mut expanded = ui
            .data(|data| data.get_temp::<bool>(expansion_id))
            .unwrap_or(false);
        let fields = visible_media_reference_fields(
            provider,
            config,
            &self.editor.project,
            MediaReferenceVisibility {
                show_all: expanded,
                include_required_empty: true,
            },
        );
        let addable = next_media_reference_slots(provider, config, &self.editor.project);
        let optional_capacity = visible_media_reference_fields(
            provider,
            config,
            &self.editor.project,
            MediaReferenceVisibility {
                show_all: true,
                include_required_empty: true,
            },
        )
        .iter()
        .any(|field| !field.required);
        kit::bounded_horizontal_row(ui, kit::SECONDARY_BUTTON_H, |ui, _| {
            ui.label(kit::body("References").size(13.0));
            if !addable.is_empty() {
                ui.add_space(6.0);
                let add = kit::secondary_button(ui, "+ Add", 64.0);
                if addable.len() == 1 {
                    if add.clicked() {
                        self.open_source_picker(asset_id, context_clip_id, provider, addable[0]);
                    }
                } else {
                    egui::Popup::menu(&add)
                        .id(ui.id().with(("attr_add_ref", asset_id, provider.id)))
                        .show(|ui| {
                            for field in &addable {
                                let kind = bound_media_type_for_input(field)
                                    .map(|kind| kind.label().to_ascii_lowercase())
                                    .unwrap_or_else(|| "source".into());
                                if ui.button(format!("Add {kind}")).clicked() {
                                    self.open_source_picker(
                                        asset_id,
                                        context_clip_id,
                                        provider,
                                        field,
                                    );
                                    ui.close();
                                }
                            }
                        });
                }
            }
            if optional_capacity {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if expanded { "Show used" } else { "Show all" };
                    let response = ui.add(egui::Label::new(kit::caption(label)).sense(Sense::click()));
                    let response = crate::core::automation::instrument_response(
                        response,
                        "button",
                        Some(label.to_string()),
                        true,
                        false,
                    );
                    if response.clicked() {
                        expanded = !expanded;
                    }
                });
            }
        });
        ui.data_mut(|data| data.insert_temp(expansion_id, expanded));
        ui.add_space(kit::FIELD_LABEL_GAP);
        if fields.is_empty() {
            ui.label(kit::caption("No references selected."));
        }
        for field in fields {
            self.media_source_picker_field_inspector(
                ui,
                asset_id,
                context_clip_id,
                provider,
                field,
            );
            ui.add_space(kit::FORM_ROW_GAP);
        }
    }
}

fn paint_header_info_mark(ui: &Ui, rect: Rect, color: Color32) {
    let center = rect.center();
    let radius = (rect.width() * 0.42).max(5.0);
    ui.painter()
        .circle_stroke(center, radius, Stroke::new(1.15_f32, color));
    ui.painter()
        .circle_filled(Pos2::new(center.x, center.y - radius * 0.36), 0.85, color);
    ui.painter().line_segment(
        [
            Pos2::new(center.x, center.y - radius * 0.06),
            Pos2::new(center.x, center.y + radius * 0.40),
        ],
        Stroke::new(1.35_f32, color),
    );
}

fn is_prompt_like_input(
    input: &crate::state::ProviderInputField,
    config: &crate::state::GenerativeConfig,
) -> bool {
    input.input_type == ProviderInputType::Text
        && (input.ui.as_ref().is_some_and(|ui| ui.multiline)
            || matches!(
                config.inputs.get(&input.name),
                Some(InputValue::Prompt { .. })
            )
            || input.name.eq_ignore_ascii_case("prompt"))
}

fn snapshot_pixel_size(
    record: &crate::state::GenerationRecord,
    provider: Option<&crate::state::ProviderEntry>,
) -> Option<(u32, u32)> {
    let named = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            record
                .inputs_snapshot
                .get(*key)
                .and_then(|value| match value {
                    InputValue::Literal { value } => input_value_as_i64(value),
                    _ => None,
                })
                .and_then(|value| (value > 0).then_some(value as u32))
        })
    };
    let role_dim = |role: InputRole| {
        provider.and_then(|provider| {
            provider
                .inputs
                .iter()
                .find(|input| input.role == Some(role))
                .and_then(|input| record.inputs_snapshot.get(&input.name))
                .and_then(|value| match value {
                    InputValue::Literal { value } => input_value_as_i64(value),
                    _ => None,
                })
                .and_then(|value| (value > 0).then_some(value as u32))
        })
    };
    let width = role_dim(InputRole::Width).or_else(|| named(&["width", "image_width"]));
    let height = role_dim(InputRole::Height).or_else(|| named(&["height", "image_height"]));
    match (width, height) {
        (Some(width), Some(height)) => Some((width, height)),
        _ => None,
    }
}

fn format_existing_output_facts(
    has_output: bool,
    version: Option<&str>,
    media_accessible: bool,
    dims: Option<(u32, u32)>,
    duration_seconds: Option<f64>,
    fps: Option<f64>,
    planned_duration_seconds: Option<f64>,
    planned_fps: Option<f64>,
) -> String {
    if !has_output {
        let mut parts = vec!["No generated output yet".to_string()];
        if let Some(duration) = planned_duration_seconds.filter(|duration| *duration > 0.0) {
            parts.push(format!("planned {duration:.1} s"));
        }
        if let Some(fps) = planned_fps.filter(|fps| *fps > 0.0) {
            parts.push(format!("{fps:.0} fps"));
        }
        return parts.join(" · ");
    }
    let mut parts = Vec::new();
    if let Some((width, height)) = dims {
        parts.push(format!("{width} × {height}"));
    }
    if media_accessible {
        if let Some(duration) = duration_seconds.filter(|duration| *duration > 0.0) {
            parts.push(format!("{duration:.1} s"));
        }
        if let Some(fps) = fps.filter(|fps| *fps > 0.0) {
            parts.push(format!("{fps:.0} fps"));
        }
        if parts.is_empty() {
            return version
                .filter(|version| !version.trim().is_empty())
                .unwrap_or("Generated output")
                .to_string();
        }
        return parts.join(" · ");
    }
    let version = version
        .filter(|version| !version.trim().is_empty())
        .unwrap_or("Version");
    if parts.is_empty() {
        format!("{version} recorded · media unavailable")
    } else {
        parts.push("media unavailable".to_string());
        parts.join(" · ")
    }
}

fn existing_output_facts(
    project: &crate::state::Project,
    providers: &[crate::state::ProviderEntry],
    asset: &crate::state::Asset,
    config: &crate::state::GenerativeConfig,
    probed: Option<egui::Vec2>,
    probed_fps: Option<f64>,
) -> String {
    let has_output = config.has_generated_output();
    let version = config.active_version.as_deref();
    let media_accessible = project
        .project_path
        .as_ref()
        .and_then(|root| crate::core::generation::active_asset_source_path(root, asset))
        .is_some_and(|path| path.is_file());
    let record = version.and_then(|version| {
        config
            .versions
            .iter()
            .find(|record| record.version == version)
    });
    let snapshot_dims = record.and_then(|record| {
        let provider = providers
            .iter()
            .find(|provider| provider.id == record.provider_id);
        snapshot_pixel_size(record, provider)
    });
    let dims = probed
        .map(|size| (size.x.round().max(1.0) as u32, size.y.round().max(1.0) as u32))
        .or(if media_accessible { snapshot_dims } else { None });
    let (duration, fps, planned_duration, planned_fps) = if has_output {
        let duration = if media_accessible {
            asset.duration_seconds
        } else {
            None
        };
        let fps = if media_accessible {
            probed_fps.or(match asset.kind {
                AssetKind::GenerativeVideo { fps, .. } if fps > 0.0 => Some(fps),
                _ => None,
            })
        } else {
            None
        };
        (duration, fps, None, None)
    } else {
        match asset.kind {
            AssetKind::GenerativeVideo { fps, .. } => (
                None,
                None,
                asset.duration_seconds,
                (fps > 0.0).then_some(fps),
            ),
            _ => (None, None, None, None),
        }
    };
    format_existing_output_facts(
        has_output,
        version,
        media_accessible,
        dims,
        duration,
        fps,
        planned_duration,
        planned_fps,
    )
}

fn footer_blocker_summary(
    issues: &[crate::core::generation::GenerationPreflightIssue],
) -> Option<String> {
    if issues.is_empty() {
        return None;
    }
    let missing: Vec<_> = issues
        .iter()
        .filter(|issue| issue.message.contains("choose a source"))
        .collect();
    if !missing.is_empty() && missing.len() == issues.len() {
        if missing.len() == 1 {
            let label = missing[0]
                .message
                .split(':')
                .next()
                .unwrap_or("Input")
                .trim();
            return Some(format!("{label} is required"));
        }
        return Some(format!("{} required inputs missing", missing.len()));
    }
    let first = &issues[0].message;
    if let Some((label, rest)) = first.split_once(':') {
        if !rest.trim().eq_ignore_ascii_case("choose a source.") {
            return Some(format!("{label} cannot resolve"));
        }
    }
    Some(first.clone())
}

#[cfg(test)]
mod output_fact_tests {
    use super::format_existing_output_facts;

    #[test]
    fn missing_provider_with_accessible_media_still_shows_facts() {
        assert_eq!(
            format_existing_output_facts(
                true,
                Some("v3"),
                true,
                Some((768, 768)),
                None,
                None,
                None,
                None
            ),
            "768 × 768"
        );
    }

    #[test]
    fn recorded_version_without_file_is_not_empty_output() {
        assert_eq!(
            format_existing_output_facts(true, Some("v3"), false, None, None, None, None, None),
            "v3 recorded · media unavailable"
        );
    }

    #[test]
    fn hollow_video_timings_are_labeled_planned() {
        assert_eq!(
            format_existing_output_facts(
                false,
                None,
                false,
                None,
                None,
                None,
                Some(5.0),
                Some(24.0)
            ),
            "No generated output yet · planned 5.0 s · 24 fps"
        );
    }

    #[test]
    fn true_empty_output_stays_explicit() {
        assert_eq!(
            format_existing_output_facts(false, None, false, None, None, None, None, None),
            "No generated output yet"
        );
    }
}
