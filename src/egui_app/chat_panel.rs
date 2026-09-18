use super::*;
use crate::core::agent_chat::{self, ChatEvent, ChatRequest, ToolResult};
use crate::core::agent_context::{self, ContextUsage};
use crate::state::AgentProviderEntry;
use serde_json::{json, Value};

#[derive(Default)]
pub(super) struct ChatRow {
    pub label: String,
    pub agent_name: Option<String>,
    pub text: String,
    pub tool: bool,
    pub image: Option<egui::TextureHandle>,
    pub summary: String,
    pub failed: bool,
}

fn composer_field(
    ui: &mut Ui,
    text: &mut String,
    composer_id: &mut Option<egui::Id>,
    rows: usize,
) -> bool {
    let enter_pressed = composer_id
            .is_some_and(|id| ui.memory(|memory| memory.has_focus(id)))
            && ui.input_mut(|input| {
                let mut send = false;
                input.events.retain(|event| {
                    if matches!(event, egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. } if modifiers.is_none()) {
                        send = true;
                        false
                    } else {
                        true
                    }
                });
                send
            });
    let composer_response = kit::multiline_text_field(
        ui,
        text,
        ui.available_width(),
        kit::MultilineTextFieldOptions { rows },
    );
    *composer_id = Some(composer_response.id);
    enter_pressed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_enter_sends_but_shift_enter_keeps_a_newline() {
        let ctx = Context::default();
        kit::configure_style(&ctx);
        let mut text = String::from("Two lines");
        let mut id = None;
        let render = |text: &mut String, id: &mut Option<egui::Id>, events| {
            let mut send = false;
            let _ = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.set_max_size(Vec2::new(420.0, 700.0));
                    send = composer_field(ui, text, id, 2);
                },
            );
            send
        };
        let enter = |modifiers| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        render(&mut text, &mut id, vec![]);
        assert!(
            !render(&mut text, &mut id, vec![enter(egui::Modifiers::NONE)]),
            "Enter outside the composer must not send"
        );
        ctx.memory_mut(|memory| memory.request_focus(id.unwrap()));
        assert!(
            !render(&mut text, &mut id, vec![enter(egui::Modifiers::SHIFT)]),
            "Shift+Enter must not send"
        );
        assert_eq!(text.matches('\n').count(), 1);
        let before = text.clone();
        assert!(render(
            &mut text,
            &mut id,
            vec![enter(egui::Modifiers::NONE)]
        ));
        assert_eq!(text, before, "Sending must not insert another newline");
    }
}

pub(super) struct ChatUi {
    pub providers: Vec<AgentProviderEntry>,
    pub draft: Option<AgentProviderEntry>,
    pub settings: super::agent_settings::AgentSettingsUi,
    pub provider_status: Option<kit::OperationPresentation>,
    pub test: Option<(ChatRequest, Instant)>,
    pub selected: Option<Uuid>,
    pub request: Option<ChatRequest>,
    pub rows: Vec<ChatRow>,
    pub messages: Vec<Value>,
    pub composer: String,
    pub composer_id: Option<egui::Id>,
    pub session: u64,
    pub stopping: bool,
    pub context: ContextUsage,
    pub context_owner: String,
    pub compacting: bool,
    pub handles: crate::core::agent_tools::Handles,
    pub media: Option<super::chat_tools::PendingChatMedia>,
}

impl Default for ChatUi {
    fn default() -> Self {
        Self {
            providers: crate::core::agent_provider_store::load(),
            draft: None,
            settings: Default::default(),
            provider_status: None,
            test: None,
            selected: None,
            request: None,
            rows: vec![],
            messages: vec![json!({"role":"system", "content":agent_chat::SYSTEM_PROMPT})],
            composer: String::new(),
            composer_id: None,
            session: u64::MAX,
            stopping: false,
            context: Default::default(),
            context_owner: String::new(),
            compacting: false,
            handles: Default::default(),
            media: None,
        }
    }
}

