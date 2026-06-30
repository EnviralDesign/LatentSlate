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
                crate::core::automation::AutomationCommand::ExtractStillToAsset {
                    source,
                    time,
                    name,
                } => {
                    let response = self.extract_agent_still_to_asset(source, time, name);
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::Capture { request } => {
                    let response = self.run_agent_capture(ctx, request);
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::GetPerformanceDiagnostics => {
                    envelope.respond(self.performance_diagnostics_response());
                }
                crate::core::automation::AutomationCommand::Seek { time } => {
                    self.seek_editor(time, false);
                    envelope.respond(crate::core::automation::AutomationResponse::ok(
                        serde_json::json!({ "current_time": self.editor.current_time }),
                    ));
                }
                crate::core::automation::AutomationCommand::SetPlayback { playing } => {
                    if self.editor.is_playing != playing {
                        self.toggle_playback();
                    }
                    envelope.respond(crate::core::automation::AutomationResponse::ok(
                        serde_json::json!({
                            "is_playing": self.editor.is_playing,
                            "current_time": self.editor.current_time,
                        }),
                    ));
                }
                crate::core::automation::AutomationCommand::StepTimeline { frames } => {
                    let fps = self.editor.project.settings.fps.max(1.0);
                    let duration = self.editor.project.duration().max(0.0);
                    let duration_frame = (duration * fps).round() as i64;
                    let current_frame = (self.editor.current_time * fps).round() as i64;
                    let next_frame = (current_frame + frames).clamp(0, duration_frame);
                    self.seek_editor(next_frame as f64 / fps, false);
                    envelope.respond(crate::core::automation::AutomationResponse::ok(
                        serde_json::json!({
                            "current_time": self.editor.current_time,
                            "frame": next_frame,
                            "fps": fps,
                        }),
                    ));
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
                crate::core::automation::AutomationCommand::CreateI2iFromClip {
                    clip_id,
                    provider_id,
                } => {
                    let response = self.agent_continuation_response(|this| {
                        this.create_i2i_from_single_clip(clip_id, provider_id);
                    });
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::CreateI2vFromClip {
                    clip_id,
                    reference,
                    provider_id,
                } => {
                    let reference = match reference {
                        crate::core::automation::I2vReference::Image => SingleI2VReference::Image,
                        crate::core::automation::I2vReference::VideoFirstFrame => {
                            SingleI2VReference::VideoFirstFrame
                        }
                        crate::core::automation::I2vReference::VideoLastFrame => {
                            SingleI2VReference::VideoLastFrame
                        }
                    };
                    let response = self.agent_continuation_response(|this| {
                        this.create_i2v_from_single_clip(clip_id, reference, provider_id);
                    });
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::CreateBridgeFromClips {
                    clip_ids,
                    provider_id,
                } => {
                    let timeline_bridge_provider_id = provider_id
                        .and_then(|id| {
                            self.editor
                                .provider_entries
                                .iter()
                                .find(|provider| provider.id == id)
                                .filter(|provider| {
                                    crate::core::timeline_bridge::provider_is_timeline_bridge(
                                        provider,
                                    ) && provider.output_type == ProviderOutputType::Video
                                })
                                .map(|provider| provider.id)
                        })
                        .or_else(|| {
                            if provider_id.is_some() {
                                return None;
                            }
                            let candidates: Vec<Uuid> = self
                                .editor
                                .provider_entries
                                .iter()
                                .filter(|provider| {
                                    crate::core::timeline_bridge::provider_is_timeline_bridge(
                                        provider,
                                    ) && provider.output_type == ProviderOutputType::Video
                                        && self.editor.provider_in_project_scope(provider.id)
                                })
                                .map(|provider| provider.id)
                                .collect();
                            (candidates.len() == 1).then(|| candidates[0])
                        });
                    let response = self.agent_continuation_response(|this| {
                        if let Some(provider_id) = timeline_bridge_provider_id {
                            let clips: Vec<Clip> = this
                                .editor
                                .project
                                .clips
                                .iter()
                                .filter(|clip| clip_ids.contains(&clip.id))
                                .cloned()
                                .collect();
                            this.create_timeline_bridge_from_selected_clips(&clips, provider_id);
                        } else {
                            this.create_bridge_video_from_clip_ids(&clip_ids, provider_id);
                        }
                    });
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
                crate::core::automation::AutomationCommand::ExportVideo { request } => {
                    let response = self.start_agent_export_video(request);
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::GetExportStatus => {
                    envelope.respond(self.agent_export_status_response());
                }
                crate::core::automation::AutomationCommand::CancelExport => {
                    let had_export = self.export_cancel.is_some();
                    self.close_or_cancel_export_modal();
                    envelope.respond(crate::core::automation::AutomationResponse::ok(
                        serde_json::json!({
                            "cancel_requested": had_export,
                            "export": self.agent_export_status_json(),
                        }),
                    ));
                }
                crate::core::automation::AutomationCommand::TestProvider { provider_id, live } => {
                    let response = self.test_agent_provider(provider_id, live);
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::SetActiveGenerationVersion {
                    asset_id,
                    version,
                } => match self.set_generative_active_version(asset_id, &version) {
                    Ok(()) => envelope.respond(crate::core::automation::AutomationResponse::ok(
                        serde_json::json!({
                            "asset_id": asset_id,
                            "version": version,
                            "status": self.editor.status,
                        }),
                    )),
                    Err(err) => envelope.respond(agent_error_response(err)),
                },
                crate::core::automation::AutomationCommand::DuplicateGenerationVersion {
                    asset_id,
                    version,
                } => match self.duplicate_generative_version(asset_id, &version) {
                    Ok(new_version) => envelope.respond(
                        crate::core::automation::AutomationResponse::ok(serde_json::json!({
                            "asset_id": asset_id,
                            "source_version": version,
                            "version": new_version,
                            "status": self.editor.status,
                        })),
                    ),
                    Err(err) => envelope.respond(agent_error_response(err)),
                },
                crate::core::automation::AutomationCommand::DeleteGenerationVersion {
                    asset_id,
                    version,
                } => match self.delete_generative_version(asset_id, &version) {
                    Ok(()) => envelope.respond(crate::core::automation::AutomationResponse::ok(
                        serde_json::json!({
                            "asset_id": asset_id,
                            "version": version,
                            "status": self.editor.status,
                        }),
                    )),
                    Err(err) => envelope.respond(agent_error_response(err)),
                },
                crate::core::automation::AutomationCommand::GetAssetLabGraph { asset_id } => {
                    envelope.respond(self.agent_asset_lab_graph_response(asset_id));
                }
                crate::core::automation::AutomationCommand::AddAssetLabNode {
                    asset_id,
                    provider_id,
                    parent_node_id,
                    inputs,
                } => {
                    let response = self.add_agent_asset_lab_node(
                        asset_id,
                        provider_id,
                        parent_node_id,
                        inputs,
                    );
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::SetAssetLabNode {
                    asset_id,
                    node_id,
                    patch,
                } => {
                    let response = self.set_agent_asset_lab_node(asset_id, node_id, patch);
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::DeleteAssetLabNode {
                    asset_id,
                    node_id,
                } => match self.delete_asset_lab_node(asset_id, node_id) {
                    Ok(()) => envelope.respond(self.agent_asset_lab_graph_response(asset_id)),
                    Err(err) => envelope.respond(agent_error_response(err)),
                },
                crate::core::automation::AutomationCommand::GenerateAssetLabNode {
                    asset_id,
                    node_id,
                } => {
                    let before: std::collections::HashSet<Uuid> = self
                        .editor
                        .generation_queue
                        .iter()
                        .map(|job| job.id)
                        .collect();
                    self.generate_asset_lab_node(asset_id, node_id);
                    let jobs: Vec<_> = self
                        .editor
                        .generation_queue
                        .iter()
                        .filter(|job| !before.contains(&job.id))
                        .cloned()
                        .collect();
                    let response = if jobs.is_empty() {
                        crate::core::automation::AutomationResponse::error(
                            self.editor.status.clone(),
                        )
                    } else {
                        crate::core::automation::AutomationResponse::ok(serde_json::json!({
                            "jobs": crate::editor::compact_generation_jobs_json(&jobs),
                            "asset_lab": self.agent_asset_lab_graph_json(asset_id),
                            "status": self.editor.status,
                        }))
                    };
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::StartGeneration {
                    asset_id,
                    context_clip_id,
                    wait,
                } => {
                    let before: std::collections::HashSet<Uuid> = self
                        .editor
                        .generation_queue
                        .iter()
                        .map(|job| job.id)
                        .collect();
                    self.start_generative_generation(asset_id, context_clip_id);
                    let jobs: Vec<_> = self
                        .editor
                        .generation_queue
                        .iter()
                        .filter(|job| !before.contains(&job.id))
                        .cloned()
                        .collect();
                    let ok = !jobs.is_empty();
                    let response = if ok {
                        crate::core::automation::AutomationResponse::ok(serde_json::json!({
                            "jobs": crate::editor::compact_generation_jobs_json(&jobs),
                            "status": self.editor.status,
                            "wait_requested": wait,
                        }))
                    } else {
                        crate::core::automation::AutomationResponse::error(
                            self.editor.status.clone(),
                        )
                    };
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::CancelJob { job_id } => {
                    let response = match self.cancel_generation_job(job_id) {
                        CancelGenerationJobResult::Cancelled { label, was_running } => {
                            crate::core::automation::AutomationResponse::ok(serde_json::json!({
                                "job_id": job_id,
                                "label": label,
                                "cancelled": true,
                                "was_running": was_running,
                                "status": self.editor.status,
                            }))
                        }
                        CancelGenerationJobResult::NotFound => {
                            crate::core::automation::AutomationResponse::not_found(
                                "Generation job not found.",
                            )
                        }
                        CancelGenerationJobResult::NotCancellable { status } => {
                            crate::core::automation::AutomationResponse::conflict(format!(
                                "Generation job is already {:?}.",
                                status
                            ))
                        }
                    };
                    envelope.respond(response);
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

    pub(super) fn start_agent_export_video(
        &mut self,
        request: crate::core::automation::ExportVideoRequest,
    ) -> crate::core::automation::AutomationResponse {
        if self.export_cancel.is_some() {
            return crate::core::automation::AutomationResponse::conflict(
                "A video export is already running.",
            );
        }
        if self.editor.project.project_path.is_none() {
            return crate::core::automation::AutomationResponse::conflict(
                "Open or create a saved project before exporting.",
            );
        }

        self.export_modal = ExportModalState::for_project(&self.editor.project);
        self.export_preview_texture = None;
        if let Some(output_path) = request.output_path {
            self.export_modal.output_path = ensure_mp4_extension(output_path).display().to_string();
        }
        if let Some(codec) = request.codec {
            self.export_modal.codec = codec;
        }
        if let Some(width) = request.width {
            self.export_modal.width = width.to_string();
        }
        if let Some(height) = request.height {
            self.export_modal.height = height.to_string();
        }
        if let Some(fps) = request.fps {
            self.export_modal.fps = format_agent_export_number(fps);
        }
        if let Some(start_seconds) = request.start_seconds {
            self.export_modal.start_seconds = format_agent_export_number(start_seconds);
        }
        if let Some(duration_seconds) = request.duration_seconds {
            self.export_modal.duration_seconds = format_agent_export_number(duration_seconds);
        }
        if let Some(include_audio) = request.include_audio {
            self.export_modal.include_audio = include_audio;
        }
        if let Some(quality) = request.quality {
            self.export_modal.quality = quality;
        }
        if let Some(frame_format) = request.frame_format {
            self.export_modal.frame_format = frame_format;
        }
        if let Some(timestamp_overlay) = request.timestamp_overlay {
            self.export_modal.timestamp_overlay_enabled = timestamp_overlay.enabled;
            self.export_modal.timestamp_overlay_position = timestamp_overlay.position;
        }
        self.editor.overlays.export_video = request.open_panel;
        self.start_export_video();

        if self.export_cancel.is_some() {
            crate::core::automation::AutomationResponse::ok(serde_json::json!({
                "export": self.agent_export_status_json(),
            }))
        } else {
            crate::core::automation::AutomationResponse::error(
                self.export_modal
                    .error
                    .clone()
                    .unwrap_or_else(|| "Export did not start.".to_string()),
            )
        }
    }

    pub(super) fn agent_export_status_response(
        &self,
    ) -> crate::core::automation::AutomationResponse {
        crate::core::automation::AutomationResponse::ok(serde_json::json!({
            "export": self.agent_export_status_json(),
        }))
    }

    pub(super) fn agent_export_status_json(&self) -> serde_json::Value {
        let status = match self.export_modal.status {
            ExportRunStatus::Idle => "idle",
            ExportRunStatus::Running => "running",
            ExportRunStatus::Finished => "finished",
            ExportRunStatus::Cancelled => "cancelled",
            ExportRunStatus::Failed => "failed",
        };
        serde_json::json!({
            "running": self.export_cancel.is_some(),
            "status": status,
            "progress": self.export_modal.progress,
            "stage": self.export_modal.stage,
            "message": self.export_modal.message,
            "frame_label": self.export_modal.frame_label,
            "error": self.export_modal.error,
            "summary": self.export_modal.summary,
            "warnings": self.export_modal.warnings,
            "settings": {
                "output_path": self.export_modal.output_path,
                "codec": self.export_modal.codec,
                "width": self.export_modal.width,
                "height": self.export_modal.height,
                "fps": self.export_modal.fps,
                "start_seconds": self.export_modal.start_seconds,
                "duration_seconds": self.export_modal.duration_seconds,
                "include_audio": self.export_modal.include_audio,
                "quality": self.export_modal.quality,
                "frame_format": self.export_modal.frame_format,
                "timestamp_overlay": {
                    "enabled": self.export_modal.timestamp_overlay_enabled,
                    "position": self.export_modal.timestamp_overlay_position,
                },
            },
        })
    }

    pub(super) fn test_agent_provider(
        &mut self,
        provider_id: Uuid,
        live: bool,
    ) -> crate::core::automation::AutomationResponse {
        let Some(provider) = self
            .editor
            .provider_entries
            .iter()
            .find(|provider| provider.id == provider_id)
            .cloned()
        else {
            return crate::core::automation::AutomationResponse::not_found("Provider not found.");
        };
        let Some(runtime) = self.generation_runtime.as_ref() else {
            return crate::core::automation::AutomationResponse::with_status(
                "Generation runtime is unavailable.",
                500,
            );
        };
        match runtime.block_on(crate::providers::test_provider_connection(&provider, live)) {
            Ok(result) => {
                self.editor.status = format!("Provider test passed: {}", provider.name);
                crate::core::automation::AutomationResponse::ok(serde_json::json!({
                    "provider": result,
                }))
            }
            Err(err) => {
                self.editor.status = format!("Provider test failed: {err}");
                crate::core::automation::AutomationResponse::error(err)
            }
        }
    }

    pub(super) fn agent_continuation_response(
        &mut self,
        action: impl FnOnce(&mut Self),
    ) -> crate::core::automation::AutomationResponse {
        let before_assets: std::collections::HashSet<Uuid> = self
            .editor
            .project
            .assets
            .iter()
            .map(|asset| asset.id)
            .collect();
        let before_clips: std::collections::HashSet<Uuid> = self
            .editor
            .project
            .clips
            .iter()
            .map(|clip| clip.id)
            .collect();

        action(self);

        let asset_ids: Vec<Uuid> = self
            .editor
            .project
            .assets
            .iter()
            .map(|asset| asset.id)
            .filter(|asset_id| !before_assets.contains(asset_id))
            .collect();
        let clip_ids: Vec<Uuid> = self
            .editor
            .project
            .clips
            .iter()
            .map(|clip| clip.id)
            .filter(|clip_id| !before_clips.contains(clip_id))
            .collect();

        if asset_ids.is_empty() && clip_ids.is_empty() {
            crate::core::automation::AutomationResponse::error(self.editor.status.clone())
        } else {
            crate::core::automation::AutomationResponse::ok(serde_json::json!({
                "asset_ids": asset_ids,
                "clip_ids": clip_ids,
                "selection": {
                    "assets": self.editor.selection.asset_ids.clone(),
                    "clips": self.editor.selection.clip_ids.clone(),
                    "tracks": self.editor.selection.track_ids.clone(),
                    "markers": self.editor.selection.marker_ids.clone(),
                },
                "status": self.editor.status,
            }))
        }
    }

    pub(super) fn agent_asset_lab_graph_response(
        &self,
        asset_id: Uuid,
    ) -> crate::core::automation::AutomationResponse {
        if self.editor.project.find_asset(asset_id).is_none() {
            return crate::core::automation::AutomationResponse::not_found("Asset not found.");
        }
        if self.editor.project.generative_config(asset_id).is_none() {
            return crate::core::automation::AutomationResponse::not_found(
                "Asset does not have an Asset Lab graph.",
            );
        }
        crate::core::automation::AutomationResponse::ok(serde_json::json!({
            "asset_lab": self.agent_asset_lab_graph_json(asset_id),
        }))
    }

    pub(super) fn agent_asset_lab_graph_json(&self, asset_id: Uuid) -> serde_json::Value {
        let asset = self.editor.project.find_asset(asset_id);
        let config = self.editor.project.generative_config(asset_id);
        serde_json::json!({
            "asset_id": asset_id,
            "asset": asset,
            "graph": config.map(|config| config.lab_graph.clone()),
            "versions": config.map(|config| config.versions.clone()),
            "active_version": config.and_then(|config| config.active_version.clone()),
            "status": self.editor.status,
        })
    }

    pub(super) fn add_agent_asset_lab_node(
        &mut self,
        asset_id: Uuid,
        provider_id: Option<Uuid>,
        parent_node_id: Option<Uuid>,
        inputs: std::collections::HashMap<String, InputValue>,
    ) -> crate::core::automation::AutomationResponse {
        if let Err(err) = self.validate_agent_asset_lab_node(asset_id, provider_id, parent_node_id)
        {
            return err;
        }
        if let Err(err) = validate_agent_input_refs(&self.editor.project, &inputs) {
            return err;
        }
        let mut node = crate::state::AssetLabNode::new_with_parent(provider_id, parent_node_id);
        node.inputs = inputs;
        let node_id = node.id;
        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                config.lab_graph.selected_node_id = Some(node_id);
                config.lab_graph.nodes.push(node);
                config.normalize_lab_graph_lineage();
            });
        if !updated {
            return crate::core::automation::AutomationResponse::not_found(
                "Asset does not support Asset Lab steps.",
            );
        }
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            return crate::core::automation::AutomationResponse::with_status(
                format!("Failed to save Asset Lab graph: {err}"),
                500,
            );
        }
        self.asset_lab.clear_draft();
        self.editor.preview_dirty = true;
        self.editor.status = "Added Asset Lab step.".to_string();
        crate::core::automation::AutomationResponse::ok(serde_json::json!({
            "node_id": node_id,
            "asset_lab": self.agent_asset_lab_graph_json(asset_id),
        }))
    }

    pub(super) fn set_agent_asset_lab_node(
        &mut self,
        asset_id: Uuid,
        node_id: Uuid,
        patch: crate::core::automation::AssetLabNodePatch,
    ) -> crate::core::automation::AutomationResponse {
        let provider_id_for_validation = patch
            .replace
            .as_ref()
            .map(|node| node.provider_id)
            .or(patch.provider_id);
        let parent_id_for_validation = patch
            .replace
            .as_ref()
            .map(|node| node.parent_node_id)
            .or(patch.parent_node_id);
        if let Err(err) = self.validate_agent_asset_lab_node(
            asset_id,
            provider_id_for_validation.flatten(),
            parent_id_for_validation.flatten(),
        ) {
            return err;
        }
        let Some(config_snapshot) = self.editor.project.generative_config(asset_id).cloned() else {
            return crate::core::automation::AutomationResponse::not_found(
                "Asset does not support Asset Lab steps.",
            );
        };
        if !config_snapshot
            .lab_graph
            .nodes
            .iter()
            .any(|node| node.id == node_id)
        {
            return crate::core::automation::AutomationResponse::not_found(
                "Asset Lab node not found.",
            );
        }
        if asset_lab_parent_would_cycle(
            &config_snapshot.lab_graph.nodes,
            node_id,
            parent_id_for_validation.flatten(),
        ) {
            return crate::core::automation::AutomationResponse::conflict(
                "Asset Lab parent would create a cycle.",
            );
        }
        if let Some(replacement) = patch.replace.as_ref() {
            if let Err(err) = validate_agent_input_refs(&self.editor.project, &replacement.inputs) {
                return err;
            }
            if let Some(output_version) = replacement.output_version.as_ref() {
                if !config_snapshot
                    .versions
                    .iter()
                    .any(|record| record.version == *output_version)
                {
                    return crate::core::automation::AutomationResponse::not_found(
                        "Generation version not found.",
                    );
                }
            }
        }
        if let Some(inputs) = patch.inputs.as_ref() {
            if let Err(err) = validate_agent_input_refs(&self.editor.project, inputs) {
                return err;
            }
        }
        if let Some(Some(output_version)) = patch.output_version.as_ref() {
            if !config_snapshot
                .versions
                .iter()
                .any(|record| record.version == *output_version)
            {
                return crate::core::automation::AutomationResponse::not_found(
                    "Generation version not found.",
                );
            }
        }

        let selected = patch.selected;
        let updated = self
            .editor
            .project
            .update_generative_config(asset_id, |config| {
                if let Some(node) = config
                    .lab_graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == node_id)
                {
                    if let Some(mut replacement) = patch.replace.clone() {
                        replacement.id = node_id;
                        *node = replacement;
                    }
                    if let Some(provider_id) = patch.provider_id {
                        node.provider_id = provider_id;
                    }
                    if let Some(parent_node_id) = patch.parent_node_id {
                        node.parent_node_id = parent_node_id;
                    }
                    if let Some(inputs) = patch.inputs.clone() {
                        node.inputs = inputs;
                    }
                    if let Some(output_version) = patch.output_version.clone() {
                        node.output_version = output_version;
                    }
                }
                match selected {
                    Some(true) => config.lab_graph.selected_node_id = Some(node_id),
                    Some(false) if config.lab_graph.selected_node_id == Some(node_id) => {
                        config.lab_graph.selected_node_id = None;
                    }
                    _ => {}
                }
                config.normalize_lab_graph_lineage();
            });
        if !updated {
            return crate::core::automation::AutomationResponse::not_found(
                "Asset does not support Asset Lab steps.",
            );
        }
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            return crate::core::automation::AutomationResponse::with_status(
                format!("Failed to save Asset Lab graph: {err}"),
                500,
            );
        }
        self.asset_lab.clear_draft();
        self.invalidate_generative_asset_runtime(asset_id);
        self.editor.status = "Updated Asset Lab step.".to_string();
        crate::core::automation::AutomationResponse::ok(serde_json::json!({
            "node_id": node_id,
            "asset_lab": self.agent_asset_lab_graph_json(asset_id),
        }))
    }

    fn validate_agent_asset_lab_node(
        &self,
        asset_id: Uuid,
        provider_id: Option<Uuid>,
        parent_node_id: Option<Uuid>,
    ) -> Result<(), crate::core::automation::AutomationResponse> {
        let Some(asset) = self.editor.project.find_asset(asset_id) else {
            return Err(crate::core::automation::AutomationResponse::not_found(
                "Asset not found.",
            ));
        };
        let Some(config) = self.editor.project.generative_config(asset_id) else {
            return Err(crate::core::automation::AutomationResponse::not_found(
                "Asset does not support Asset Lab steps.",
            ));
        };
        if let Some(parent_node_id) = parent_node_id {
            if !config
                .lab_graph
                .nodes
                .iter()
                .any(|node| node.id == parent_node_id)
            {
                return Err(crate::core::automation::AutomationResponse::not_found(
                    "Parent Asset Lab node not found.",
                ));
            }
        }
        if let Some(provider_id) = provider_id {
            let Some(provider) = self
                .editor
                .provider_entries
                .iter()
                .find(|provider| provider.id == provider_id)
            else {
                return Err(crate::core::automation::AutomationResponse::not_found(
                    "Provider not found.",
                ));
            };
            if !asset_lab_provider_is_compatible(asset, provider) {
                return Err(crate::core::automation::AutomationResponse::conflict(
                    "Provider output type does not match this asset.",
                ));
            }
            if !self.editor.provider_in_project_scope(provider.id) {
                return Err(crate::core::automation::AutomationResponse::conflict(
                    "Provider is outside this project's provider scope.",
                ));
            }
        }
        Ok(())
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

    pub(super) fn run_agent_capture(
        &mut self,
        ctx: &Context,
        request: crate::core::automation::CaptureRequest,
    ) -> crate::core::automation::AutomationResponse {
        match request {
            crate::core::automation::CaptureRequest::Frame {
                source,
                time,
                mode,
                format,
                annotate,
                seek_ui,
                name,
            } => {
                self.capture_agent_frame(ctx, source, time, mode, &format, annotate, seek_ui, name)
            }
            crate::core::automation::CaptureRequest::Cutsheet {
                source,
                frames,
                layout,
                mode,
                format,
                annotate,
                seek_ui,
                name,
            } => self.capture_agent_cutsheet(
                ctx, source, frames, layout, mode, &format, annotate, seek_ui, name,
            ),
        }
    }

    fn extract_agent_still_to_asset(
        &mut self,
        source: crate::core::automation::CaptureSource,
        time: Option<crate::core::automation::TimeSelector>,
        name: Option<String>,
    ) -> crate::core::automation::AutomationResponse {
        let request = CaptureWorkItem {
            label: Some("extract still".to_string()),
            time,
        };
        let capture = match self.render_agent_capture_item(
            &source,
            &request,
            crate::core::automation::CaptureMode::Normal,
            false,
        ) {
            Ok(capture) => capture,
            Err(err) => return agent_error_response(err),
        };

        let source_json = capture.source_json.clone();
        let time_json = capture.time_json.clone();
        let local_time_json = capture.local_time_json.clone();
        let image = capture.image;
        let result = match &source {
            crate::core::automation::CaptureSource::Clip { clip_id } => {
                let Some(clip) = self
                    .editor
                    .project
                    .clips
                    .iter()
                    .find(|clip| clip.id == *clip_id)
                    .cloned()
                else {
                    return crate::core::automation::AutomationResponse::not_found(
                        "Clip not found.",
                    );
                };
                let label = name.as_deref();
                let base_name = label.unwrap_or_else(|| {
                    clip.label
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or("Clip Still")
                });
                self.editor
                    .add_rendered_frame_asset(base_name, image)
                    .map(|asset_id| (asset_id, Some(clip.asset_id)))
            }
            crate::core::automation::CaptureSource::Asset { asset_id, version } => {
                let source_asset_id = *asset_id;
                let base_name = name.as_deref().unwrap_or("Asset Still");
                match self.editor.add_extracted_frame_asset(
                    source_asset_id,
                    version.as_deref(),
                    local_time_json
                        .as_ref()
                        .and_then(|value| value.get("seconds"))
                        .and_then(|value| value.as_f64())
                        .unwrap_or(0.0),
                    image,
                ) {
                    Ok(asset_id) => {
                        if name.is_some() {
                            let _ = self.editor.rename_asset(asset_id, base_name.to_string());
                        }
                        Ok((asset_id, Some(source_asset_id)))
                    }
                    Err(err) => Err(err),
                }
            }
            crate::core::automation::CaptureSource::Timeline => {
                let base_name = name.as_deref().unwrap_or("Timeline Still");
                self.editor
                    .add_rendered_frame_asset(base_name, image)
                    .map(|asset_id| (asset_id, None))
            }
        };

        match result {
            Ok((asset_id, source_asset_id)) => {
                crate::core::automation::AutomationResponse::ok(serde_json::json!({
                    "asset_id": asset_id,
                    "source_asset_id": source_asset_id,
                    "source": source_json,
                    "time": time_json,
                    "local_time": local_time_json,
                    "status": self.editor.status,
                }))
            }
            Err(err) => crate::core::automation::AutomationResponse::error(err),
        }
    }

    fn capture_agent_frame(
        &mut self,
        ctx: &Context,
        source: crate::core::automation::CaptureSource,
        time: Option<crate::core::automation::TimeSelector>,
        mode: crate::core::automation::CaptureMode,
        format: &str,
        annotate: bool,
        seek_ui: bool,
        name: Option<String>,
    ) -> crate::core::automation::AutomationResponse {
        if !format.eq_ignore_ascii_case("png") {
            return crate::core::automation::AutomationResponse::error(
                "Only PNG capture format is supported.",
            );
        }
        let dir = match crate::core::automation::agent_capture_dir(name.as_deref()) {
            Ok(dir) => dir,
            Err(err) => {
                return crate::core::automation::AutomationResponse::with_status(err, 500);
            }
        };
        let request = CaptureWorkItem {
            label: Some("frame".to_string()),
            time,
        };
        let capture = match self.render_agent_capture_item(&source, &request, mode, annotate) {
            Ok(capture) => capture,
            Err(err) => return agent_error_response(err),
        };
        if seek_ui {
            self.seek_editor(capture.timeline_seconds, false);
            ctx.request_repaint();
        }

        let frame_path = dir.join("frame-0001.png");
        if let Err(err) = save_agent_rgba_png(&frame_path, &capture.image) {
            return crate::core::automation::AutomationResponse::with_status(err, 500);
        }
        let manifest_path = dir.join("manifest.json");
        let manifest = serde_json::json!({
            "kind": "frame",
            "path": frame_path,
            "markdown": agent_markdown_image(&frame_path, "LatentSlate capture"),
            "source": capture.source_json,
            "time": capture.time_json,
            "local_time": capture.local_time_json,
            "mode": capture_mode_label(mode),
            "stats": capture.stats,
            "inspection": capture.inspection_json,
        });
        if let Err(err) = write_agent_manifest(&manifest_path, &manifest) {
            return crate::core::automation::AutomationResponse::with_status(err, 500);
        }

        crate::core::automation::AutomationResponse::ok(serde_json::json!({
            "capture": {
                "kind": "frame",
                "path": frame_path,
                "markdown": agent_markdown_image(&frame_path, "LatentSlate capture"),
                "manifest_path": manifest_path,
                "source": capture.source_json,
                "time": capture.time_json,
                "local_time": capture.local_time_json,
                "mode": capture_mode_label(mode),
                "stats": capture.stats,
                "inspection": capture.inspection_json,
            }
        }))
    }

    fn capture_agent_cutsheet(
        &mut self,
        ctx: &Context,
        source: crate::core::automation::CaptureSource,
        frames: Vec<crate::core::automation::CaptureFrameRequest>,
        layout: crate::core::automation::CaptureSheetLayout,
        mode: crate::core::automation::CaptureMode,
        format: &str,
        annotate: bool,
        seek_ui: bool,
        name: Option<String>,
    ) -> crate::core::automation::AutomationResponse {
        if !format.eq_ignore_ascii_case("png") {
            return crate::core::automation::AutomationResponse::error(
                "Only PNG capture format is supported.",
            );
        }
        if frames.is_empty() {
            return crate::core::automation::AutomationResponse::error(
                "Cutsheet requires at least one frame request.",
            );
        }
        if frames.len() > crate::core::automation::MAX_AGENT_CUTSHEET_FRAMES {
            return crate::core::automation::AutomationResponse::error(format!(
                "Cutsheet supports at most {} frame requests.",
                crate::core::automation::MAX_AGENT_CUTSHEET_FRAMES
            ));
        }
        if layout.columns == 0 || layout.columns > 8 {
            return crate::core::automation::AutomationResponse::error(
                "Cutsheet layout columns must be between 1 and 8.",
            );
        }
        if layout.thumb_width > crate::core::automation::MAX_AGENT_CUTSHEET_THUMB_WIDTH {
            return crate::core::automation::AutomationResponse::error(format!(
                "Cutsheet thumb_width must be at most {} pixels.",
                crate::core::automation::MAX_AGENT_CUTSHEET_THUMB_WIDTH
            ));
        }
        let dir = match crate::core::automation::agent_capture_dir(name.as_deref()) {
            Ok(dir) => dir,
            Err(err) => {
                return crate::core::automation::AutomationResponse::with_status(err, 500);
            }
        };

        let mut rendered = Vec::new();
        for (index, frame) in frames.into_iter().enumerate() {
            let request = CaptureWorkItem {
                label: frame
                    .label
                    .clone()
                    .or_else(|| Some(format!("frame-{:02}", index + 1))),
                time: frame.time,
            };
            let capture = match self.render_agent_capture_item(&source, &request, mode, annotate) {
                Ok(capture) => capture,
                Err(err) => return agent_error_response(err),
            };
            let frame_path = dir.join(format!("frame-{:04}.png", index + 1));
            if let Err(err) = save_agent_rgba_png(&frame_path, &capture.image) {
                return crate::core::automation::AutomationResponse::with_status(err, 500);
            }
            rendered.push((frame_path, capture));
        }

        if seek_ui {
            if let Some((_, capture)) = rendered.last() {
                self.seek_editor(capture.timeline_seconds, false);
                ctx.request_repaint();
            }
        }

        let sheet_path = dir.join("cutsheet.png");
        if let Err(err) = save_agent_cutsheet_png(&sheet_path, &rendered, &layout) {
            return crate::core::automation::AutomationResponse::with_status(err, 500);
        }

        let frames_json: Vec<_> = rendered
            .iter()
            .map(|(path, capture)| {
                serde_json::json!({
                    "label": capture.label,
                    "path": path,
                    "markdown": agent_markdown_image(path, capture.label.as_deref().unwrap_or("LatentSlate capture frame")),
                    "source": capture.source_json,
                    "time": capture.time_json,
                    "local_time": capture.local_time_json,
                    "stats": capture.stats,
                    "inspection": capture.inspection_json,
                })
            })
            .collect();
        let manifest_path = dir.join("manifest.json");
        let manifest = serde_json::json!({
            "kind": "cutsheet",
            "path": sheet_path,
            "markdown": agent_markdown_image(&sheet_path, "LatentSlate cutsheet"),
            "mode": capture_mode_label(mode),
            "frames": frames_json,
        });
        if let Err(err) = write_agent_manifest(&manifest_path, &manifest) {
            return crate::core::automation::AutomationResponse::with_status(err, 500);
        }

        crate::core::automation::AutomationResponse::ok(serde_json::json!({
            "capture": {
                "kind": "cutsheet",
                "path": sheet_path,
                "markdown": agent_markdown_image(&sheet_path, "LatentSlate cutsheet"),
                "manifest_path": manifest_path,
                "mode": capture_mode_label(mode),
                "frames": frames_json,
            }
        }))
    }

    fn render_agent_capture_item(
        &self,
        source: &crate::core::automation::CaptureSource,
        request: &CaptureWorkItem,
        mode: crate::core::automation::CaptureMode,
        annotate: bool,
    ) -> Result<AgentRenderedCapture, String> {
        let (project, timeline_seconds, local_seconds, source_json, local_scope) =
            self.capture_project_and_time(source, request.time.as_ref())?;
        let decode_mode = PreviewDecodeMode::Seek;
        let output = self.editor.previewer.render_frame_rgba(
            &project,
            timeline_seconds,
            decode_mode,
            self.editor.layout.hardware_decode,
        );
        let Some(frame) = output.frame else {
            return Err("No visual frame was available for capture.".to_string());
        };
        let mut image = image::RgbaImage::from_raw(frame.width, frame.height, frame.bytes)
            .ok_or_else(|| "Rendered frame bytes were invalid.".to_string())?;
        if mode == crate::core::automation::CaptureMode::Enhanced || annotate {
            let layers = self.editor.previewer.render_layers(
                &project,
                timeline_seconds,
                decode_mode,
                self.editor.layout.hardware_decode,
            );
            let clip_summary = agent_capture_clip_summary(&project, timeline_seconds);
            draw_agent_capture_overlay(
                &mut image,
                layers.layers.as_ref(),
                timeline_seconds,
                local_seconds,
                &request.label,
                local_scope,
                mode,
                clip_summary.as_deref(),
            );
        }
        let fps = project.settings.fps.max(1.0);
        let inspection_json = agent_capture_inspection_json(&project, timeline_seconds);
        Ok(AgentRenderedCapture {
            label: request.label.clone(),
            image,
            stats: output.stats,
            timeline_seconds,
            time_json: normalized_time_json(timeline_seconds, fps, "timeline"),
            local_time_json: local_seconds
                .map(|seconds| normalized_time_json(seconds, fps, local_scope)),
            source_json,
            inspection_json,
        })
    }

    fn capture_project_and_time(
        &self,
        source: &crate::core::automation::CaptureSource,
        selector: Option<&crate::core::automation::TimeSelector>,
    ) -> Result<(Project, f64, Option<f64>, serde_json::Value, &'static str), String> {
        let mut project = self.editor.project.clone();
        match source {
            crate::core::automation::CaptureSource::Timeline => {
                let duration = project.duration();
                let seconds = resolve_agent_time_selector(
                    selector,
                    duration,
                    project.settings.fps,
                    Some(self.editor.current_time),
                    None,
                );
                Ok((
                    project,
                    seconds,
                    None,
                    serde_json::json!({ "type": "timeline" }),
                    "timeline",
                ))
            }
            crate::core::automation::CaptureSource::Clip { clip_id } => {
                let clip = project
                    .clips
                    .iter()
                    .find(|clip| clip.id == *clip_id)
                    .cloned()
                    .ok_or_else(|| "Clip not found.".to_string())?;
                let local = resolve_agent_time_selector(
                    selector,
                    clip.duration,
                    project.settings.fps,
                    Some((self.editor.current_time - clip.start_time).max(0.0)),
                    project
                        .find_asset(clip.asset_id)
                        .and_then(|asset| asset_last_frame_time_seconds(asset, clip.duration)),
                );
                let timeline_seconds = (clip.start_time + local).clamp(0.0, project.duration());
                Ok((
                    project,
                    timeline_seconds,
                    Some(local),
                    serde_json::json!({ "type": "clip", "clip_id": clip_id }),
                    "clip",
                ))
            }
            crate::core::automation::CaptureSource::Asset { asset_id, version } => {
                let asset = project
                    .find_asset(*asset_id)
                    .cloned()
                    .ok_or_else(|| "Asset not found.".to_string())?;
                if !asset.is_visual() {
                    return Err("Asset capture only supports visual assets.".to_string());
                }
                if let Some(version) = version {
                    let Some(config) = project.generative_config(*asset_id) else {
                        return Err("Generative config not found.".to_string());
                    };
                    if !config
                        .versions
                        .iter()
                        .any(|record| record.version == *version)
                    {
                        return Err("Generation version not found.".to_string());
                    }
                    if let Some(asset) = project
                        .assets
                        .iter_mut()
                        .find(|asset| asset.id == *asset_id)
                    {
                        match &mut asset.kind {
                            AssetKind::GenerativeImage { active_version, .. }
                            | AssetKind::GenerativeVideo { active_version, .. } => {
                                *active_version = Some(version.clone());
                            }
                            _ => {}
                        }
                    }
                    let _ = project.update_generative_config(*asset_id, |config| {
                        config.active_version = Some(version.clone());
                    });
                }
                let duration = asset
                    .duration_seconds
                    .unwrap_or(crate::constants::DEFAULT_CLIP_DURATION_SECONDS);
                let seconds = resolve_agent_time_selector(
                    selector,
                    duration,
                    project.settings.fps,
                    Some(0.0),
                    asset_last_frame_time_seconds(&asset, duration),
                );
                let track_id = project
                    .tracks
                    .iter()
                    .find(|track| project.asset_compatible_with_track(*asset_id, track.id))
                    .map(|track| track.id)
                    .ok_or_else(|| "No compatible track exists for asset capture.".to_string())?;
                let mut clip = Clip::new(*asset_id, track_id, 0.0, duration.max(0.1));
                clip.label = Some("asset capture".to_string());
                project.clips = vec![clip];
                project.markers.clear();
                project.settings.duration_seconds = duration.max(seconds);
                Ok((
                    project,
                    seconds,
                    Some(seconds),
                    serde_json::json!({
                        "type": "asset",
                        "asset_id": asset_id,
                        "version": version,
                    }),
                    "asset",
                ))
            }
        }
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

fn asset_lab_parent_would_cycle(
    nodes: &[crate::state::AssetLabNode],
    node_id: Uuid,
    parent_node_id: Option<Uuid>,
) -> bool {
    let Some(mut current_id) = parent_node_id else {
        return false;
    };
    let mut visited = std::collections::HashSet::new();
    loop {
        if current_id == node_id {
            return true;
        }
        if !visited.insert(current_id) {
            return true;
        }
        let Some(parent) = nodes.iter().find(|node| node.id == current_id) else {
            return false;
        };
        let Some(next_id) = parent.parent_node_id else {
            return false;
        };
        current_id = next_id;
    }
}

fn agent_error_response(message: String) -> crate::core::automation::AutomationResponse {
    let lower = message.to_ascii_lowercase();
    if lower.contains("not found") || lower.contains("unavailable") {
        crate::core::automation::AutomationResponse::not_found(message)
    } else if lower.contains("could not") || lower.contains("not support") {
        crate::core::automation::AutomationResponse::conflict(message)
    } else {
        crate::core::automation::AutomationResponse::error(message)
    }
}

fn validate_agent_input_refs(
    project: &crate::state::Project,
    values: &std::collections::HashMap<String, InputValue>,
) -> Result<(), crate::core::automation::AutomationResponse> {
    for (name, value) in values {
        match value {
            InputValue::AssetRef {
                asset_id,
                source_clip_id,
                ..
            } => {
                if project.find_asset(*asset_id).is_none() {
                    return Err(crate::core::automation::AutomationResponse::not_found(
                        format!("Input {name} references missing asset {asset_id}."),
                    ));
                }
                if let Some(source_clip_id) = source_clip_id {
                    let Some(clip) = project.clips.iter().find(|clip| clip.id == *source_clip_id)
                    else {
                        return Err(crate::core::automation::AutomationResponse::not_found(
                            format!(
                                "Input {name} references missing source clip {source_clip_id}."
                            ),
                        ));
                    };
                    if clip.asset_id != *asset_id {
                        return Err(crate::core::automation::AutomationResponse::conflict(
                            format!(
                                "Input {name} source clip {source_clip_id} does not belong to asset {asset_id}."
                            ),
                        ));
                    }
                }
            }
            InputValue::GenerationRef {
                asset_id, version, ..
            } => {
                if project.find_asset(*asset_id).is_none() {
                    return Err(crate::core::automation::AutomationResponse::not_found(
                        format!("Input {name} references missing asset {asset_id}."),
                    ));
                }
                let Some(config) = project.generative_config(*asset_id) else {
                    return Err(crate::core::automation::AutomationResponse::conflict(
                        format!("Input {name} references non-generative asset {asset_id}."),
                    ));
                };
                if !config
                    .versions
                    .iter()
                    .any(|record| record.version == *version)
                {
                    return Err(crate::core::automation::AutomationResponse::not_found(
                        format!("Input {name} references missing generation version {version}."),
                    ));
                }
            }
            InputValue::Literal { .. } => {}
        }
    }
    Ok(())
}

struct CaptureWorkItem {
    label: Option<String>,
    time: Option<crate::core::automation::TimeSelector>,
}

struct AgentRenderedCapture {
    label: Option<String>,
    image: image::RgbaImage,
    stats: PreviewStats,
    timeline_seconds: f64,
    time_json: serde_json::Value,
    local_time_json: Option<serde_json::Value>,
    source_json: serde_json::Value,
    inspection_json: serde_json::Value,
}

fn resolve_agent_time_selector(
    selector: Option<&crate::core::automation::TimeSelector>,
    duration: f64,
    fps: f64,
    current: Option<f64>,
    last_frame_seconds: Option<f64>,
) -> f64 {
    let duration = duration.max(0.0);
    let Some(selector) = selector else {
        return clamp_agent_capture_time(current.unwrap_or(0.0), duration, fps);
    };
    if let Some(seconds) = selector.seconds {
        return clamp_agent_capture_time(seconds, duration, fps);
    }
    if let Some(frame) = selector.frame {
        return clamp_agent_capture_time(frame.max(0) as f64 / fps.max(1.0), duration, fps);
    }
    if let Some(percent) = selector.percent {
        return clamp_agent_capture_time(duration * percent.clamp(0.0, 1.0), duration, fps);
    }
    match selector.key {
        Some(crate::core::automation::TimeKey::First) => 0.0,
        Some(crate::core::automation::TimeKey::Last) => {
            clamp_agent_capture_time(last_frame_seconds.unwrap_or(duration), duration, fps)
        }
        Some(crate::core::automation::TimeKey::Current) | None => {
            clamp_agent_capture_time(current.unwrap_or(0.0), duration, fps)
        }
    }
}

fn asset_last_frame_time_seconds(asset: &Asset, duration: f64) -> Option<f64> {
    let AssetKind::GenerativeVideo {
        fps, frame_count, ..
    } = &asset.kind
    else {
        return None;
    };
    if !fps.is_finite() || *fps <= 0.0 || *frame_count == 0 {
        return None;
    }
    Some((frame_count.saturating_sub(1)) as f64 / *fps).map(|seconds| {
        let duration = duration.max(0.0);
        if duration <= f64::EPSILON {
            0.0
        } else {
            seconds.clamp(0.0, duration)
        }
    })
}

fn clamp_agent_capture_time(seconds: f64, duration: f64, fps: f64) -> f64 {
    let duration = duration.max(0.0);
    if duration <= f64::EPSILON {
        return 0.0;
    }
    let frame_epsilon = (1.0 / fps.max(1.0)).min(duration);
    seconds.clamp(0.0, (duration - frame_epsilon).max(0.0))
}

fn normalized_time_json(seconds: f64, fps: f64, scope: &str) -> serde_json::Value {
    let fps = fps.max(1.0);
    serde_json::json!({
        "seconds": seconds,
        "frame": (seconds.max(0.0) * fps).round() as i64,
        "fps": fps,
        "scope": scope,
    })
}

fn save_agent_rgba_png(path: &Path, image: &image::RgbaImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Failed to create capture directory {}: {err}",
                parent.display()
            )
        })?;
    }
    image
        .save(path)
        .map_err(|err| format!("Failed to save capture {}: {err}", path.display()))
}

fn write_agent_manifest(path: &Path, manifest: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Failed to create capture directory {}: {err}",
                parent.display()
            )
        })?;
    }
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|err| format!("Failed to encode capture manifest: {err}"))?;
    std::fs::write(path, json)
        .map_err(|err| format!("Failed to write capture manifest {}: {err}", path.display()))
}

fn agent_markdown_image(path: &Path, alt: &str) -> String {
    format!(
        "![{}]({})",
        alt,
        path.display().to_string().replace('\\', "/")
    )
}

fn save_agent_cutsheet_png(
    path: &Path,
    rendered: &[(PathBuf, AgentRenderedCapture)],
    layout: &crate::core::automation::CaptureSheetLayout,
) -> Result<(), String> {
    let columns = layout.columns.clamp(1, 8);
    let thumb_width = layout.thumb_width.clamp(96, 1024);
    let resized: Vec<image::RgbaImage> = rendered
        .iter()
        .map(|(_, capture)| {
            let scale = thumb_width as f32 / capture.image.width().max(1) as f32;
            let thumb_height = (capture.image.height().max(1) as f32 * scale)
                .round()
                .max(1.0) as u32;
            image::imageops::resize(
                &capture.image,
                thumb_width,
                thumb_height,
                image::imageops::FilterType::Triangle,
            )
        })
        .collect();
    let rows = (resized.len() + columns - 1) / columns;
    let cell_height = resized
        .iter()
        .map(|image| image.height())
        .max()
        .unwrap_or(thumb_width)
        .max(1);
    let width = thumb_width * columns as u32;
    let height = cell_height * rows.max(1) as u32;
    let mut sheet = image::RgbaImage::from_pixel(width, height, image::Rgba([8, 9, 12, 255]));
    for (index, image) in resized.iter().enumerate() {
        let col = index % columns;
        let row = index / columns;
        image::imageops::overlay(
            &mut sheet,
            image,
            (col as u32 * thumb_width) as i64,
            (row as u32 * cell_height) as i64,
        );
    }
    save_agent_rgba_png(path, &sheet)
}

fn draw_agent_capture_overlay(
    image: &mut image::RgbaImage,
    layers: Option<&PreviewLayerStack>,
    timeline_seconds: f64,
    local_seconds: Option<f64>,
    label: &Option<String>,
    local_scope: &str,
    mode: crate::core::automation::CaptureMode,
    clip_summary: Option<&str>,
) {
    if mode == crate::core::automation::CaptureMode::Enhanced {
        if let Some(layers) = layers {
            for (index, layer) in layers.layers.iter().enumerate() {
                let color = enhanced_layer_color(index);
                let rect = imageproc::rect::Rect::at(
                    layer.placement.offset_x.round() as i32,
                    layer.placement.offset_y.round() as i32,
                )
                .of_size(
                    layer.placement.scaled_w.round().max(1.0) as u32,
                    layer.placement.scaled_h.round().max(1.0) as u32,
                );
                imageproc::drawing::draw_hollow_rect_mut(image, rect, color);
                let inset = imageproc::rect::Rect::at(rect.left() + 2, rect.top() + 2).of_size(
                    rect.width().saturating_sub(4).max(1),
                    rect.height().saturating_sub(4).max(1),
                );
                imageproc::drawing::draw_hollow_rect_mut(image, inset, color);
            }
        }
    }
    let caption = if let Some(local) = local_seconds {
        format!(
            "{} | timeline {:.3}s | {} {:.3}s",
            label.as_deref().unwrap_or("capture"),
            timeline_seconds,
            local_scope,
            local
        )
    } else {
        format!(
            "{} | timeline {:.3}s",
            label.as_deref().unwrap_or("capture"),
            timeline_seconds
        )
    };
    let caption = if let Some(clip_summary) = clip_summary.filter(|value| !value.is_empty()) {
        format!("{caption} | clips: {clip_summary}")
    } else {
        caption
    };
    draw_agent_caption(image, &caption);
}

fn agent_capture_clip_summary(project: &Project, timeline_seconds: f64) -> Option<String> {
    let mut labels = Vec::new();
    for clip in active_agent_capture_clips(project, timeline_seconds)
        .into_iter()
        .take(3)
    {
        let asset_name = project
            .find_asset(clip.asset_id)
            .map(asset_display_name)
            .unwrap_or_else(|| "missing asset".to_string());
        let label = clip
            .label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(asset_name.as_str());
        labels.push(compact_agent_label(label, 28));
    }
    if labels.is_empty() {
        None
    } else {
        Some(labels.join(", "))
    }
}

fn agent_capture_inspection_json(project: &Project, timeline_seconds: f64) -> serde_json::Value {
    let clips: Vec<_> = active_agent_capture_clips(project, timeline_seconds)
        .into_iter()
        .map(|clip| {
            let asset = project.find_asset(clip.asset_id);
            let track = project.find_track(clip.track_id);
            serde_json::json!({
                "clip_id": clip.id,
                "asset_id": clip.asset_id,
                "asset_name": asset.map(asset_display_name),
                "track_id": clip.track_id,
                "track_name": track.map(|track| track.name.clone()),
                "track_type": track.map(|track| track.track_type),
                "start_time": clip.start_time,
                "duration": clip.duration,
                "local_time": (timeline_seconds - clip.start_time).max(0.0),
                "label": clip.label.clone(),
            })
        })
        .collect();
    serde_json::json!({
        "active_clip_count": clips.len(),
        "active_clips": clips,
    })
}

fn active_agent_capture_clips(project: &Project, timeline_seconds: f64) -> Vec<&Clip> {
    let frame_epsilon = 1.0 / project.settings.fps.max(1.0);
    project
        .clips
        .iter()
        .filter(|clip| {
            if project.is_keyframe_reference_clip(clip) {
                (clip.start_time - timeline_seconds).abs() <= frame_epsilon * 0.5
            } else {
                timeline_seconds >= clip.start_time
                    && timeline_seconds < clip.start_time + clip.duration
            }
        })
        .collect()
}

fn compact_agent_label(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut label = normalized
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    label.push_str("...");
    label
}

fn enhanced_layer_color(index: usize) -> image::Rgba<u8> {
    const COLORS: [[u8; 4]; 6] = [
        [56, 189, 248, 255],
        [251, 191, 36, 255],
        [74, 222, 128, 255],
        [248, 113, 113, 255],
        [196, 181, 253, 255],
        [45, 212, 191, 255],
    ];
    image::Rgba(COLORS[index % COLORS.len()])
}

fn draw_agent_caption(image: &mut image::RgbaImage, caption: &str) {
    let Some(font) = agent_caption_font() else {
        return;
    };
    let font_size = (image.height() as f32 * 0.024).clamp(13.0, 26.0);
    let scale = ab_glyph::PxScale::from(font_size);
    let (text_w, text_h) = imageproc::drawing::text_size(scale, &font, caption);
    let pad_x = (font_size * 0.8).round() as u32;
    let pad_y = (font_size * 0.42).round() as u32;
    let box_w = text_w.saturating_add(pad_x * 2).min(image.width());
    let box_h = text_h.saturating_add(pad_y * 2).min(image.height());
    blend_agent_rect(image, 0, 0, box_w, box_h, 0.78);
    imageproc::drawing::draw_text_mut(
        image,
        image::Rgba([236, 239, 243, 255]),
        pad_x.min(image.width()) as i32,
        pad_y.min(image.height()) as i32,
        scale,
        &font,
        caption,
    );
}

fn agent_caption_font() -> Option<ab_glyph::FontArc> {
    static FONT: std::sync::OnceLock<Option<ab_glyph::FontArc>> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        for path in [
            r"C:\Windows\Fonts\seguisb.ttf",
            r"C:\Windows\Fonts\segoeuib.ttf",
            r"C:\Windows\Fonts\segoeui.ttf",
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(font) = ab_glyph::FontArc::try_from_vec(bytes) {
                    return Some(font);
                }
            }
        }
        None
    })
    .clone()
}

