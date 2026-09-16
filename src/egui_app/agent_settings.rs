use super::*;
use crate::core::{
    agent_chat,
    agent_openai::{self, AgentModel, SettingsAction, SettingsEvent, SettingsRequest},
};
use crate::state::{AgentConnection, AgentProtocol, AgentProviderEntry, OpenAiAuth};
use serde_json::json;

#[derive(Default)]
pub(super) struct AgentSettingsUi {
    request: Option<SettingsRequest>,
    models: Vec<AgentModel>,
    account: String,
    login_url: Option<String>,
}

fn status(ok: bool, text: impl Into<String>) -> kit::OperationPresentation {
    kit::OperationPresentation::new(
        if ok {
            kit::OperationPhase::Succeeded
        } else {
            kit::OperationPhase::Failed
        },
        if ok {
            kit::OperationSeverity::Success
        } else {
            kit::OperationSeverity::Error
        },
        text,
    )
}

fn capabilities(draft: &mut AgentProviderEntry, models: &[AgentModel]) {
    if let Some(model) = models.iter().find(|m| m.id == draft.connection.model()) {
        if let Some(image) = model.image {
            draft.capabilities.image_input = image;
        }
        if let Some(video) = model.video {
            draft.capabilities.video_input = video;
        }
    }
    draft.capabilities.video_input &= draft.connection.supports_native_video();
}

fn password(ui: &mut Ui, key: &mut Option<String>, label: &str) {
    kit::field_label(ui, label);
    let value = key.get_or_insert_default();
    let mut response = ui.add(
        egui::TextEdit::singleline(&mut *value)
            .password(true)
            .desired_width(f32::INFINITY),
    );
    crate::core::automation::apply_pending_text(&mut response, value);
    crate::core::automation::instrument_response(
        response,
        "password_field",
        Some(label.into()),
        true,
        true,
    );
}

impl LatentSlateApp {
    pub(super) fn create_agent_provider(&mut self, openai: bool) {
        let mut provider = AgentProviderEntry::default();
        if openai {
            provider.name = "OpenAI Agent".into();
            provider.connection = AgentConnection::OpenAi {
                model: String::new(),
                auth: OpenAiAuth::ChatGpt,
                api_key: None,
            };
        }
        match crate::core::agent_provider_store::save(&provider) {
            Ok(()) => {
                self.selected_provider = Some(ProviderModalSelection::Agent(provider.id));
                self.chat.settings = AgentSettingsUi {
                    account: agent_openai::account_label(provider.id),
                    ..Default::default()
                };
                self.chat.draft = Some(provider.clone());
                self.chat.providers.push(provider);
                self.chat.provider_status = None;
                self.chat.test = None;
            }
            Err(_) => self.editor.status = "Unable to save agent provider.".into(),
        }
    }