fn context_wheel(ui: &mut Ui, context: ContextUsage) {
    let fraction = context
        .tokens
        .zip(context.limit)
        .filter(|(_, max)| *max > 0)
        .map(|(n, max)| (n as f32 / max as f32).clamp(0.0, 1.0));
    let label = fraction
        .map(|f| format!("{:.0}%", f * 100.0))
        .unwrap_or_else(|| "-".into());
    let color = if context.full {
        kit::DANGER
    } else {
        kit::PRIMARY
    };
    let response = kit::icon_button_sized(ui, "", Vec2::splat(32.0));
    let center = response.rect.center();
    ui.painter()
        .circle_stroke(center, 7.0, egui::Stroke::new(2.0_f32, kit::TEXT_MUTED));
    if let Some(fraction) = fraction.filter(|f| *f > 0.0) {
        let points = (0..=48)
            .map(|i| {
                let angle = -std::f32::consts::FRAC_PI_2
                    + std::f32::consts::TAU * fraction * i as f32 / 48.0;
                center + Vec2::angled(angle) * 7.0
            })
            .collect();
        ui.painter()
            .add(egui::Shape::line(points, egui::Stroke::new(2.0_f32, color)));
    }
    let count = context
        .tokens
        .map(format_token_count)
        .unwrap_or_else(|| "unknown".into());
    let limit = context
        .limit
        .map(format_token_count)
        .unwrap_or_else(|| "unknown".into());
    let detail = if context.compacted {
        "Compacted by OpenAI. Awaiting the next usage report."
    } else if context.preflight {
        "Counted prompt, including tools and media. Space is reserved for the reply."
    } else {
        "Last reported input + output tokens, including cached input and reasoning. Excludes unsent text."
    };
    let tooltip = format!("Context used: {label}\n{count} / {limit} tokens\n{detail}");
    let tooltip_width = (ui.ctx().content_rect().width() - 48.0).clamp(120.0, 280.0);
    crate::core::automation::instrument_response(
        response,
        "context_usage",
        Some(tooltip.clone()),
        false,
        false,
    )
    .on_hover_ui(|ui| {
        ui.set_max_width(tooltip_width);
        ui.add(egui::Label::new(tooltip).wrap());
    });
}

fn format_token_count(tokens: u64) -> String {
    let digits = tokens.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}

const CHAT_PANEL_W: f32 = 460.0;

fn tool_error(result: &str) -> Option<String> {
    serde_json::from_str::<Value>(result)
        .ok()?
        .get("error")?
        .as_str()
        .map(str::to_owned)
}

fn tool_summary(name: &str, arguments: &str, result: &str) -> String {
    if let Some(error) = tool_error(result) {
        return error;
    }
    let args: Value = serde_json::from_str(arguments).unwrap_or_default();
    let data: Value = serde_json::from_str(result).unwrap_or_default();
    let subject = data["asset"]["name"]
        .as_str()
        .or(data["name"].as_str())
        .or(data["asset"]["asset"]["name"].as_str())
        .or(args["name"].as_str())
        .or(args["source"]
            .as_str()
            .or(args["asset"].as_str())
            .or(args["handle"].as_str()))
        .unwrap_or("project");
    let action = args["action"].as_str().unwrap_or_default();
    match name {
        "project_context" => "Read project overview".into(),
        "inspect" => format!("Inspected {subject}"),
        "timeline_edit" => match action {
            "place" => format!("Placed {subject} on the timeline"),
            "move" => format!("Moved {subject} on the timeline"),
            "resize" => format!("Resized {subject} on the timeline"),
            "add_track" => "Added a timeline track".into(),
            _ => "Updated the timeline".into(),
        },
        "asset_edit" => match action {
            "import" => format!("Imported {subject}"),
            "rename" => format!("Renamed {subject}"),
            "create_generative" => format!("Created {subject}"),
            "extract_still" => format!("Extracted a still from {subject}"),
            _ => format!("Updated {subject}"),
        },
        "generation" => match action {
            "configure" => format!("Configured generation for {subject}"),
            "start" => format!("Started generation for {subject}"),
            "status" => format!("Checked generation for {subject}"),
            "activate_version" => format!("Activated a version for {subject}"),
            _ => format!("Updated generation for {subject}"),
        },
        "save_project" => "Saved project".into(),
        "look" => format!("Reviewed {subject}"),
        "watch_video" => format!("Reviewed video from {subject}"),
        _ => format!("Completed {name}"),
    }
}

