use super::*;
#[derive(Clone, Debug, Default)]
pub(super) struct ApiKeyModalState {
    pub(super) credential_id: String,
    pub(super) label: String,
    pub(super) value: String,
    pub(super) saved: bool,
    pub(super) masked_existing: bool,
    pub(super) error: Option<String>,
}

impl NlaEguiApp {
    pub(super) fn open_api_key_modal(&mut self, credential_id: &str, label: &str) {
        let saved = crate::core::credentials::has_secret(credential_id);
        let mut error = None;
        let value = if saved {
            match crate::core::credentials::secret_char_count(credential_id) {
                Ok(count) => "*".repeat(count.max(1)),
                Err(err) => {
                    error = Some(err);
                    String::new()
                }
            }
        } else {
            String::new()
        };
        self.api_key_modal = ApiKeyModalState {
            credential_id: credential_id.to_string(),
            label: label.to_string(),
            value,
            saved,
            masked_existing: saved && error.is_none(),
            error,
        };
        self.editor.overlays.api_keys = true;
    }

    pub(super) fn api_keys_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let mut save_clicked = false;
        let mut remove_clicked = false;
        let size = modal_size(ctx, API_KEYS_MODAL_SIZE, [420.0, 300.0]);
        let title = format!("{} API Key", self.api_key_modal.label);
        let subtitle = if self.api_key_modal.saved {
            "Stored. Enter a new key to replace it."
        } else {
            "Not stored yet."
        };