    pub(super) fn poll_agent_settings(&mut self, ctx: &Context) {
        loop {
            let event = self
                .chat
                .settings
                .request
                .as_ref()
                .and_then(|r| r.events.try_recv().ok());
            match event {
                Some(SettingsEvent::Browser(url)) => {
                    ctx.open_url(egui::OpenUrl::new_tab(&url));
                    self.chat.settings.login_url = Some(url);
                    self.chat.provider_status = Some(kit::OperationPresentation::new(
                        kit::OperationPhase::Waiting,
                        kit::OperationSeverity::Neutral,
                        "Complete sign-in in your browser",
                    ));
                }
                Some(SettingsEvent::Finished(result)) => {
                    self.chat.settings.request = None;
                    self.chat.settings.login_url = None;
                    if matches!(&result, Ok(None))
                        && self
                            .chat
                            .draft
                            .as_ref()
                            .is_some_and(|p| Some(p.id) == self.chat.selected)
                    {
                        self.clear_chat();
                    }
                    if let Some(draft) = &mut self.chat.draft {
                        self.chat.settings.account = agent_openai::account_label(draft.id);
                        self.chat.provider_status = Some(match result {
                            Ok(Some(models)) => {
                                self.chat.settings.models = models;
                                capabilities(draft, &self.chat.settings.models);
                                status(true, "Models refreshed. Choose a model and save the agent.")
                            }
                            Ok(None) => status(true, &self.chat.settings.account),
                            Err(error) => status(false, error),
                        });
                    }
                }
                None => break,
            }
        }
        if self.chat.settings.request.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    pub(super) fn agent_provider_inspector(&mut self, ui: &mut Ui, id: Uuid) {
        if self.chat.draft.as_ref().is_none_or(|p| p.id != id) {
            self.chat.draft = self.chat.providers.iter().find(|p| p.id == id).cloned();
            self.chat.provider_status = None;
            self.chat.test = None;
            self.chat.settings = AgentSettingsUi {
                account: agent_openai::account_label(id),
                ..Default::default()
            };
        }
        let Some(draft) = self.chat.draft.as_mut() else {
            return;
        };
        let settings = &mut self.chat.settings;
        let busy =
            self.chat.test.is_some() || settings.request.is_some() || self.chat.request.is_some();
        ui.label(kit::section_label(
            if matches!(draft.connection, AgentConnection::OpenAi { .. }) {
                "OpenAI Agent"
            } else {
                "OpenAI-compatible Agent"
            },
        ));
        ui.add_space(kit::FORM_ROW_GAP);
        let mut action = None;
        let mut save = false;
        let mut test = false;
        let mut delete = false;
        kit::scroll_body(ui, |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                kit::labeled_text_field(ui, "Agent name", &mut draft.name);
                let mut before = draft.connection.clone();
                *before.model_mut() = String::new();
                match &mut draft.connection {
                    AgentConnection::OpenAiCompatible { base_url, protocol, api_key, .. } => {
                        kit::labeled_text_field(ui, "Endpoint / Base URL (including /v1)", base_url);
                        kit::field_label(ui, "API format");
                        kit::combo_field(ui, "agent_protocol", if *protocol == AgentProtocol::Responses { "Responses" } else { "Chat Completions" }, ui.available_width(), |ui| {
                            automation_selectable_value(ui, protocol, AgentProtocol::Responses, "Responses");
                            automation_selectable_value(ui, protocol, AgentProtocol::ChatCompletions, "Chat Completions");
                        });
                        password(ui, api_key, "API / Bearer key (optional)");
                        ui.label(kit::caption("Leave blank for anonymous local endpoints."));
                    }
                    AgentConnection::OpenAi { auth, api_key, .. } => {
                        kit::field_label(ui, "Connection");
                        kit::combo_field(ui, "openai_auth", if *auth == OpenAiAuth::ChatGpt { "ChatGPT subscription" } else { "OpenAI API key" }, ui.available_width(), |ui| {
                            automation_selectable_value(ui, auth, OpenAiAuth::ChatGpt, "ChatGPT subscription");
                            automation_selectable_value(ui, auth, OpenAiAuth::ApiKey, "OpenAI API key");
                        });
                        if *auth == OpenAiAuth::ApiKey {
                            password(ui, api_key, "OpenAI API key");
                            ui.label(kit::caption("Uses Responses. Billed separately through OpenAI Platform."));
                        } else {
                            ui.label(kit::caption(&settings.account));
                            ui.horizontal_wrapped(|ui| {
                                if kit::secondary_button(ui, "Sign in with ChatGPT", 170.0).clicked() { action = Some(SettingsAction::Login); }
                                if kit::secondary_button(ui, "Sign out", 85.0).clicked() { action = Some(SettingsAction::Logout); }
                            });
                            ui.label(kit::caption("Uses your Codex subscription limits. No automatic API-key fallback."));
                        }
                    }
                }
                let mut after = draft.connection.clone();
                *after.model_mut() = String::new();
                if before != after { settings.models.clear(); }
                kit::labeled_text_field(ui, "Agent model", draft.connection.model_mut());
                ui.horizontal_wrapped(|ui| {
                    if kit::secondary_button(ui, "Refresh models", 125.0).clicked() { action = Some(SettingsAction::Models); }
                });
                if !settings.models.is_empty() {
                    kit::combo_field(ui, "agent_available_models", "Choose an available model", ui.available_width(), |ui| {
                        for model in &settings.models {
                            automation_selectable_value(ui, draft.connection.model_mut(), model.id.clone(), &model.label);
                        }
                    });
                }
                capabilities(draft, &settings.models);
                let model = settings.models.iter().find(|m| m.id == draft.connection.model());
                automation_checkbox(ui, &mut draft.enabled, "Enabled");
                ui.add_enabled_ui(model.is_none_or(|m| m.image.is_none()), |ui| {
                    automation_checkbox(ui, &mut draft.capabilities.image_input, "Image understanding");
                });
                ui.add_enabled_ui(draft.connection.supports_native_video() && model.is_none_or(|m| m.video.is_none()), |ui| {
                    automation_checkbox(ui, &mut draft.capabilities.video_input, "Video understanding");
                });
                if !draft.connection.supports_native_video() {
                    ui.label(kit::caption(if matches!(draft.connection, AgentConnection::OpenAi { .. }) {
                        "OpenAI agents do not accept native video. Image understanding can inspect frames and contact sheets."
                    } else { "Native llama.cpp video requires Chat Completions. Responses supports text, images and tools." }));
                } else if draft.capabilities.video_input {
                    ui.label(kit::caption("Requires native input_video support. Silent proxies: up to 12 seconds, 320×320. Server controls sampling FPS."));
                }
                ui.label(kit::caption(if model.is_some_and(|m| m.image.is_some() || m.video.is_some()) {
                    "Reported model capabilities are read-only. Unreported capabilities can be set manually."
                } else { "This connection has not reported model capabilities. Set image/video support only when known." }));
            });
            if settings.request.is_some() {
                ui.horizontal_wrapped(|ui| {
                    if kit::secondary_button(ui, "Cancel connection", 150.0).clicked() {
                        settings.request = None;
                        settings.login_url = None;
                        self.chat.provider_status = None;
                    }
                    if let Some(url) = &settings.login_url {
                        if kit::secondary_button(ui, "Open sign-in page", 145.0).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        }
                    }
                });
            }
            ui.add_space(kit::FORM_ROW_GAP);
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    save = kit::primary_button(ui, "Save agent", 100.0).clicked();
                    test = kit::secondary_button(ui, "Test agent", 100.0).clicked();
                    delete = kit::secondary_button(ui, "Delete agent", 100.0).clicked();
                });
                if self.chat.test.is_some()
                    && kit::secondary_button(ui, "Cancel test", 100.0).clicked()
                {
                    self.chat.test = None;
                    self.chat.provider_status = None;
                }
            });
            if let Some(status) = &mut self.chat.provider_status {
                if let Some((_, started)) = &self.chat.test {
                    status.detail = Some(format!("{}s elapsed. Cold model loads can take a minute or more; the request allows up to 15 minutes.", started.elapsed().as_secs()));
                }
                ui.add_space(kit::FORM_ROW_GAP);
                kit::operation_banner(ui, "agent_provider_status", status);
            }
        });
        if let Some(action) = action {
            settings.request = Some(agent_openai::start_settings(draft.clone(), action));
            self.chat.provider_status = Some(kit::OperationPresentation::new(
                kit::OperationPhase::Waiting,
                kit::OperationSeverity::Neutral,
                "Connecting…",
            ));
        }
        if save {
            self.chat.provider_status =
                Some(match crate::core::agent_provider_store::save(draft) {
                    Ok(()) => {
                        self.chat.providers = crate::core::agent_provider_store::load();
                        status(true, "Agent saved")
                    }
                    Err(_) => status(
                        false,
                        "Unable to save agent. Previous settings remain active.",
                    ),
                });
        }
        if test {
            self.chat.test = Some((
                agent_chat::start(
                    draft.clone(),
                    vec![
                        json!({"role":"user", "content":"Reply briefly to confirm this connection works."}),
                    ],
                    vec![],
                ),
                Instant::now(),
            ));
            self.chat.provider_status = Some(kit::OperationPresentation::new(
                kit::OperationPhase::Waiting,
                kit::OperationSeverity::Neutral,
                "Waiting for agent response",
            ));
        }
        if delete {
            self.chat.provider_status = Some(
                match agent_openai::forget_account(id).and_then(|_| {
                    crate::core::agent_provider_store::delete(id)
                        .map_err(|_| "Unable to delete agent provider.".into())
                }) {
                    Ok(()) => {
                        self.chat.providers.retain(|p| p.id != id);
                        self.selected_provider = None;
                        status(true, "Agent deleted")
                    }
                    Err(error) => status(false, error),
                },
            );
        }
    }
}