fn paint_chat_surface_hover(ui: &Ui, response: &egui::Response) {
    if response.hovered() {
        ui.painter().rect_filled(
            response.rect,
            egui::CornerRadius::same(8),
            Color32::from_rgba_unmultiplied(255, 255, 255, 6),
        );
        ui.painter().rect_stroke(
            response.rect,
            egui::CornerRadius::same(8),
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(82, 88, 99, 112)),
            egui::StrokeKind::Inside,
        );
    }
}

fn paint_chat_accent(ui: &Ui, rect: Rect, accent: Color32) {
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + 1.0, rect.top() + 8.0),
            Pos2::new(rect.left() + 1.0, rect.bottom() - 8.0),
        ],
        Stroke::new(2.0_f32, accent),
    );
}

// A small presentation layer for streamed prose; wire messages remain untouched.
fn chat_text(text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for (line_index, line) in text.lines().enumerate() {
        if line_index > 0 {
            job.append("\n", 0.0, egui::TextFormat::default());
        }
        let heading = line.starts_with("# ") || line.starts_with("## ") || line.starts_with("### ");
        let line = if heading {
            line.trim_start_matches('#').trim_start()
        } else {
            line
        };
        let bullet = line.strip_prefix("- ").or_else(|| line.strip_prefix("* "));
        let owned;
        let mut remaining = if let Some(body) = bullet {
            owned = format!("• {body}");
            owned.as_str()
        } else {
            line
        };
        let mut strong = heading;
        let mut code = false;
        while !remaining.is_empty() {
            if remaining.starts_with("**") && !code {
                strong = !strong;
                remaining = &remaining[2..];
                continue;
            }
            if remaining.starts_with('`') {
                code = !code;
                remaining = &remaining[1..];
                continue;
            }
            let end = remaining
                .char_indices()
                .skip(1)
                .find(|(i, c)| *c == '`' || (!code && remaining[*i..].starts_with("**")))
                .map(|(i, _)| i)
                .unwrap_or(remaining.len());
            let size = if heading { 17.0 } else { 14.0 };
            job.append(
                &remaining[..end],
                0.0,
                egui::TextFormat {
                    font_id: if code {
                        egui::FontId::monospace(13.0)
                    } else {
                        egui::FontId::proportional(size)
                    },
                    color: if strong {
                        egui::Color32::WHITE
                    } else {
                        kit::TEXT
                    },
                    background: if code {
                        kit::PANEL_RAISED
                    } else {
                        egui::Color32::TRANSPARENT
                    },
                    extra_letter_spacing: if strong { 0.25 } else { 0.0 },
                    line_height: Some(20.0),
                    ..Default::default()
                },
            );
            remaining = &remaining[end..];
        }
    }
    job
}

impl LatentSlateApp {
    pub(super) fn clear_chat(&mut self) {
        self.chat.request = None;
        self.chat.rows.clear();
        self.chat.messages = vec![json!({"role":"system", "content":agent_chat::SYSTEM_PROMPT})];
        self.chat.composer.clear();
        self.chat.composer_id = None;
        self.chat.stopping = false;
        self.chat.context = Default::default();
        self.chat.context_owner.clear();
        self.chat.compacting = false;
        self.chat.handles = Default::default();
        self.chat.media = None;
    }

