use super::*;
use crate::core::agent_chat::{self, ChatEvent, ChatRequest, ToolResult};
use crate::state::{AgentConnection, AgentProviderEntry};
use serde_json::{json, Value};

pub(super) struct ChatRow {
    pub label: String,
    pub text: String,
    pub tool: bool,
}

pub(super) struct ChatUi {
    pub providers: Vec<AgentProviderEntry>,
    pub draft: Option<AgentProviderEntry>,
    pub provider_status: String,
    pub test: Option<ChatRequest>,
    pub selected: Option<Uuid>,
    pub request: Option<ChatRequest>,
    pub rows: Vec<ChatRow>,
    pub messages: Vec<Value>,
    pub composer: String,
    pub session: u64,
    pub stopping: bool,
}

impl Default for ChatUi {
    fn default() -> Self {
        Self {
            providers: crate::core::agent_provider_store::load(),
            draft: None,
            provider_status: String::new(),
            test: None,
            selected: None,
            request: None,
            rows: vec![],
            messages: vec![json!({"role":"system", "content":agent_chat::SYSTEM_PROMPT})],
            composer: String::new(),
            session: u64::MAX,
            stopping: false,
        }
    }
}

const CHAT_PANEL_W: f32 = 380.0;

impl LatentSlateApp {
    pub(super) fn create_agent_provider(&mut self) {
        let provider = AgentProviderEntry::default();
        match crate::core::agent_provider_store::save(&provider) {
            Ok(()) => {
                self.selected_provider = Some(ProviderModalSelection::Agent(provider.id));
                self.chat.draft = Some(provider.clone());
                self.chat.providers.push(provider);
                self.chat.provider_status.clear();
            }
            Err(_) => self.editor.status = "Unable to save agent provider.".into(),
        }
    }

