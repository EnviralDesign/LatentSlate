use super::*;

impl LatentSlateApp {
    pub(super) fn poll_automation(&mut self, ctx: &Context) {
        if !crate::core::automation::is_enabled() {
            return;
        }
        while let Some(envelope) = crate::core::automation::try_recv_command() {
            match envelope.command.clone() {
                crate::core::automation::AutomationCommand::GetUi => {
                    envelope.respond(crate::core::automation::AutomationResponse::ok(
                        serde_json::json!({
                            "elements": crate::core::automation::ui_snapshot(),
                        }),
                    ));
                }
                crate::core::automation::AutomationCommand::ClickUi { id } => {
                    self.queue_automation_click(ctx, envelope, id);
                }
                crate::core::automation::AutomationCommand::TextUi { id, text, replace } => {
                    self.queue_automation_text(ctx, envelope, id, text, replace);
                }
                crate::core::automation::AutomationCommand::Screenshot { name } => {
                    self.queue_automation_screenshot(ctx, envelope, name.as_deref());
                }
                crate::core::automation::AutomationCommand::GetPerformanceDiagnostics => {
                    envelope.respond(self.performance_diagnostics_response());
                }
                crate::core::automation::AutomationCommand::ScrubTimelineProfile {
                    start_time,
                    end_time,
                    steps,
                    repeats,
                    scrub_audio,
                    settle_ms,
                } => {
                    let response = self.run_automation_scrub_profile(
                        ctx,
                        start_time,
                        end_time,
                        steps,
                        repeats,
                        scrub_audio,
                        settle_ms,
                    );
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::OpenExportVideo => {
                    self.open_export_modal();
                    envelope.respond(crate::core::automation::AutomationResponse::empty_ok());
                }
                crate::core::automation::AutomationCommand::CloseExportVideo => {
                    self.close_or_cancel_export_modal();
                    envelope.respond(crate::core::automation::AutomationResponse::empty_ok());
                }
                _ => {
                    let previous_project_path = self.editor.project.project_path.clone();
                    let response = self.editor.apply_automation_command(&envelope.command);
                    self.project_settings = self.editor.project.settings.clone();
                    if self.editor.project.project_path != previous_project_path {
                        self.clear_project_runtime_cache();
                        self.warm_audio_playback_cache();
                    }
                    envelope.respond(response);
                }
            }
        }
    }

    pub(super) fn queue_automation_click(
        &mut self,
        ctx: &Context,
        envelope: crate::core::automation::AutomationEnvelope,
        id: String,
    ) {
        let Some(element) = crate::core::automation::find_ui_element(&id) else {
            envelope.respond(crate::core::automation::AutomationResponse::not_found(
                format!("No visible UI element with id {id}. Refresh /ui and try again."),
            ));
            return;
        };
        if !element.enabled {
            envelope.respond(crate::core::automation::AutomationResponse::conflict(
                format!("UI element {id} is visible but disabled."),
            ));
            return;
        }
        if !element.clickable {
            envelope.respond(crate::core::automation::AutomationResponse::conflict(
                format!("UI element {id} is not clickable."),
            ));
            return;
        }

        crate::core::automation::queue_ui_click(id.clone());
        self.pending_automation_ui_actions
            .push(PendingAutomationUiAction {
                id,
                action: "click",
                envelope,
            });
        ctx.request_repaint();
    }

    pub(super) fn queue_automation_text(
        &mut self,
        ctx: &Context,
        envelope: crate::core::automation::AutomationEnvelope,
        id: String,
        text: String,
        replace: bool,
    ) {
        let Some(element) = crate::core::automation::find_ui_element(&id) else {
            envelope.respond(crate::core::automation::AutomationResponse::not_found(
                format!("No visible UI element with id {id}. Refresh /ui and try again."),
            ));
            return;
        };
        if !element.enabled {
            envelope.respond(crate::core::automation::AutomationResponse::conflict(
                format!("UI element {id} is visible but disabled."),
            ));
            return;
        }
        if !element.editable {
            envelope.respond(crate::core::automation::AutomationResponse::conflict(
                format!("UI element {id} is not editable."),
            ));
            return;
        }

        crate::core::automation::queue_ui_text(id.clone(), text, replace);
        self.pending_automation_ui_actions
            .push(PendingAutomationUiAction {
                id,
                action: "text",
                envelope,
            });
        ctx.request_repaint();
    }

    pub(super) fn queue_automation_screenshot(
        &mut self,
        ctx: &Context,
        envelope: crate::core::automation::AutomationEnvelope,
        name: Option<&str>,
    ) {
        if self.pending_automation_screenshot.is_some() {
            envelope.respond(crate::core::automation::AutomationResponse::conflict(
                "A screenshot request is already pending.",
            ));
            return;
        }

        let path = match crate::core::automation::screenshot_path(name) {
            Ok(path) => path,
            Err(err) => {
                envelope.respond(crate::core::automation::AutomationResponse::with_status(
                    err, 500,
                ));
                return;
            }
        };
        self.pending_automation_screenshot = Some(PendingAutomationScreenshot {
            path,
            requested_at: Instant::now(),
            envelope,
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
            "automation_screenshot".to_string(),
        )));
        ctx.request_repaint();
    }