    pub(super) fn poll_chat(&mut self, ctx: &Context) {
        self.poll_agent_settings(ctx);
        self.poll_magic_prompt(ctx);
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
        if let Some(pending) = self.chat.media.as_ref() {
            if let Ok((result, image)) = pending.receiver.try_recv() {
                let mut pending = self.chat.media.take().unwrap();
                self.chat.rows[pending.row]
                    .text
                    .push_str(&format!("\n{}", result.text));
                self.chat.rows[pending.row].failed = tool_error(&result.text).is_some();
                if let Some(error) = tool_error(&result.text) {
                    self.chat.rows[pending.row].summary = error;
                }
                self.chat.rows[pending.row].image = image.map(|im| {
                    ctx.load_texture(
                        format!("chat_capture_{}_{}", self.chat.session, pending.row),
                        egui::ColorImage::from_rgba_unmultiplied(
                            [im.width() as usize, im.height() as usize],
                            im.as_raw(),
                        ),
                        egui::TextureOptions::LINEAR,
                    )
                });
                if let Some(reply) = pending.reply.take() {
                    let _ = reply.send(result);
                }
            }
        }
        loop {
            let event = self
                .chat
                .test
                .as_ref()
                .and_then(|(request, _)| request.events.try_recv().ok());
            match event {
                Some(ChatEvent::Finished { error, .. }) => {
                    self.chat.provider_status = Some(match error {
                        Some(error) => kit::OperationPresentation::new(
                            kit::OperationPhase::Failed,
                            kit::OperationSeverity::Warning,
                            "Agent connection test failed",
                        )
                        .detail(error),
                        None => kit::OperationPresentation::new(
                            kit::OperationPhase::Succeeded,
                            kit::OperationSeverity::Success,
                            "Connection and streaming verified",
                        ),
                    });
                    self.chat.test = None;
                }
                Some(ChatEvent::Tool { reply, .. }) => {
                    let _ = reply.send(ToolResult::error("No tools available in connection test."));
                }
                Some(ChatEvent::Text(_)) => {
                    self.chat.provider_status = Some(kit::OperationPresentation::new(
                        kit::OperationPhase::Running,
                        kit::OperationSeverity::Neutral,
                        "Agent is responding",
                    ));
                }
                Some(ChatEvent::Context(_) | ChatEvent::Compacting(_)) => {}
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
                Some(ChatEvent::Context(mut context)) => {
                    if context.tokens.is_none() && context.preflight && !context.full {
                        context.tokens = self.chat.context.tokens;
                        context.compacted = self.chat.context.compacted;
                        context.preflight = self.chat.context.preflight;
                    }
                    self.chat.context = context;
                }
                Some(ChatEvent::Compacting(active)) => self.chat.compacting = active,
                Some(ChatEvent::Text(text)) => {
                    if self
                        .chat
                        .rows
                        .last()
                        .is_none_or(|r| r.tool || r.label != "Assistant")
                    {
                        self.chat.rows.push(ChatRow {
                            label: "Assistant".into(),
                            agent_name: self
                                .chat
                                .providers
                                .iter()
                                .find(|p| Some(p.id) == self.chat.selected)
                                .map(|p| p.name.clone()),
                            text: String::new(),
                            tool: false,
                            image: None,
                            ..Default::default()
                        });
                    }
                    self.chat.rows.last_mut().unwrap().text.push_str(&text);
                }
                Some(ChatEvent::Tool { call, reply }) => {
                    let prepared = if self.chat.stopping {
                        ToolResult::error("Stopped.").into()
                    } else {
                        self.execute_chat_tool(ctx, &call)
                            .unwrap_or_else(|e| ToolResult::error(&e).into())
                    };
                    let super::chat_tools::ChatToolResult { result, video } = prepared;
                    let row = self.chat.rows.len();
                    self.chat.rows.push(ChatRow {
                        summary: tool_summary(
                            &call.function.name,
                            &call.function.arguments,
                            &result.text,
                        ),
                        failed: tool_error(&result.text).is_some(),
                        label: call.function.name,
                        text: format!("{}\n{}", call.function.arguments, result.text),
                        tool: true,
                        image: None,
                        agent_name: None,
                    });
                    if result.media.is_empty() {
                        let _ = reply.send(result);
                    } else {
                        let (sender, receiver) = std::sync::mpsc::channel();
                        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                        let worker_cancel = cancel.clone();
                        std::thread::spawn(move || {
                            let result = match video.as_ref() {
                                Some(video) => match super::chat_tools::prepare_video(
                                    video,
                                    worker_cancel.clone(),
                                ) {
                                    Ok(()) => result,
                                    Err(error) => ToolResult::error(&error),
                                },
                                None => result,
                            };
                            let image = result
                                .media
                                .first()
                                .and_then(|m| m.get("host_image_path"))
                                .and_then(Value::as_str)
                                .and_then(|p| image::open(p).ok())
                                .map(|im| im.thumbnail(2048, 2048).into_rgba8());
                            let result = if worker_cancel.load(std::sync::atomic::Ordering::Relaxed)
                            {
                                ToolResult::error("Stopped.")
                            } else {
                                super::chat_tools::encode_media(result)
                            };
                            if let Some(video) = video {
                                let _ = std::fs::remove_file(video.output);
                            }
                            let _ = sender.send((result, image));
                        });
                        self.chat.media = Some(super::chat_tools::PendingChatMedia {
                            receiver,
                            reply: Some(reply),
                            row,
                            cancel,
                        });
                    }
                }
                Some(ChatEvent::Finished { messages, error }) => {
                    self.chat.messages = messages;
                    self.chat.request = None;
                    self.chat.compacting = false;
                    self.chat.stopping = false;
                    self.chat.media = None;
                    if let Some(error) = error {
                        self.chat.rows.push(ChatRow {
                            label: "Chat".into(),
                            text: error,
                            tool: false,
                            image: None,
                            ..Default::default()
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
        ui.horizontal(|ui| {
            ui.set_min_height(kit::ICON_BUTTON_H);
            ui.label(kit::section_label("CHAT"));
        });
        ui.add_space(8.0);
        ui.add_enabled_ui(!busy, |ui| {
            let provider = self
                .chat
                .providers
                .iter()
                .find(|p| Some(p.id) == self.chat.selected);
            let selected = provider
                .map(|p| p.name.as_str())
                .unwrap_or("Choose agent provider");
            let mut indicators = Vec::new();
            if let Some(provider) = provider {
                for (enabled, label, color) in [
                    (provider.capabilities.image_input, "Images", kit::IMAGE),
                    (
                        provider.capabilities.video_input
                            && provider.connection.supports_native_video(),
                        "Video",
                        kit::VIDEO,
                    ),
                ] {
                    if enabled {
                        indicators.push((
                            ui.painter().layout_no_wrap(
                                label.into(),
                                egui::FontId::proportional(10.0),
                                color,
                            ),
                            color,
                        ));
                    }
                }
            }
            let indicator_width = indicators
                .iter()
                .map(|(galley, _)| galley.size().x + 8.0)
                .sum();
            kit::combo_field_with_trailing(
                ui,
                "chat_provider",
                selected,
                ui.available_width().max(100.0) - 8.0,
                indicator_width,
                |ui, rect| {
                    let mut x = rect.left();
                    for (galley, color) in &indicators {
                        ui.painter().galley(
                            Pos2::new(x, rect.center().y - galley.mesh_bounds.center().y),
                            galley.clone(),
                            *color,
                        );
                        x += galley.size().x + 8.0;
                    }
                },
                |ui| {
                    for p in self.chat.providers.iter().filter(|p| p.enabled) {
                        automation_selectable_value(
                            ui,
                            &mut self.chat.selected,
                            Some(p.id),
                            &p.name,
                        );
                    }
                },
            );
        });
        let provider = self
            .chat
            .providers
            .iter()
            .find(|p| Some(p.id) == self.chat.selected);
        let owner = provider
            .map(crate::core::agent_responses::history_owner)
            .unwrap_or_default();
        if owner != self.chat.context_owner {
            self.chat.context = ContextUsage {
                limit: provider
                    .filter(|p| {
                        matches!(p.connection, crate::state::AgentConnection::OpenAi { .. })
                    })
                    .map(|_| agent_context::OPENAI_CEILING),
                ..Default::default()
            };
            self.chat.context_owner = owner;
        }
        if self.chat.selected.is_none() {
            ui.label("Add an agent in AI Providers to start chatting.");
            if kit::secondary_button(ui, "AI Providers", 120.0).clicked() {
                self.editor.overlays.providers = true;
            }
        }
        ui.add_space(8.0);
        ui.separator();
        const COMPOSER_MAX_ROWS: usize = 6;
        let composer_rows = ui
            .painter()
            .layout(
                self.chat.composer.clone(),
                egui::FontId::proportional(kit::FIELD_TEXT_SIZE),
                kit::TEXT,
                (ui.available_width() - 2.0 * kit::FIELD_INNER_MARGIN_X as f32).max(1.0),
            )
            .rows
            .len()
            .clamp(1, COMPOSER_MAX_ROWS);
        let composer_height = kit::multiline_text_field_height(composer_rows);
        let transcript_height = (ui.available_height() - 94.0 - composer_height).max(28.0);
        egui::ScrollArea::vertical()
            .id_salt("chat_transcript")
            .stick_to_bottom(true)
            .max_height(transcript_height)
            .min_scrolled_height(transcript_height)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(transcript_height);
                for (index, row) in self.chat.rows.iter().enumerate() {
                    if row.tool {
                        let surface = egui::Frame::new()
                            .fill(kit::PANEL)
                            .corner_radius(8)
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.set_width((ui.available_width()).max(1.0));
                                let running =
                                    self.chat.media.as_ref().is_some_and(|p| p.row == index);
                                let details_id = ui.make_persistent_id(("chat_tool", index));
                                let mut details = egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    details_id,
                                    false,
                                );
                                let status = if running {
                                    "Running"
                                } else if row.failed {
                                    "Error"
                                } else {
                                    "Done"
                                };
                                let status_color = if row.failed {
                                    kit::DANGER
                                } else {
                                    kit::TEXT_MUTED
                                };
                                let mut toggle_details = false;
                                kit::bounded_horizontal_row(ui, 20.0, |ui, row_width| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    let details_width = 66.0;
                                    let separator_width = 10.0;
                                    let status_width = ui
                                        .painter()
                                        .layout_no_wrap(
                                            status.into(),
                                            egui::FontId::proportional(10.5),
                                            status_color,
                                        )
                                        .size()
                                        .x;
                                    let max_summary_width = (row_width
                                        - details_width
                                        - separator_width
                                        - status_width
                                        - ui.spacing().item_spacing.x * 4.0)
                                        .max(0.0);
                                    let summary_width = ui
                                        .painter()
                                        .layout_no_wrap(
                                            row.summary.clone(),
                                            egui::FontId::proportional(12.0),
                                            kit::TEXT_MUTED,
                                        )
                                        .size()
                                        .x
                                        .min(max_summary_width);
                                    let spacer_width = (row_width
                                        - summary_width
                                        - separator_width
                                        - status_width
                                        - details_width
                                        - ui.spacing().item_spacing.x * 4.0)
                                        .max(0.0);
                                    let summary = ui.add_sized(
                                        [summary_width, 20.0],
                                        egui::Label::new(
                                            egui::RichText::new(&row.summary)
                                                .size(12.0)
                                                .color(kit::TEXT_MUTED),
                                        )
                                        .halign(Align::Min)
                                        .truncate(),
                                    );
                                    summary.on_hover_text(&row.summary);
                                    ui.add_sized(
                                        [separator_width, 20.0],
                                        egui::Label::new(
                                            egui::RichText::new("·")
                                                .size(10.5)
                                                .color(kit::TEXT_DIM),
                                        )
                                        .halign(Align::Center),
                                    );
                                    ui.add_sized(
                                        [status_width, 20.0],
                                        egui::Label::new(
                                            egui::RichText::new(status)
                                                .size(10.5)
                                                .color(status_color),
                                        )
                                        .halign(Align::Min),
                                    );
                                    ui.allocate_exact_size(
                                        Vec2::new(spacer_width, 20.0),
                                        Sense::hover(),
                                    );
                                    let details_label = if details.is_open() {
                                        "▾ Details"
                                    } else {
                                        "▸ Details"
                                    };
                                    let trigger = automation_button(
                                        ui.add_sized(
                                            [details_width, 20.0],
                                            egui::Button::new(
                                                egui::RichText::new(details_label)
                                                    .size(10.5)
                                                    .color(kit::TEXT_MUTED),
                                            )
                                            .frame(false),
                                        ),
                                        &format!("Chat tool {index}: {}", row.label),
                                    );
                                    toggle_details = trigger.clicked();
                                });
                                if toggle_details {
                                    details.toggle(ui);
                                    details.store(ui.ctx());
                                }
                                if details.is_open() {
                                    ui.add_space(8.0);
                                    if let Some(texture) = &row.image {
                                        let size = texture.size_vec2();
                                        let scale = (ui.available_width() / size.x).min(1.0);
                                        ui.add(
                                            egui::Image::new(texture)
                                                .fit_to_exact_size(size * scale),
                                        );
                                        ui.add_space(8.0);
                                    }
                                    crate::core::automation::instrument_response(
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&row.text)
                                                    .size(10.0)
                                                    .color(kit::TEXT_MUTED)
                                                    .monospace(),
                                            )
                                            .wrap()
                                            .selectable(true),
                                        ),
                                        "chat_tool_result",
                                        Some(row.text.clone()),
                                        false,
                                        false,
                                    );
                                }
                            });
                        paint_chat_surface_hover(ui, &surface.response);
                        paint_chat_accent(
                            ui,
                            surface.response.rect,
                            Color32::from_rgb(71, 137, 201),
                        );
                    } else {
                        let user = row.label == "You";
                        let assistant = row.label == "Assistant";
                        let row_width = ui.available_width();
                        let bubble_width =
                            (row_width - if user { 32.0 } else { 20.0 }).clamp(32.0, 704.0);
                        let accent = egui::Color32::from_rgb(66, 112, 86);
                        let response = ui
                            .horizontal(|ui| {
                                if user {
                                    ui.add_space(row_width - bubble_width);
                                }
                                egui::Frame::new()
                                    .fill(if user { kit::PANEL_RAISED } else { kit::PANEL })
                                    .corner_radius(8)
                                    .inner_margin(12)
                                    .show(ui, |ui| {
                                        ui.with_layout(
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                ui.set_width(bubble_width - 24.0);
                                                ui.label(
                                                    egui::RichText::new(
                                                        row.agent_name
                                                            .as_deref()
                                                            .unwrap_or(&row.label),
                                                    )
                                                    .size(11.0)
                                                    .color(kit::TEXT_MUTED),
                                                );
                                                ui.add_space(5.0);
                                                ui.add(
                                                    egui::Label::new(chat_text(&row.text)).wrap(),
                                                );
                                            },
                                        );
                                    })
                            })
                            .inner;
                        if assistant {
                            paint_chat_surface_hover(ui, &response.response);
                            paint_chat_accent(ui, response.response.rect, accent);
                        }
                        crate::core::automation::instrument_response(
                            response.response,
                            "chat_message",
                            Some(row.text.clone()),
                            false,
                            false,
                        );
                    }
                    ui.add_space(10.0);
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            if busy {
                ui.spinner();
            }
            ui.label(kit::caption(if self.chat.stopping {
                "Stopping…"
            } else if self.chat.compacting {
                "Compacting context... You can keep typing."
            } else if self.chat.context.full {
                agent_context::FULL_MESSAGE
            } else if self.chat.media.is_some() {
                "Preparing media for the assistant…"
            } else if busy {
                "Waiting for the assistant…"
            } else {
                "Enter to send · Shift+Enter for a newline"
            }));
        });
        let enter_pressed = composer_field(
            ui,
            &mut self.chat.composer,
            &mut self.chat.composer_id,
            composer_rows,
        );
        let can_send = !busy
            && !self.chat.context.full
            && self.chat.selected.is_some()
            && !self.chat.composer.trim().is_empty();
        let row_width = ui.available_width();
        let spacing = ui.spacing().item_spacing.x;
        let send_width =
            (row_width - 64.0 - 2.0 * spacing - if busy { 65.0 + spacing } else { 0.0 }).max(32.0);
        ui.horizontal(|ui| {
            let new_chat = ui
                .add_enabled_ui(!busy, |ui| {
                    kit::icon_button_sized(ui, "+", Vec2::splat(32.0))
                })
                .inner;
            if automation_button(new_chat, "New Chat")
                .on_hover_text("New Chat")
                .clicked()
            {
                self.clear_chat();
            }
            if (ui
                .add_enabled_ui(can_send, |ui| kit::primary_button(ui, "Send", send_width))
                .inner
                .clicked()
                || enter_pressed)
                && can_send
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
                        image: None,
                        ..Default::default()
                    });
                    self.chat.request = Some(agent_chat::start(
                        provider.clone(),
                        self.chat.messages.clone(),
                        crate::core::agent_tools::schemas(&provider),
                    ));
                    ui.ctx().request_repaint();
                }
            }
            if busy && kit::secondary_button(ui, "Stop", 65.0).clicked() {
                self.chat.stopping = true;
                if let Some(media) = &self.chat.media {
                    media
                        .cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if let Some(request) = &self.chat.request {
                    request.stop();
                }
            }
            context_wheel(ui, self.chat.context);
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
            .with_min_inner_size([360.0, 440.0])
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
                            (main_outer.height() - frame_height).max(440.0),
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