        let outside_clicked = kit::dismissible_modal_scrim(ctx, "api_keys", true);
        egui::Window::new("API Key")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(ui, &title, Some(subtitle), true);
                kit::modal_body(ui, |ui| {
                    if let Some(error) = &self.api_key_modal.error {
                        ui.label(RichText::new(error).color(kit::DANGER).size(12.0));
                        ui.add_space(kit::FORM_ROW_GAP);
                    }

                    kit::body_with_footer(
                        ui,
                        132.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| {
                            kit::card_panel(ui, ui.available_height(), |ui| {
                                ui.label(kit::caption(
                                    "Keys are stored locally with Windows user-level encryption.",
                                ));
                                if self.api_key_modal.masked_existing {
                                    ui.add_space(kit::FORM_ROW_GAP);
                                    ui.label(kit::caption(
                                        "The saved key is shown as a length-matched placeholder.",
                                    ));
                                }
                                ui.add_space(kit::ACTION_GAP);
                                let response = kit::labeled_password_field(
                                    ui,
                                    "API Key",
                                    &mut self.api_key_modal.value,
                                );
                                if self.api_key_modal.masked_existing
                                    && (response.changed()
                                        || response.has_focus()
                                            && self.api_key_modal.value.chars().any(|ch| ch != '*'))
                                {
                                    self.api_key_modal.masked_existing = false;
                                }
                            });
                        },
                        |ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if kit::primary_button(ui, "Save Key", 120.0).clicked() {
                                    save_clicked = true;
                                }
                                if kit::secondary_button(ui, "Close", 110.0).clicked() {
                                    close_clicked = true;
                                }
                                if self.api_key_modal.saved
                                    && kit::danger_button(ui, "Remove", 100.0).clicked()
                                {
                                    remove_clicked = true;
                                }
                            });
                        },
                    );
                });
            });

        if remove_clicked {
            match crate::core::credentials::delete_secret(&self.api_key_modal.credential_id) {
                Ok(()) => {
                    self.editor.status = format!("Removed {} API key.", self.api_key_modal.label);
                    self.editor.overlays.api_keys = false;
                }
                Err(err) => self.api_key_modal.error = Some(err),
            }
        }
        if save_clicked {
            self.save_api_key_modal();
        }
        if close_clicked || outside_clicked || !open {
            self.api_key_modal.value.clear();
            self.api_key_modal.error = None;
            self.editor.overlays.api_keys = false;
        }
    }

    pub(super) fn save_api_key_modal(&mut self) {
        if self.api_key_modal.masked_existing {
            self.editor.status = format!("Kept existing {} API key.", self.api_key_modal.label);
            self.editor.overlays.api_keys = false;
            return;
        }
        if self.api_key_modal.value.trim().is_empty() {
            self.api_key_modal.error = Some("Enter an API key before saving.".to_string());
            return;
        }
        let storage_label = format!("{} API Key", self.api_key_modal.label);
        if let Err(err) = crate::core::credentials::save_secret(
            &self.api_key_modal.credential_id,
            &storage_label,
            &self.api_key_modal.value,
        ) {
            self.api_key_modal.error = Some(err);
            return;
        }

        self.editor.status = format!("Saved {} API key.", self.api_key_modal.label);
        self.api_key_modal.value.clear();
        self.api_key_modal.error = None;
        self.api_key_modal.saved = true;
        self.editor.overlays.api_keys = false;
    }

    pub(super) fn providers_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let modal_size = modal_size(ctx, PROVIDERS_MODAL_SIZE, [620.0, 460.0]);
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "providers", true);
        egui::Window::new("AI Providers (Global)")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(modal_size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "AI Providers",
                    Some("Global provider definitions and manifests."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    StripBuilder::new(ui)
                        .clip(true)
                        .size(Size::exact(300.0))
                        .size(Size::exact(12.0))
                        .size(Size::remainder().at_least(260.0))
                        .horizontal(|mut strip| {
                            strip.cell(|ui| self.provider_list_card(ui));
                            strip.empty();
                            strip.cell(|ui| self.provider_editor_choice_card(ui));
                        });
                });
            });
        if close_clicked || outside_clicked || !open {
            self.editor.overlays.providers = false;
        }
    }

    pub(super) fn provider_list_card(&mut self, ui: &mut Ui) {
        let card_h = ui.available_height();
        kit::card_panel(ui, card_h, |ui| {
            self.add_provider_controls(ui);

            ui.add_space(kit::ACTION_GAP);
            let selected = self.selected_provider_file.clone();
            let provider_files = self.editor.provider_files.clone();
            let mut next_selection: Option<PathBuf> = None;

            ui.horizontal(|ui| {
                ui.label(kit::section_label("Installed"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if kit::secondary_button(ui, "Reload", 76.0).clicked() {
                        self.editor.refresh_providers();
                    }
                });
            });
            ui.add_space(kit::FORM_ROW_GAP);
            kit::scroll_body(ui, |ui| {
                ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                if provider_files.is_empty() {
                    kit::empty_state(
                        ui,
                        "No providers yet",
                        "Create a provider or reload the global provider folder.",
                    );
                }
                for path in provider_files.iter() {
                    let summary = provider_file_summary(path);
                    let is_selected = selected.as_ref() == Some(path);
                    let response = provider_row(ui, path, &summary, is_selected);
                    if response.clicked() {
                        next_selection = Some(path.clone());
                    }
                }
            });

            if let Some(path) = next_selection {
                self.selected_provider_file = Some(path);
            }
        });
    }

    pub(super) fn add_provider_controls(&mut self, ui: &mut Ui) {
        kit::field_label(ui, "Add Provider");
        ui.add_space(kit::FORM_ROW_GAP);

        let selected_label = provider_template_dropdown_label(
            self.provider_template_kind,
            self.provider_template_unavailable(self.provider_template_kind),
        );
        let mut selected_kind = self.provider_template_kind;
        ui.horizontal(|ui| {
            let button_w = kit::FIELD_H;
            let combo_w = (ui.available_width() - kit::FIELD_COMPOUND_GAP - button_w).max(80.0);
            kit::combo_field(
                ui,
                "provider_template_kind",
                selected_label,
                combo_w,
                |ui| {
                    for kind in ProviderTemplateKind::ALL {
                        let unavailable = self.provider_template_unavailable(kind);
                        let label = provider_template_dropdown_label(kind, unavailable);
                        ui.add_enabled_ui(!unavailable, |ui| {
                            automation_selectable_value(ui, &mut selected_kind, kind, &label);
                        });
                    }
                },
            );
            let unavailable = self.provider_template_unavailable(self.provider_template_kind);
            ui.add_enabled_ui(!unavailable, |ui| {
                if kit::primary_button(ui, "+", button_w).clicked() {
                    self.create_selected_provider_template();
                }
            });
        });
        self.provider_template_kind = selected_kind;
    }

    pub(super) fn provider_template_unavailable(&self, kind: ProviderTemplateKind) -> bool {
        match kind {
            ProviderTemplateKind::ComfyUi => false,
            ProviderTemplateKind::OpenAiImage => self
                .editor
                .provider_entries
                .iter()
                .any(|entry| matches!(entry.connection, ProviderConnection::OpenAiImage { .. })),
            ProviderTemplateKind::XaiImage => self
                .editor
                .provider_entries
                .iter()
                .any(|entry| matches!(entry.connection, ProviderConnection::XaiImage { .. })),
            ProviderTemplateKind::XaiVideo => self
                .editor
                .provider_entries
                .iter()
                .any(|entry| matches!(entry.connection, ProviderConnection::XaiVideo { .. })),
        }
    }

    pub(super) fn create_selected_provider_template(&mut self) {
        match self.provider_template_kind {
            ProviderTemplateKind::ComfyUi => self.open_provider_builder(None),
            ProviderTemplateKind::OpenAiImage => self.save_provider_template(
                crate::core::provider_store::default_openai_image_provider_entry(),
            ),
            ProviderTemplateKind::XaiImage => self.save_provider_template(
                crate::core::provider_store::default_xai_image_provider_entry(),
            ),
            ProviderTemplateKind::XaiVideo => self.save_provider_template(
                crate::core::provider_store::default_xai_video_provider_entry(),
            ),
        }
    }

    pub(super) fn provider_editor_choice_card(&mut self, ui: &mut Ui) {
        let card_h = ui.available_height();
        kit::card_panel(ui, card_h, |ui| {
            let Some(path) = self.selected_provider_file.clone() else {
                kit::empty_state(
                    ui,
                    "Select a provider",
                    "Choose an installed provider to edit, or add one from the cloud provider catalog.",
                );
                return;
            };

            if !path.exists() {
                kit::empty_state(
                    ui,
                    "Provider missing",
                    "Reload the provider list to refresh this selection.",
                );
                return;
            }

            let summary = provider_file_summary(&path);
            let supports_builder = provider_file_supports_comfy_builder(&path);
            let credential = provider_file_credential(&path);
            let mut open_builder = false;
            let mut open_json = false;
            let mut open_key = false;
            let mut delete_clicked = false;
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(&summary.name)
                            .color(kit::TEXT)
                            .strong()
                            .size(15.0),
                    );
                    ui.add_space(4.0);
                    ui.label(kit::caption(if supports_builder {
                        "Select an editor:"
                    } else {
                        "Cloud providers use direct settings and app API keys."
                    }));
                    ui.add_space(24.0);
                    if supports_builder {
                        if kit::secondary_button(ui, "Edit in Builder", 250.0).clicked() {
                            open_builder = true;
                        }
                        ui.add_space(8.0);
                    }
                    if kit::secondary_button(ui, "Edit as JSON", 250.0).clicked() {
                        open_json = true;
                    }
                    if credential.is_some() {
                        ui.add_space(8.0);
                        if kit::secondary_button(ui, "API Key", 250.0).clicked() {
                            open_key = true;
                        }
                    }
                    ui.add_space(8.0);
                    if kit::danger_button(ui, "Delete Provider", 250.0).clicked() {
                        delete_clicked = true;
                    }
                });
            });

            if open_builder {
                self.open_provider_builder(Some(path.clone()));
            }
            if open_json {
                self.open_provider_json_editor(path.clone());
            }
            if open_key {
                if let Some((credential_id, label)) = credential {
                    self.open_api_key_modal(credential_id, label);
                }
            }
            if delete_clicked {
                self.delete_provider_file(path);
            }
        });
    }

    pub(super) fn delete_provider_file(&mut self, path: PathBuf) {
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.editor.status = format!("Deleted provider {}", path_label(&path));
                self.selected_provider_file = None;
                self.refresh_provider_files();
            }
            Err(err) => {
                self.editor.status =
                    format!("Failed to delete provider {}: {err}", path_label(&path));
            }
        }
    }

    pub(super) fn provider_json_editor_modal(&mut self, ctx: &Context) {
        let Some(path) = self.provider_json_editor_path.clone() else {
            return;
        };
        let mut open = true;
        let mut close_clicked = false;
        let mut save_clicked = false;
        let size = modal_size(ctx, PROVIDER_JSON_MODAL_SIZE, [680.0, 520.0]);

        let outside_clicked = kit::dismissible_modal_scrim(ctx, "provider_json_editor", true);
        egui::Window::new("Edit Provider JSON")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let file_name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("provider.json");
                close_clicked =
                    kit::modal_header_with_close(ui, "Edit Provider JSON", Some(file_name), true);
                kit::modal_body(ui, |ui| {
                    if let Some(error) = &self.provider_json_error {
                        ui.label(RichText::new(error).color(kit::MARKER).size(12.0));
                        ui.add_space(kit::FORM_ROW_GAP);
                    }

                    kit::body_with_footer(
                        ui,
                        320.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| {
                            kit::code_editor_field(
                                ui,
                                &mut self.provider_json_text,
                                "provider_json_editor",
                            );
                        },
                        |ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if kit::primary_button(ui, "Save JSON", 130.0).clicked() {
                                    save_clicked = true;
                                }
                                if kit::secondary_button(ui, "Cancel", 110.0).clicked() {
                                    close_clicked = true;
                                }
                            });
                        },
                    );
                });
            });

        if save_clicked {
            self.save_provider_json_editor(&path);
        }
        if close_clicked || outside_clicked || !open {
            self.provider_json_editor_path = None;
            self.provider_json_error = None;
        }
    }

    pub(super) fn refresh_provider_files(&mut self) {
        self.editor.refresh_providers();
        if let Some(selected) = &self.selected_provider_file {
            if !self
                .editor
                .provider_files
                .iter()
                .any(|path| path == selected)
            {
                self.selected_provider_file = None;
            }
        }
    }

    pub(super) fn save_provider_template(&mut self, entry: ProviderEntry) {
        match crate::core::provider_store::save_global_provider_entry(&entry) {
            Ok(path) => {
                self.selected_provider_file = Some(path.clone());
                self.refresh_provider_files();
                self.editor.status = format!("Created provider {}", path_label(&path));
            }
            Err(err) => {
                self.editor.status = format!("Failed to create provider: {err}");
            }
        }
    }

    pub(super) fn open_provider_json_editor(&mut self, path: PathBuf) {
        self.provider_json_text =
            crate::core::provider_store::read_provider_file(&path).unwrap_or_default();
        self.provider_json_error = if self.provider_json_text.is_empty() {
            Some(format!("Failed to read provider {}", path.display()))
        } else {
            None
        };
        self.provider_json_editor_path = Some(path);
    }

    pub(super) fn save_provider_json_editor(&mut self, path: &Path) {
        let entry = match serde_json::from_str::<ProviderEntry>(&self.provider_json_text) {
            Ok(entry) => entry,
            Err(err) => {
                self.provider_json_error = Some(format!("Invalid provider JSON: {err}"));
                return;
            }
        };
        let pretty = match serde_json::to_string_pretty(&entry) {
            Ok(pretty) => pretty,
            Err(err) => {
                self.provider_json_error = Some(format!("Failed to format provider JSON: {err}"));
                return;
            }
        };
        if let Err(err) = crate::core::provider_store::write_provider_file(path, &pretty) {
            self.provider_json_error = Some(format!("Failed to save provider: {err}"));
            return;
        }

        self.provider_json_text = pretty;
        self.provider_json_error = None;
        self.selected_provider_file = Some(path.to_path_buf());
        self.refresh_provider_files();
        self.provider_json_editor_path = None;
        self.editor.status = format!("Saved provider {}", path_label(path));
    }

    pub(super) fn open_provider_builder(&mut self, path: Option<PathBuf>) {
        let mut state = match path.as_ref() {
            Some(path) => ProviderBuilderState::from_path(path),
            None => ProviderBuilderState::from_entry(
                None,
                crate::core::provider_store::default_provider_entry(),
            ),
        };
        if state.source_path.is_none() {
            state.source_path = path;
        }
        self.provider_builder = state;
        self.provider_builder_open = true;
    }

    pub(super) fn provider_builder_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let mut save_clicked = false;
        let size = modal_size(ctx, PROVIDER_BUILDER_MODAL_SIZE, [780.0, 560.0]);

        let outside_clicked = kit::dismissible_modal_scrim(ctx, "provider_builder", true);
        egui::Window::new("Provider Builder")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "Provider Builder (ComfyUI)",
                    Some(if self.provider_builder.source_path.is_some() {
                        "Mode: Edit"
                    } else {
                        "Mode: New"
                    }),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    self.provider_builder_topbar(ui);
                    self.provider_builder_errors(ui);
                    ui.add_space(kit::FORM_ROW_GAP);
                    self.provider_builder_tabs(ui);
                    ui.add_space(kit::FORM_ROW_GAP);
                    kit::body_with_footer(
                        ui,
                        360.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| self.provider_builder_columns(ui),
                        |ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if kit::primary_button(ui, "Save Provider", 150.0).clicked() {
                                    save_clicked = true;
                                }
                                if kit::secondary_button(ui, "Cancel", 110.0).clicked() {
                                    close_clicked = true;
                                }
                            });
                        },
                    );
                });
            });

        if save_clicked {
            self.save_provider_builder();
        }
        if close_clicked || outside_clicked || !open {
            self.provider_builder_open = false;
            self.provider_builder.error = None;
            self.provider_builder.workflow_error = None;
        }
    }

    pub(super) fn provider_builder_topbar(&mut self, ui: &mut Ui) {
        let workflow_display = self
            .provider_builder
            .workflow_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "No workflow selected".to_string());
        ui.horizontal(|ui| {
            ui.add_sized(
                [(ui.available_width() - 160.0).max(80.0), 18.0],
                egui::Label::new(kit::caption(workflow_display)).truncate(),
            );
            if kit::secondary_button(ui, "Choose Workflow...", 148.0).clicked() {
                let initial = self
                    .provider_builder
                    .workflow_path
                    .as_ref()
                    .and_then(|path| path.parent().map(Path::to_path_buf))
                    .or_else(|| crate::core::paths::resource_dir("workflows"));
                let mut options = kit::BrowseFileOptions::new()
                    .id_salt("provider_builder_workflow")
                    .filters(JSON_FILE_FILTERS)
                    .remember_last_dir();
                if let Some(initial) = initial.as_deref() {
                    options = options.initial_dir(initial);
                }
                if let Some(path) = kit::pick_file_dialog(ui, options) {
                    self.set_provider_builder_workflow(path);
                }
            }
        });
    }

    pub(super) fn provider_builder_errors(&mut self, ui: &mut Ui) {
        if let Some(error) = &self.provider_builder.workflow_error {
            ui.label(RichText::new(error).color(kit::MARKER).size(12.0));
        }
        if let Some(error) = &self.provider_builder.error {
            ui.label(RichText::new(error).color(kit::MARKER).size(12.0));
        }
    }

    pub(super) fn provider_builder_tabs(&mut self, ui: &mut Ui) {
        self.provider_builder.ensure_valid_tab();
        let output_active = self.provider_builder.tab == ProviderBuilderTab::Output;
        let inputs_active = self.provider_builder.tab == ProviderBuilderTab::Inputs;
        let inputs_enabled = self.provider_builder.output_configured();

        ui.horizontal(|ui| {
            if kit::timeline_tool_text_button(ui, "Output", 74.0, output_active).clicked() {
                self.provider_builder.tab = ProviderBuilderTab::Output;
            }
            let inputs_response = ui
                .add_enabled_ui(inputs_enabled, |ui| {
                    kit::timeline_tool_text_button(ui, "Inputs", 74.0, inputs_active)
                })
                .inner;
            let inputs_clicked = inputs_response.clicked();
            if !inputs_enabled {
                inputs_response.on_disabled_hover_text("Select an output node first.");
            }
            if inputs_clicked {
                self.provider_builder.tab = ProviderBuilderTab::Inputs;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(kit::caption(self.provider_builder.output_status_label()));
            });
        });
    }

    pub(super) fn provider_builder_columns(&mut self, ui: &mut Ui) {
        kit::fixed_panel_body(ui, |ui| {
            StripBuilder::new(ui)
                .clip(true)
                .size(Size::exact(280.0))
                .size(Size::exact(12.0))
                .size(Size::exact(230.0))
                .size(Size::exact(12.0))
                .size(Size::remainder().at_least(260.0))
                .horizontal(|mut strip| {
                    strip.cell(|ui| self.provider_builder_node_list(ui));
                    strip.empty();
                    strip.cell(|ui| self.provider_builder_node_details(ui));
                    strip.empty();
                    strip.cell(|ui| self.provider_builder_settings(ui));
                });
        });
    }

    pub(super) fn provider_builder_node_list(&mut self, ui: &mut Ui) {
        kit::card_panel(ui, ui.available_height(), |ui| {
            kit::singleline_text_field(
                ui,
                &mut self.provider_builder.workflow_search,
                ui.available_width(),
            );
            ui.add_space(kit::FORM_ROW_GAP);
            let filtered = self.provider_builder.filtered_nodes();
            kit::scroll_body(ui, |ui| {
                ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                if filtered.is_empty() {
                    kit::empty_state(
                        ui,
                        if self.provider_builder.workflow_nodes.is_empty() {
                            "No workflow nodes"
                        } else {
                            "No matching nodes"
                        },
                        "Choose a workflow or adjust the search.",
                    );
                }
                for node in filtered {
                    let selected = self
                        .provider_builder
                        .selected_node_id
                        .as_ref()
                        .is_some_and(|id| id == &node.id);
                    let output_selected = self.provider_builder.node_is_output(&node.id);
                    let exposed_input_count =
                        self.provider_builder.exposed_input_count_for_node(&node.id);
                    let response = workflow_node_row(
                        ui,
                        &node,
                        selected,
                        output_selected,
                        exposed_input_count,
                        self.provider_builder.output_type,
                    );
                    if response.clicked() {
                        self.provider_builder.selected_node_id = Some(node.id);
                    }
                }
            });
        });
    }

    pub(super) fn provider_builder_node_details(&mut self, ui: &mut Ui) {
        kit::card_panel(ui, ui.available_height(), |ui| {
            let selected_node = self.provider_builder.selected_node();
            let Some(node) = selected_node else {
                kit::empty_state(
                    ui,
                    "Select a node",
                    if self.provider_builder.tab == ProviderBuilderTab::Inputs {
                        "Expose workflow inputs from the selected node."
                    } else {
                        "Use the selected node as the output source."
                    },
                );
                return;
            };

            ui.label(kit::value(
                node.title.clone().unwrap_or_else(|| "Untitled".to_string()),
            ));
            ui.label(kit::caption(format!("Class: {}", node.class_type)));
            ui.label(kit::caption(format!("Node ID: {}", node.id)));
            ui.add_space(kit::ACTION_GAP);

            match self.provider_builder.tab {
                ProviderBuilderTab::Inputs => {
                    if !self.provider_builder.output_configured() {
                        kit::empty_state(
                            ui,
                            "Set output first",
                            "Choose the workflow node that produces the final media before exposing inputs.",
                        );
                        return;
                    }
                    kit::field_label(ui, "Inputs");
                    ui.add_space(kit::FORM_ROW_GAP);
                    if node.inputs.is_empty() {
                        ui.label(kit::caption("No inputs found on this node."));
                        return;
                    }
                    let mut expose_key: Option<String> = None;
                    kit::scroll_body(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                        for input_key in node.inputs.iter() {
                            let already_exposed =
                                self.provider_builder.input_exposed(&node.id, input_key);
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [(ui.available_width() - 76.0).max(60.0), 18.0],
                                    egui::Label::new(kit::body(input_key)).truncate(),
                                );
                                let label = if already_exposed { "Exposed" } else { "Expose" };
                                let response = ui
                                    .add_enabled_ui(!already_exposed, |ui| {
                                        kit::field_button(ui, label, 68.0)
                                    })
                                    .inner;
                                let clicked = response.clicked();
                                if already_exposed {
                                    response.on_disabled_hover_text(
                                        "This workflow input is already exposed.",
                                    );
                                }
                                if clicked {
                                    expose_key = Some(input_key.clone());
                                }
                            });
                        }
                    });
                    if let Some(input_key) = expose_key {
                        self.expose_provider_builder_input(&node, &input_key);
                    }
                }
                ProviderBuilderTab::Output => {
                    kit::field_label(ui, "Output Node");
                    ui.add_space(kit::FORM_ROW_GAP);
                    let output_selected = self.provider_builder.node_is_output(&node.id);
                    let use_output_w = ui.available_width();
                    let label = if output_selected {
                        "Output Selected"
                    } else {
                        "Use as Output"
                    };
                    let response = ui
                        .add_enabled_ui(!output_selected, |ui| {
                            kit::secondary_button(ui, label, use_output_w)
                        })
                        .inner;
                    let clicked = response.clicked();
                    if output_selected {
                        response
                            .on_disabled_hover_text("This node is already the provider output.");
                    }
                    if clicked {
                        self.provider_builder.output_node = Some(ProviderOutputNodeDraft {
                            node_id: Some(node.id),
                            class_type: node.class_type,
                            title: node.title,
                        });
                        self.provider_builder.output_key = self
                            .provider_builder
                            .output_node
                            .as_ref()
                            .map(|node| {
                                inferred_output_key_for_node(
                                    node,
                                    self.provider_builder.output_type,
                                )
                            })
                            .unwrap_or_else(|| {
                                default_output_key(self.provider_builder.output_type).to_string()
                            });
                        self.provider_builder.output_tag = "output".to_string();
                        self.provider_builder.error = None;
                    }
                }
            }
        });
    }

    pub(super) fn provider_builder_settings(&mut self, ui: &mut Ui) {
        kit::scroll_body(ui, |ui| {
            kit::card_frame().show(ui, |ui| {
                kit::field_label(ui, "Provider Settings");
                ui.add_space(kit::FORM_ROW_GAP);
                kit::field_grid_row(ui, &[1.0, 0.46], |ui, index| match index {
                    0 => {
                        kit::labeled_text_field(
                            ui,
                            "Name",
                            &mut self.provider_builder.provider_name,
                        );
                    }
                    1 => {
                        provider_output_type_field(
                            ui,
                            "Type",
                            &mut self.provider_builder.output_type,
                        );
                    }
                    _ => {}
                });
                ui.add_space(kit::FORM_ROW_GAP);
                provider_workflow_kind_field(
                    ui,
                    "Generation",
                    &mut self.provider_builder.workflow_kind,
                );
                ui.add_space(kit::FORM_ROW_GAP);
                kit::labeled_text_field(ui, "Base URL", &mut self.provider_builder.base_url);
                ui.add_space(kit::FORM_ROW_GAP);
                ui.horizontal(|ui| {
                    if kit::secondary_button(ui, "Refresh Schema", 130.0).clicked() {
                        self.refresh_provider_builder_schema();
                    }
                    if let Some(status) = &self.provider_builder.schema_status {
                        ui.add_sized(
                            [(ui.available_width()).max(40.0), 18.0],
                            egui::Label::new(kit::caption(status)).truncate(),
                        );
                    }
                });
                ui.add_space(kit::FORM_ROW_GAP);

                let workflow_display = self.provider_builder.workflow_path_display();
                let workflow_initial = self
                    .provider_builder
                    .workflow_path
                    .as_ref()
                    .and_then(|path| path.parent())
                    .or_else(|| {
                        self.provider_builder
                            .source_path
                            .as_deref()
                            .and_then(Path::parent)
                    });
                let mut workflow_options = kit::BrowseFileOptions::new()
                    .id_salt("provider_builder_workflow_field")
                    .filters(JSON_FILE_FILTERS)
                    .remember_last_dir();
                if let Some(initial) = workflow_initial {
                    workflow_options = workflow_options.initial_dir(initial);
                }
                if let Some(path) = kit::labeled_browse_file_field(
                    ui,
                    "Workflow",
                    workflow_display,
                    workflow_options,
                ) {
                    self.set_provider_builder_workflow(path);
                }
                ui.add_space(kit::FORM_ROW_GAP);

                let manifest_display = self.provider_builder.manifest_path_display();
                let manifest_initial = self
                    .provider_builder
                    .manifest_path
                    .as_ref()
                    .and_then(|path| path.parent())
                    .or_else(|| {
                        self.provider_builder
                            .workflow_path
                            .as_deref()
                            .and_then(Path::parent)
                    });
                let mut manifest_options = kit::BrowseFileOptions::new()
                    .id_salt("provider_builder_manifest_field")
                    .filters(JSON_FILE_FILTERS)
                    .remember_last_dir();
                if let Some(initial) = manifest_initial {
                    manifest_options = manifest_options.initial_dir(initial);
                }
                if let Some(path) = kit::labeled_browse_file_field(
                    ui,
                    "Manifest",
                    manifest_display,
                    manifest_options,
                ) {
                    self.set_provider_builder_manifest(path);
                }
            });

            ui.add_space(kit::ACTION_GAP);
            self.provider_builder.ensure_valid_tab();
            match self.provider_builder.tab {
                ProviderBuilderTab::Inputs => self.provider_builder_inputs_editor(ui),
                ProviderBuilderTab::Output => self.provider_builder_output_editor(ui),
            }
        });
    }

    pub(super) fn provider_builder_inputs_editor(&mut self, ui: &mut Ui) {
        kit::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                kit::field_label(
                    ui,
                    &format!("Exposed Inputs ({})", self.provider_builder.inputs.len()),
                );
            });
            ui.add_space(kit::FORM_ROW_GAP);
            if self.provider_builder.inputs.is_empty() {
                ui.label(kit::caption(
                    "No inputs exposed. Select a workflow node and expose its inputs.",
                ));
                return;
            }

            let mut action = None;
            let len = self.provider_builder.inputs.len();
            for index in 0..len {
                if index > 0 {
                    ui.add_space(kit::FORM_ROW_GAP);
                }
                provider_builder_input_editor(
                    ui,
                    index,
                    len,
                    &mut self.provider_builder.inputs[index],
                    &mut action,
                );
            }
            if let Some(action) = action {
                self.apply_provider_input_action(action);
            }
        });
    }

    pub(super) fn provider_builder_output_editor(&mut self, ui: &mut Ui) {
        kit::card_frame().show(ui, |ui| {
            kit::field_label(ui, "Output Configuration");
            ui.add_space(kit::FORM_ROW_GAP);
            if let Some(node) = self.provider_builder.output_node.as_ref() {
                let output_label = node
                    .title
                    .clone()
                    .unwrap_or_else(|| node.class_type.clone());
                ui.label(kit::value(output_label));
                ui.label(kit::caption(format!(
                    "Node {} / {}",
                    node.node_id.as_deref().unwrap_or("-"),
                    node.class_type
                )));
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(format!(
                    "The app will read the selected node's ComfyUI history and pick the first {} file it produced.",
                    provider_output_type_label(self.provider_builder.output_type)
                )));
            } else {
                ui.label(kit::caption(
                    "Select a saver/output node, then click Use as Output.",
                ));
            }
        });
    }

    pub(super) fn set_provider_builder_workflow(&mut self, path: PathBuf) {
        if let Some(name) = provider_name_from_workflow_path(&path) {
            self.provider_builder.provider_name = name;
        }
        match load_workflow_nodes_resolved(&path) {
            Ok(nodes) => {
                self.provider_builder.workflow_path = Some(path);
                self.provider_builder.workflow_nodes = nodes;
                self.provider_builder.workflow_error = None;
                self.provider_builder.selected_node_id = None;
                self.provider_builder.reset_workflow_bindings();
            }
            Err(err) => {
                self.provider_builder.workflow_path = Some(path);
                self.provider_builder.workflow_nodes.clear();
                self.provider_builder.workflow_error = Some(err);
                self.provider_builder.selected_node_id = None;
                self.provider_builder.reset_workflow_bindings();
            }
        }
    }

    pub(super) fn set_provider_builder_manifest(&mut self, path: PathBuf) {
        match load_provider_manifest_resolved(&path) {
            Ok(manifest) => {
                self.provider_builder.apply_manifest(manifest);
                self.provider_builder.manifest_path = Some(path);
                self.provider_builder.error = None;
                if let Some(workflow_path) = self.provider_builder.workflow_path.clone() {
                    match load_workflow_nodes_resolved(&workflow_path) {
                        Ok(nodes) => {
                            self.provider_builder.workflow_nodes = nodes;
                            self.provider_builder.workflow_error = None;
                            self.provider_builder.selected_node_id = None;
                        }
                        Err(err) => {
                            self.provider_builder.workflow_nodes.clear();
                            self.provider_builder.workflow_error = Some(err);
                        }
                    }
                }
            }
            Err(err) => {
                self.provider_builder.manifest_path = Some(path);
                self.provider_builder.error = Some(err);
            }
        }
    }

    pub(super) fn refresh_provider_builder_schema(&mut self) {
        let base_url = self.provider_builder.base_url.trim().to_string();
        if base_url.is_empty() {
            self.provider_builder.error =
                Some("Enter the provider's ComfyUI base URL before refreshing schema.".to_string());
            return;
        }
        let Some(runtime) = self.generation_runtime.as_ref() else {
            self.provider_builder.error =
                Some("Generation runtime is unavailable; cannot query ComfyUI schema.".to_string());
            return;
        };

        match runtime.block_on(crate::providers::comfyui::fetch_object_info(&base_url)) {
            Ok(value) => {
                let schema = crate::core::comfyui_workflow::parse_object_info_schema(&value);
                let class_count = schema.len();
                self.provider_builder.comfy_schema = schema;
                self.provider_builder.schema_base_url = Some(base_url);
                let enriched_count = self.provider_builder.enrich_existing_inputs_from_schema();
                self.provider_builder.schema_status = Some(format!(
                    "Loaded schema for {class_count} Comfy node classes; updated {enriched_count} exposed inputs."
                ));
                self.provider_builder.error = None;
            }
            Err(err) => {
                self.provider_builder.schema_status = None;
                self.provider_builder.error = Some(err);
            }
        }
    }

    pub(super) fn expose_provider_builder_input(
        &mut self,
        node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
        input_key: &str,
    ) {
        if self.provider_builder.inputs.iter().any(|input| {
            input.selector.node_id.as_deref() == Some(node.id.as_str())
                && input.selector.input_key == input_key
        }) {
            self.provider_builder.error = Some("Input already exposed.".to_string());
            return;
        }
        let (name, label) = provider_input_name_and_label(
            node.title.as_deref(),
            input_key,
            &self.provider_builder.inputs,
        );
        let schema = self.provider_builder.input_schema(node, input_key);
        self.provider_builder
            .inputs
            .push(ProviderBuilderInput::from_node(
                node, input_key, name, label, schema,
            ));
        self.provider_builder.error = None;
    }

    pub(super) fn apply_provider_input_action(&mut self, action: ProviderInputAction) {
        match action {
            ProviderInputAction::MoveUp(index) => {
                if index > 0 && index < self.provider_builder.inputs.len() {
                    self.provider_builder.inputs.swap(index - 1, index);
                }
            }
            ProviderInputAction::MoveDown(index) => {
                if index + 1 < self.provider_builder.inputs.len() {
                    self.provider_builder.inputs.swap(index, index + 1);
                }
            }
            ProviderInputAction::Delete(index) => {
                if index < self.provider_builder.inputs.len() {
                    self.provider_builder.inputs.remove(index);
                }
            }
        }
    }

    pub(super) fn save_provider_builder(&mut self) {
        let save = match self.provider_builder.build_save_payload() {
            Ok(save) => save,
            Err(err) => {
                self.provider_builder.error = Some(err);
                return;
            }
        };

        let manifest_json = match serde_json::to_string_pretty(&save.manifest) {
            Ok(json) => json,
            Err(err) => {
                self.provider_builder.error = Some(format!("Failed to serialize manifest: {err}"));
                return;
            }
        };
        if let Some(parent) = save.manifest_path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                self.provider_builder.error =
                    Some(format!("Failed to create manifest folder: {err}"));
                return;
            }
        }
        if let Err(err) = std::fs::write(&save.manifest_path, manifest_json) {
            self.provider_builder.error = Some(format!("Failed to write manifest: {err}"));
            return;
        }

        let provider_json = match serde_json::to_string_pretty(&save.entry) {
            Ok(json) => json,
            Err(err) => {
                self.provider_builder.error = Some(format!("Failed to serialize provider: {err}"));
                return;
            }
        };
        if let Err(err) =
            crate::core::provider_store::write_provider_file(&save.provider_path, &provider_json)
        {
            self.provider_builder.error = Some(format!("Failed to save provider: {err}"));
            return;
        }

        self.provider_builder.source_path = Some(save.provider_path.clone());
        self.provider_builder.manifest_path = Some(save.manifest_path);
        self.provider_builder.error = None;
        self.selected_provider_file = Some(save.provider_path.clone());
        self.refresh_provider_files();
        self.provider_builder_open = false;
        self.editor.status = format!("Saved provider {}", path_label(&save.provider_path));
    }
}