    pub(super) fn agent_provider_inspector(&mut self, ui: &mut Ui, id: Uuid) {
        if self.chat.draft.as_ref().is_none_or(|p| p.id != id) {
            self.chat.draft = self.chat.providers.iter().find(|p| p.id == id).cloned();
            self.chat.provider_status.clear();
        }
        let Some(draft) = self.chat.draft.as_mut() else {
            return;
        };
        ui.label(kit::section_label("OpenAI-compatible Agent"));
        ui.add_space(kit::FORM_ROW_GAP);
        kit::scroll_body(ui, |ui| {
            kit::labeled_text_field(ui, "Agent name", &mut draft.name);
            let AgentConnection::OpenAiCompatible {
                base_url,
                model,
                api_key,
            } = &mut draft.connection;
            kit::labeled_text_field(ui, "Endpoint / Base URL (including /v1)", base_url);
            kit::labeled_text_field(ui, "Agent model", model);
            kit::field_label(ui, "API / Bearer key (optional)");
            let mut key = api_key.clone().unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut key)
                        .password(true)
                        .desired_width(f32::INFINITY),
                )
                .changed()
            {
                *api_key = (!key.trim().is_empty()).then_some(key);
            }
            ui.label(kit::caption("Leave blank for anonymous local endpoints."));
            automation_checkbox(ui, &mut draft.enabled, "Enabled");
            automation_checkbox(
                ui,
                &mut draft.capabilities.image_input,
                "Image understanding",
            );
            automation_checkbox(
                ui,
                &mut draft.capabilities.video_input,
                "Video understanding",
            );
            if draft.capabilities.video_input {
                ui.label(kit::caption(
                    "Requires native input_video support (llama.cpp).",
                ));
            }
            ui.add_space(kit::FORM_ROW_GAP);
            let mut save = false;
            let mut test = false;
            let mut delete = false;
            ui.horizontal_wrapped(|ui| {
                save = kit::primary_button(ui, "Save agent", 100.0).clicked();
                test = automation_button(
                    ui.add_enabled(self.chat.test.is_none(), egui::Button::new("Test agent")),
                    "Test agent",
                )
                .clicked();
                delete = kit::secondary_button(ui, "Delete agent", 100.0).clicked();
            });
            if save {
                match crate::core::agent_provider_store::save(draft) {
                    Ok(()) => {
                        self.chat.providers = crate::core::agent_provider_store::load();
                        self.chat.provider_status = "Agent saved.".into();
                    }
                    Err(_) => {
                        self.chat.provider_status =
                            "Unable to save agent. Previous settings remain active.".into()
                    }
                }
            }
            if test {
                self.chat.test = Some(agent_chat::start(
                    draft.clone(),
                    vec![
                        json!({"role":"user", "content":"Reply briefly to confirm this connection works."}),
                    ],
                    vec![],
                ));
                self.chat.provider_status = "Testing connection…".into();
            }
            if delete {
                match crate::core::agent_provider_store::delete(id) {
                    Ok(()) => {
                        self.chat.providers.retain(|p| p.id != id);
                        self.selected_provider = None;
                    }
                    Err(_) => self.chat.provider_status = "Unable to delete agent provider.".into(),
                }
            }
            if !self.chat.provider_status.is_empty() {
                ui.label(&self.chat.provider_status);
            }
        });
    }

    fn clear_chat(&mut self) {
        self.chat.request = None;
        self.chat.rows.clear();
        self.chat.messages = vec![json!({"role":"system", "content":agent_chat::SYSTEM_PROMPT})];
        self.chat.composer.clear();
        self.chat.stopping = false;
    }

    pub(super) fn poll_chat(&mut self, ctx: &Context) {
        if self.chat.session != self.editor.project_session_revision {
            self.clear_chat();
            self.chat.session = self.editor.project_session_revision;
            self.chat.selected = self.editor.layout.agent_provider;
        }
        if self
            .chat
            .selected
            .is_none_or(|id| !self.chat.providers.iter().any(|p| p.id == id && p.enabled))
        {
            self.chat.selected = self.chat.providers.iter().find(|p| p.enabled).map(|p| p.id);
        }
        self.editor.layout.agent_provider = self.chat.selected;
        loop {
            let event = self
                .chat
                .test
                .as_ref()
                .and_then(|r| r.events.try_recv().ok());
            match event {
                Some(ChatEvent::Finished { error, .. }) => {
                    self.chat.provider_status =
                        error.unwrap_or_else(|| "Connection and streaming verified.".into());
                    self.chat.test = None;
                }
                Some(ChatEvent::Tool { reply, .. }) => {
                    let _ = reply.send(ToolResult::error("No tools available in connection test."));
                }
                Some(ChatEvent::Text(_)) => {}
                None => break,
            }
        }
        loop {
            let event = self
                .chat
                .request
                .as_ref()
                .and_then(|r| r.events.try_recv().ok());
            match event {
                Some(ChatEvent::Text(text)) => {
                    if self
                        .chat
                        .rows
                        .last()
                        .is_none_or(|r| r.tool || r.label != "Assistant")
                    {
                        self.chat.rows.push(ChatRow {
                            label: "Assistant".into(),
                            text: String::new(),
                            tool: false,
                        });
                    }
                    self.chat.rows.last_mut().unwrap().text.push_str(&text);
                }
                Some(ChatEvent::Tool { call, reply }) => {
                    let result = ToolResult::error(if self.chat.stopping {
                        "Stopped."
                    } else {
                        "Tool unavailable."
                    });
                    self.chat.rows.push(ChatRow {
                        label: call.function.name,
                        text: result.text.clone(),
                        tool: true,
                    });
                    let _ = reply.send(result);
                }
                Some(ChatEvent::Finished { messages, error }) => {
                    self.chat.messages = messages;
                    self.chat.request = None;
                    self.chat.stopping = false;
                    if let Some(error) = error {
                        self.chat.rows.push(ChatRow {
                            label: "Chat".into(),
                            text: error,
                            tool: false,
                        });
                    }
                }
                None => break,
            }
        }
        if self.chat.request.is_some() || self.chat.test.is_some() {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    fn chat_contents(&mut self, ui: &mut Ui) {
        let busy = self.chat.request.is_some();
        ui.add_enabled_ui(!busy, |ui| {
            let selected = self
                .chat
                .providers
                .iter()
                .find(|p| Some(p.id) == self.chat.selected)
                .map(|p| p.name.as_str())
                .unwrap_or("Choose agent provider");
            egui::ComboBox::from_id_salt("chat_provider")
                .selected_text(selected)
                .width(ui.available_width().max(100.0) - 8.0)
                .show_ui(ui, |ui| {
                    for p in self.chat.providers.iter().filter(|p| p.enabled) {
                        automation_selectable_value(
                            ui,
                            &mut self.chat.selected,
                            Some(p.id),
                            &p.name,
                        );
                    }
                });
        });
        if self.chat.selected.is_none() {
            ui.label("Add an OpenAI-compatible Agent in AI Providers to start chatting.");
            if kit::secondary_button(ui, "AI Providers", 120.0).clicked() {
                self.editor.overlays.providers = true;
            }
        }
        let transcript_height = (ui.available_height() - 114.0).max(28.0);
        egui::ScrollArea::vertical()
            .id_salt("chat_transcript")
            .stick_to_bottom(true)
            .max_height(transcript_height)
            .min_scrolled_height(transcript_height)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (index, row) in self.chat.rows.iter().enumerate() {
                    if row.tool {
                        egui::CollapsingHeader::new(&row.label)
                            .id_salt(("chat_tool", index))
                            .show(ui, |ui| {
                                ui.add(egui::Label::new(&row.text).wrap());
                            });
                    } else {
                        ui.label(kit::section_label(&row.label));
                        ui.add(egui::Label::new(&row.text).wrap());
                        ui.add_space(8.0);
                    }
                }
                if busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(if self.chat.stopping {
                            "Stopping…"
                        } else {
                            "Working…"
                        });
                    });
                }
            });
        kit::multiline_text_field(
            ui,
            &mut self.chat.composer,
            ui.available_width(),
            kit::MultilineTextFieldOptions { rows: 2 },
        );
        ui.horizontal(|ui| {
            if busy {
                if kit::secondary_button(ui, "Stop", 65.0).clicked() {
                    self.chat.stopping = true;
                    if let Some(request) = &self.chat.request {
                        request.stop();
                    }
                }
            } else if automation_button(
                ui.add_enabled(
                    self.chat.selected.is_some() && !self.chat.composer.trim().is_empty(),
                    egui::Button::new("Send"),
                ),
                "Send",
            )
            .clicked()
            {
                if let Some(provider) = self
                    .chat
                    .providers
                    .iter()
                    .find(|p| Some(p.id) == self.chat.selected && p.enabled)
                    .cloned()
                {
                    let text = std::mem::take(&mut self.chat.composer);
                    self.chat
                        .messages
                        .push(json!({"role":"user", "content":text}));
                    self.chat.rows.push(ChatRow {
                        label: "You".into(),
                        text,
                        tool: false,
                    });
                    self.chat.request = Some(agent_chat::start(
                        provider,
                        self.chat.messages.clone(),
                        vec![],
                    ));
                    ui.ctx().request_repaint();
                }
            }
            if automation_button(
                ui.add_enabled(!busy, egui::Button::new("New Chat")),
                "New Chat",
            )
            .clicked()
            {
                self.clear_chat();
            }
        });
    }

    pub(super) fn chat_panel(&mut self, ctx: &Context) {
        let viewport_id =
            egui::ViewportId::from_hash_of(("chat_window", self.editor.project_session_revision));
        let main_rects =
            ctx.input(|input| input.viewport().outer_rect.zip(input.viewport().inner_rect));
        let saved = self.editor.layout.chat_window;
        let mut builder = egui::ViewportBuilder::default()
            .with_title("LatentSlate Chat")
            .with_min_inner_size([300.0, 240.0])
            .with_resizable(true);
        if !ctx.input(|input| input.raw.viewports.contains_key(&viewport_id)) {
            if let Some(placement) = saved {
                builder = builder
                    .with_position(placement.outer_position)
                    .with_inner_size(placement.inner_size);
            } else if let Some((outer, inner)) = main_rects {
                builder = builder
                    .with_position([outer.right() + 6.0, outer.top()])
                    .with_inner_size([CHAT_PANEL_W, inner.height()]);
            } else {
                builder = builder.with_inner_size([CHAT_PANEL_W, 600.0]);
            }
        }
        ctx.show_viewport_immediate(viewport_id, builder, |ui, _class| {
            let placement = ui.input(|input| {
                let viewport = input.viewport();
                if viewport.minimized == Some(true) {
                    return None;
                }
                let (outer, inner) = viewport.outer_rect.zip(viewport.inner_rect)?;
                Some((outer, inner))
            });
            if let Some((outer, inner)) = placement {
                let mut placement = crate::state::ChatWindowPlacement {
                    outer_position: [outer.min.x, outer.min.y],
                    inner_size: [inner.width(), inner.height()],
                };
                if saved.is_none() {
                    if let Some((main_outer, _)) = main_rects {
                        // Match outer edges using the child window's actual native frame height.
                        let frame_height = outer.height() - inner.height();
                        let size = Vec2::new(
                            CHAT_PANEL_W,
                            (main_outer.height() - frame_height).max(240.0),
                        );
                        let position = Pos2::new(main_outer.right() + 6.0, main_outer.top());
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                        placement.outer_position = [position.x, position.y];
                        placement.inner_size = [size.x, size.y];
                    }
                }
                self.editor.layout.chat_window = Some(placement);
            }
            if ui.input(|input| input.viewport().close_requested()) {
                self.editor.overlays.chat = false;
            }
            egui::Frame::new()
                .fill(kit::PANEL)
                .inner_margin(egui::Margin::same(16))
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    self.chat_contents(ui);
                });
        });
    }
}