fn blend_agent_rect(
    image: &mut image::RgbaImage,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    alpha: f32,
) {
    let right = left.saturating_add(width).min(image.width());
    let bottom = top.saturating_add(height).min(image.height());
    let keep = (1.0 - alpha.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    for y in top..bottom {
        for x in left..right {
            let pixel = image.get_pixel_mut(x, y);
            pixel.0[0] = (pixel.0[0] as f32 * keep).round() as u8;
            pixel.0[1] = (pixel.0[1] as f32 * keep).round() as u8;
            pixel.0[2] = (pixel.0[2] as f32 * keep).round() as u8;
            pixel.0[3] = 255;
        }
    }
}

fn capture_mode_label(mode: crate::core::automation::CaptureMode) -> &'static str {
    match mode {
        crate::core::automation::CaptureMode::Normal => "normal",
        crate::core::automation::CaptureMode::Enhanced => "enhanced",
    }
}

fn format_agent_export_number(value: f64) -> String {
    if (value - value.round()).abs() < 0.0001 {
        format!("{value:.0}")
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_agent_time_selector_uses_current_when_no_selector() {
        assert_eq!(
            resolve_agent_time_selector(None, 10.0, 24.0, Some(3.5), None),
            3.5
        );
    }

    #[test]
    fn resolve_agent_time_selector_clamps_seconds_and_percent() {
        let seconds = crate::core::automation::TimeSelector {
            seconds: Some(12.0),
            ..Default::default()
        };
        assert_eq!(
            resolve_agent_time_selector(Some(&seconds), 10.0, 24.0, None, None),
            10.0 - 1.0 / 24.0
        );

        let percent = crate::core::automation::TimeSelector {
            percent: Some(0.25),
            ..Default::default()
        };
        assert_eq!(
            resolve_agent_time_selector(Some(&percent), 20.0, 24.0, None, None),
            5.0
        );

        let end_percent = crate::core::automation::TimeSelector {
            percent: Some(1.0),
            ..Default::default()
        };
        assert_eq!(
            resolve_agent_time_selector(Some(&end_percent), 20.0, 25.0, None, None),
            19.96
        );
    }

    #[test]
    fn resolve_agent_time_selector_converts_frames_and_last_key() {
        let frame = crate::core::automation::TimeSelector {
            frame: Some(48),
            ..Default::default()
        };
        assert_eq!(
            resolve_agent_time_selector(Some(&frame), 10.0, 24.0, None, None),
            2.0
        );

        let last = crate::core::automation::TimeSelector {
            key: Some(crate::core::automation::TimeKey::Last),
            ..Default::default()
        };
        assert_eq!(
            resolve_agent_time_selector(Some(&last), 10.0, 25.0, None, None),
            9.96
        );
        assert_eq!(
            resolve_agent_time_selector(Some(&last), 10.0, 25.0, None, Some(4.9333333333)),
            4.9333333333
        );
    }

    #[test]
    fn normalized_time_json_reports_frame_scope_and_fps() {
        let value = normalized_time_json(1.5, 24.0, "clip");
        assert_eq!(value["seconds"], serde_json::json!(1.5));
        assert_eq!(value["frame"], serde_json::json!(36));
        assert_eq!(value["fps"], serde_json::json!(24.0));
        assert_eq!(value["scope"], serde_json::json!("clip"));
    }

    #[test]
    fn asset_lab_parent_cycle_detection_rejects_self_and_descendant_parent() {
        let root = crate::state::AssetLabNode::new_with_parent(None, None);
        let child = crate::state::AssetLabNode::new_with_parent(None, Some(root.id));
        let grandchild = crate::state::AssetLabNode::new_with_parent(None, Some(child.id));
        let nodes = vec![root.clone(), child.clone(), grandchild.clone()];

        assert!(asset_lab_parent_would_cycle(
            &nodes,
            child.id,
            Some(child.id)
        ));
        assert!(asset_lab_parent_would_cycle(
            &nodes,
            child.id,
            Some(grandchild.id)
        ));
        assert!(!asset_lab_parent_would_cycle(
            &nodes,
            grandchild.id,
            Some(root.id)
        ));
    }
}