    pub(super) fn performance_diagnostics_response(
        &self,
    ) -> crate::core::automation::AutomationResponse {
        let samples: Vec<PreviewPerfSample> = self.preview_perf_samples.iter().cloned().collect();
        let recent_summary = summarize_preview_perf_samples(&samples);
        crate::core::automation::AutomationResponse::ok(serde_json::json!({
            "current_time": self.editor.current_time,
            "is_playing": self.editor.is_playing,
            "preview_dirty": self.editor.preview_dirty,
            "project_loaded": self.editor.project.project_path.is_some(),
            "latest_stats": self.preview_stats.clone(),
            "cache": self.editor.previewer.cache_stats(),
            "render": {
                "in_flight": self.preview_render_in_flight.load(Ordering::Relaxed),
                "latest_request_id": self.preview_render_request_id.load(Ordering::Relaxed),
                "busy_ms": self.preview_render_busy_since.map(|start| start.elapsed().as_secs_f64() * 1000.0),
                "completed": self.preview_render_completed_count,
                "stale": self.preview_render_stale_count,
                "last_worker_ms": self.preview_render_last_worker_ms,
                "last_delivery_ms": self.preview_render_last_delivery_ms,
            },
            "recent_summary": recent_summary,
            "recent_samples": samples,
        }))
    }

    pub(super) fn run_automation_scrub_profile(
        &mut self,
        ctx: &Context,
        start_time: f64,
        end_time: f64,
        steps: usize,
        repeats: usize,
        scrub_audio: bool,
        settle_ms: u64,
    ) -> crate::core::automation::AutomationResponse {
        if self.editor.project.project_path.is_none() {
            return crate::core::automation::AutomationResponse::conflict(
                "Open a project before running a scrub profile.",
            );
        }

        let duration = self.editor.project.duration();
        let start_time = start_time.clamp(0.0, duration);
        let end_time = end_time.clamp(0.0, duration);
        let steps = steps.clamp(1, AUTOMATION_SCRUB_MAX_STEPS);
        let repeats = repeats.clamp(1, AUTOMATION_SCRUB_MAX_REPEATS);
        let settle_ms = settle_ms.min(500);
        let was_playing = self.editor.is_playing;
        if scrub_audio {
            self.timeline_scrub_was_playing = was_playing;
            self.timeline_last_scrub_audio_time = None;
        }

        let profile_start = Instant::now();
        let mut samples = Vec::with_capacity(steps.saturating_mul(repeats));
        for repeat in 0..repeats {
            for step in 0..steps {
                let alpha = if steps <= 1 {
                    0.0
                } else {
                    step as f64 / (steps - 1) as f64
                };
                let requested_time = start_time + (end_time - start_time) * alpha;
                let seek_start = Instant::now();
                self.seek_editor(requested_time, scrub_audio);
                let seek_ms = seek_start.elapsed().as_secs_f64() * 1000.0;

                let render_start = Instant::now();
                let stats = self.render_preview_sync_for_profile(ctx);
                let render_wall_ms = render_start.elapsed().as_secs_f64() * 1000.0;

                samples.push(ScrubProfileSample {
                    repeat,
                    step,
                    requested_time,
                    actual_time: self.editor.current_time,
                    seek_ms,
                    render_wall_ms,
                    stats,
                });

                if settle_ms > 0 {
                    std::thread::sleep(Duration::from_millis(settle_ms));
                }
            }
        }

        if scrub_audio {
            self.timeline_scrub_was_playing = was_playing;
            self.finish_timeline_scrub();
        }

        ctx.request_repaint();
        let summary = summarize_scrub_profile_samples(&samples);
        crate::core::automation::AutomationResponse::ok(serde_json::json!({
            "profile": {
                "start_time": start_time,
                "end_time": end_time,
                "steps": steps,
                "repeats": repeats,
                "scrub_audio": scrub_audio,
                "settle_ms": settle_ms,
                "wall_ms": profile_start.elapsed().as_secs_f64() * 1000.0,
                "summary": summary,
                "samples": samples,
                "cache": self.editor.previewer.cache_stats(),
            }
        }))
    }

    pub(super) fn keep_automation_responsive(&self, ctx: &Context) {
        if crate::core::automation::is_enabled() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    pub(super) fn handle_automation_screenshot_events(&mut self, ctx: &Context) {
        if self.pending_automation_screenshot.is_none() {
            return;
        }

        let screenshot = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                _ => None,
            })
        });

        if let Some(image) = screenshot {
            let pending = self
                .pending_automation_screenshot
                .take()
                .expect("pending screenshot checked above");
            match save_color_image_png(&pending.path, &image) {
                Ok(()) => {
                    pending
                        .envelope
                        .respond(crate::core::automation::AutomationResponse::ok(
                            serde_json::json!({ "path": pending.path }),
                        ))
                }
                Err(err) => pending.envelope.respond(
                    crate::core::automation::AutomationResponse::with_status(err, 500),
                ),
            }
            return;
        }

        let expired = self
            .pending_automation_screenshot
            .as_ref()
            .map(|pending| pending.requested_at.elapsed() > Duration::from_secs(18))
            .unwrap_or(false);
        if expired {
            let pending = self
                .pending_automation_screenshot
                .take()
                .expect("pending screenshot checked above");
            pending
                .envelope
                .respond(crate::core::automation::AutomationResponse::with_status(
                    "Timed out waiting for eframe screenshot event.",
                    500,
                ));
        }
    }

    pub(super) fn finish_automation_ui_actions(&mut self) {
        if self.pending_automation_ui_actions.is_empty() {
            return;
        }

        let pending = std::mem::take(&mut self.pending_automation_ui_actions);
        for action in pending {
            if crate::core::automation::was_action_consumed(&action.id) {
                action
                    .envelope
                    .respond(crate::core::automation::AutomationResponse::ok(
                        serde_json::json!({
                            "id": action.id,
                            "action": action.action,
                        }),
                    ));
            } else {
                crate::core::automation::clear_pending_ui_action(&action.id);
                action.envelope.respond(
                    crate::core::automation::AutomationResponse::conflict(format!(
                        "UI element {} was visible in the previous frame but did not consume the queued {} action. It may have disappeared or is not instrumented yet.",
                        action.id, action.action
                    )),
                );
            }
        }
    }
}
