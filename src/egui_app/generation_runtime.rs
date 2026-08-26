use super::*;
use crate::state::AssetLabNode;
#[derive(Debug)]
pub(super) enum GenerationEvent {
    Progress {
        job_id: Uuid,
        overall: Option<f32>,
        node: Option<f32>,
    },
    Finished {
        job_id: Uuid,
        result: Result<GenerationOutput, GenerationFailure>,
    },
}

#[derive(Debug)]
pub(super) struct GenerationOutput {
    pub(super) version: String,
    pub(super) path: PathBuf,
}

#[derive(Debug)]
pub(super) enum GenerationFailure {
    Offline(String),
    Error(String),
    Canceled,
}

#[derive(Debug)]
pub(super) enum CancelGenerationJobResult {
    Canceling { label: String },
    Cancelled { label: String, was_running: bool },
    NotFound,
    NotCancellable { status: GenerationJobStatus },
}

impl LatentSlateApp {
    pub(super) fn cancel_generation_job(&mut self, job_id: Uuid) -> CancelGenerationJobResult {
        let cancellation_token = self.generation_cancel_tokens.get(&job_id).cloned();
        let cancelled_label = if let Some(job) = self
            .editor
            .generation_queue
            .iter_mut()
            .find(|job| job.id == job_id)
        {
            let label = job.asset_label.clone();
            let was_running =
                match request_generation_cancellation(job, cancellation_token.as_deref()) {
                    Ok(was_running) => was_running,
                    Err(status) => {
                        self.editor.status = format!("Generation job is already {status:?}.");
                        return CancelGenerationJobResult::NotCancellable { status };
                    }
                };
            (label, was_running)
        } else {
            self.editor.status = "Generation job not found.".to_string();
            return CancelGenerationJobResult::NotFound;
        };

        let (label, was_running) = cancelled_label;
        if was_running {
            self.editor.status = format!(
                "Canceling generation for {label}; waiting for provider to stop or finish."
            );
            return CancelGenerationJobResult::Canceling { label };
        }
        self.editor.status = format!("Removed queued generation for {label}.");
        CancelGenerationJobResult::Cancelled { label, was_running }
    }

    pub(super) fn service_generation_queue(&mut self, ctx: &Context) {
        while let Ok(event) = self.generation_events_rx.try_recv() {
            self.handle_generation_event(event);
        }

        if generation_queue_slot_available(self.generation_active) {
            self.start_next_generation_job();
        }

        if self.generation_active.is_some()
            || self
                .editor
                .generation_queue
                .iter()
                .any(|job| job.status == GenerationJobStatus::Queued)
        {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }

    pub(super) fn start_next_generation_job(&mut self) {
        let Some(index) = self
            .editor
            .generation_queue
            .iter()
            .position(|job| job.status == GenerationJobStatus::Queued)
        else {
            return;
        };

        let Some(runtime) = self.generation_runtime.as_ref() else {
            if let Some(job) = self.editor.generation_queue.get_mut(index) {
                job.status = GenerationJobStatus::Failed;
                job.error = Some("Generation runtime unavailable.".to_string());
            }
            self.editor.status = "Generation runtime unavailable.".to_string();
            return;
        };

        let version = {
            let asset_id = self.editor.generation_queue[index].asset_id;
            let config = self
                .editor
                .project
                .generative_config(asset_id)
                .cloned()
                .unwrap_or_default();
            next_version_label(&config)
        };
        let job = {
            let entry = &mut self.editor.generation_queue[index];
            entry.status = GenerationJobStatus::Running;
            entry.progress_overall = Some(0.0);
            entry.progress_node = Some(0.0);
            entry.error = None;
            entry.version = Some(version.clone());
            entry.clone()
        };
        self.generation_active = Some(job.id);

        let cancel_token = Arc::new(AtomicBool::new(false));
        self.generation_cancel_tokens
            .insert(job.id, Arc::clone(&cancel_token));
        let events = self.generation_events_tx.clone();
        runtime.spawn(async move {
            let (progress_tx, mut progress_rx) =
                tokio::sync::mpsc::unbounded_channel::<ProviderProgress>();
            let progress_job_id = job.id;
            let progress_events = events.clone();
            tokio::spawn(async move {
                while let Some(progress) = progress_rx.recv().await {
                    let _ = progress_events.send(GenerationEvent::Progress {
                        job_id: progress_job_id,
                        overall: progress.overall,
                        node: progress.node,
                    });
                }
            });

            let job_id = job.id;
            let result =
                execute_generation_job_async(job, version, Some(progress_tx), cancel_token).await;
            let _ = events.send(GenerationEvent::Finished { job_id, result });
        });
    }

    pub(super) fn handle_generation_event(&mut self, event: GenerationEvent) {
        match event {
            GenerationEvent::Progress {
                job_id,
                overall,
                node,
            } => {
                if let Some(job) = self
                    .editor
                    .generation_queue
                    .iter_mut()
                    .find(|job| job.id == job_id)
                {
                    if job.status == GenerationJobStatus::Running {
                        if let Some(overall) = overall {
                            job.progress_overall = Some(overall.clamp(0.0, 1.0));
                        }
                        if let Some(node) = node {
                            job.progress_node = Some(node.clamp(0.0, 1.0));
                        }
                    }
                }
            }
            GenerationEvent::Finished { job_id, result } => {
                if self.generation_active == Some(job_id) {
                    self.generation_active = None;
                }
                self.generation_cancel_tokens.remove(&job_id);
                let job_snapshot = self
                    .editor
                    .generation_queue
                    .iter()
                    .find(|job| job.id == job_id)
                    .cloned();
                if job_snapshot.is_none() {
                    return;
                }

                if job_snapshot
                    .as_ref()
                    .is_some_and(|job| job.status == GenerationJobStatus::Canceling)
                {
                    self.finish_canceling_generation(job_id, result);
                    return;
                }

                // A queued job can be locally canceled before it owns a provider.  In the
                // unlikely event that a Finished message was already in flight, keep its output
                // unpublished too.
                if job_snapshot
                    .as_ref()
                    .is_some_and(|job| job.status == GenerationJobStatus::Canceled)
                {
                    if let Ok(output) = result {
                        if let Err(message) = cleanup_unpublished_output(&output.path) {
                            if let Some(entry) = self
                                .editor
                                .generation_queue
                                .iter_mut()
                                .find(|job| job.id == job_id)
                            {
                                entry.status = GenerationJobStatus::Failed;
                                entry.error = Some(message.clone());
                            }
                            self.editor.status = message;
                        }
                    }
                    return;
                }

                match result {
                    Ok(output) => {
                        if let Some(job) = job_snapshot {
                            if let Some(entry) = self
                                .editor
                                .generation_queue
                                .iter_mut()
                                .find(|job| job.id == job_id)
                            {
                                entry.status = GenerationJobStatus::Succeeded;
                                entry.version = Some(output.version.clone());
                                entry.progress_overall = Some(1.0);
                                entry.progress_node = Some(1.0);
                                entry.error = None;
                            }
                            self.finish_generation_success(job.clone(), output);
                            if let Err(err) = self.advance_generation_seed_after_attempt(&job) {
                                self.editor.status =
                                    format!("Generated, but seed advance save failed: {err}");
                            }
                        }
                    }
                    Err(err) => {
                        let message = match err {
                            GenerationFailure::Offline(err) => format!("Provider offline: {err}"),
                            GenerationFailure::Error(err) => err,
                            GenerationFailure::Canceled => "Generation cancelled.".to_string(),
                        };
                        if let Some(entry) = self
                            .editor
                            .generation_queue
                            .iter_mut()
                            .find(|job| job.id == job_id)
                        {
                            entry.status = GenerationJobStatus::Failed;
                            entry.progress_overall = None;
                            entry.progress_node = None;
                            entry.error = Some(message.clone());
                        }
                        let seed_save_error = job_snapshot
                            .as_ref()
                            .and_then(|job| self.advance_generation_seed_after_attempt(job).err());
                        self.editor.status = if let Some(err) = seed_save_error {
                            format!("{message} (seed advance save failed: {err})")
                        } else {
                            message
                        };
                    }
                }
            }
        }
    }

    fn finish_canceling_generation(
        &mut self,
        job_id: Uuid,
        result: Result<GenerationOutput, GenerationFailure>,
    ) {
        let (mut status, mut message, unpublished_output) = resolve_canceling_completion(result);
        if let Some(path) = unpublished_output {
            if let Err(cleanup_error) = cleanup_unpublished_output(&path) {
                status = GenerationJobStatus::Failed;
                message = cleanup_error;
            }
        }
        if let Some(entry) = self
            .editor
            .generation_queue
            .iter_mut()
            .find(|job| job.id == job_id)
        {
            entry.status = status;
            entry.progress_overall = None;
            entry.progress_node = None;
            entry.error = Some(message.clone());
        }
        self.editor.status = message;
    }

    pub(super) fn advance_generation_seed_after_attempt(
        &mut self,
        job: &GenerationJob,
    ) -> Result<(), String> {
        let Some(seed_advance) = job.seed_advance.as_ref() else {
            return Ok(());
        };
        if self.editor.project.find_asset(job.asset_id).is_none() {
            return Ok(());
        }

        let next_seed_value = serde_json::Value::Number(seed_advance.next_seed.into());
        self.editor
            .project
            .update_generative_config(job.asset_id, |config| {
                let next_input = InputValue::Literal {
                    value: next_seed_value,
                };
                if let Some(node_id) = job.lab_node_id {
                    if let Some(node) = config
                        .lab_graph
                        .nodes
                        .iter_mut()
                        .find(|node| node.id == node_id)
                    {
                        node.inputs
                            .insert(seed_advance.field.clone(), next_input.clone());
                    }
                }
                if job.activate_on_success || job.lab_node_id.is_none() {
                    config.inputs.insert(seed_advance.field.clone(), next_input);
                }
            });

        self.editor
            .project
            .save_generative_config(job.asset_id)
            .map_err(|err| err.to_string())
    }

    pub(super) fn finish_generation_success(
        &mut self,
        job: GenerationJob,
        output: GenerationOutput,
    ) {
        if self.editor.project.find_asset(job.asset_id).is_none() {
            return;
        }

        let version = output.version.clone();
        let record = GenerationRecord {
            version: version.clone(),
            timestamp: chrono::Utc::now(),
            provider_id: job.provider.id,
            inputs_snapshot: job.inputs_snapshot.clone(),
            media_bindings_snapshot: job.media_bindings_snapshot.clone(),
            resolved_media_inputs: job.resolved_media_inputs.clone(),
            lab_node_id: job.lab_node_id,
        };
        self.editor
            .project
            .update_generative_config(job.asset_id, |config| {
                if let Some(node_id) = job.lab_node_id {
                    if let Some(node) = config
                        .lab_graph
                        .nodes
                        .iter_mut()
                        .find(|node| node.id == node_id)
                    {
                        node.provider_id = Some(job.provider.id);
                        node.output_version = Some(version.clone());
                        if !job.media_bindings_snapshot.is_empty() {
                            node.media_bindings = job.media_bindings_snapshot.clone();
                        }
                    }
                    config.lab_graph.selected_node_id = Some(node_id);
                }
                if job.activate_on_success || job.lab_node_id.is_none() {
                    config.provider_id = Some(job.provider.id);
                    config.active_version = Some(version.clone());
                    for (name, value) in &job.inputs_snapshot {
                        if matches!(value, InputValue::Literal { .. }) {
                            config.inputs.insert(name.clone(), value.clone());
                        }
                    }
                }
                if let Some(existing) = config
                    .versions
                    .iter_mut()
                    .find(|record| record.version == version)
                {
                    *existing = record;
                } else {
                    config.versions.push(record);
                }
            });
        if let Err(err) = self.editor.project.save_generative_config(job.asset_id) {
            self.editor.status = format!("Generated, but config save failed: {err}");
        } else {
            self.editor.status = format!(
                "Generated {} {} ({})",
                job.asset_label,
                output.version,
                path_label(&output.path)
            );
        }

        if job.output_type == ProviderOutputType::Video {
            if let Some(metadata) = probe_video_metadata(&output.path) {
                if let (Some(fps), Some(frame_count)) = (metadata.fps, metadata.frame_count) {
                    let _ = self.editor.project.set_generative_video_timing(
                        job.asset_id,
                        fps,
                        frame_count,
                    );
                } else if let Some(duration) = metadata.duration_seconds {
                    let _ = self
                        .editor
                        .project
                        .set_asset_duration(job.asset_id, Some(duration));
                }
            }
        }

        self.editor.previewer.invalidate_folder(&job.folder_path);
        self.invalidate_asset_visual_cache(job.asset_id);
        self.editor.preview_dirty = true;
        if self.asset_lab.asset_id == Some(job.asset_id) && self.asset_lab.compare.is_none() {
            self.asset_lab.selected_version = Some(version.clone());
            self.asset_lab.pending_delete_version = None;
            self.asset_lab_preview_texture = None;
        }

        if let (Some(runtime), Some(asset)) = (
            self.generation_runtime.as_ref(),
            self.editor.project.find_asset(job.asset_id).cloned(),
        ) {
            let thumbnailer = Arc::clone(&self.editor.thumbnailer);
            if asset.is_video() {
                self.asset_thumbnail_warmups.insert(job.asset_id);
            }
            runtime.spawn(async move {
                let _ = thumbnailer.generate(&asset, true).await;
            });
        }
    }

    pub(super) fn invalidate_asset_visual_cache(&mut self, asset_id: Uuid) {
        self.asset_thumbnails.remove(&asset_id);
        self.asset_thumbnail_misses.remove(&asset_id);
        self.asset_thumbnail_warmups.remove(&asset_id);
        self.asset_source_dimensions.remove(&asset_id);
        self.asset_source_dimension_misses.remove(&asset_id);
        self.asset_source_fps.remove(&asset_id);
        self.asset_source_fps_misses.remove(&asset_id);
        self.timeline_thumbnails
            .retain(|key, _| key.asset_id != asset_id);
        self.timeline_thumbnail_misses
            .retain(|key| key.asset_id != asset_id);
    }

    pub(super) fn generation_status_for_asset(&self, asset_id: Uuid) -> Option<String> {
        self.editor
            .generation_queue
            .iter()
            .rev()
            .find(|job| job.asset_id == asset_id)
            .map(|job| match job.status {
                GenerationJobStatus::Queued => "Queued".to_string(),
                GenerationJobStatus::Running => {
                    let pct = job
                        .progress_overall
                        .or(job.progress_node)
                        .map(|value| format!(" {:.0}%", value * 100.0))
                        .unwrap_or_default();
                    format!("Generating{pct}")
                }
                GenerationJobStatus::Canceling => "Canceling".to_string(),
                GenerationJobStatus::Succeeded => job
                    .version
                    .as_ref()
                    .map(|version| format!("Generated {version}"))
                    .unwrap_or_else(|| "Generated".to_string()),
                GenerationJobStatus::Failed => job
                    .error
                    .as_ref()
                    .map(|error| format!("Failed: {error}"))
                    .unwrap_or_else(|| "Failed".to_string()),
                GenerationJobStatus::Canceled => "Canceled".to_string(),
            })
    }

    pub(super) fn enqueue_generation_jobs(
        &mut self,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        lab_node_id: Option<Uuid>,
        provider: ProviderEntry,
        config_snapshot: GenerativeConfig,
        folder_path: PathBuf,
        asset_label: String,
    ) -> Result<String, String> {
        if self.provider_resource_release_in_flight {
            return Err(
                "Wait for provider resource release to finish before starting generation."
                    .to_string(),
            );
        }
        if !self.editor.provider_in_project_scope(provider.id) {
            return Err("Provider is outside this project's provider scope.".to_string());
        }
        if provider.output_type == ProviderOutputType::Audio {
            return Err("Audio generation is not supported in the queue yet.".to_string());
        }

        let resolved = resolve_provider_inputs(
            &self.editor.project,
            Some(asset_id),
            context_clip_id,
            &provider,
            &config_snapshot,
        );
        if !resolved.media_errors.is_empty() {
            return Err(resolved.media_errors.join("\n"));
        }
        if !resolved.input_errors.is_empty() {
            return Err(resolved.input_errors.join("\n"));
        }
        if !resolved.missing_required.is_empty() {
            return Err(missing_provider_inputs_message(
                &provider,
                &resolved.missing_required,
            ));
        }

        let batch = config_snapshot.batch.clone();
        let batch_count = batch.count.max(1).min(MAX_GENERATION_BATCH_COUNT);
        let seed_field = resolve_seed_field(&provider);
        if batch_count > 1 && batch.seed_strategy != SeedStrategy::Keep && seed_field.is_none() {
            return Err(
                "Seed role is required for batch generation. Open Provider Builder and assign a numeric input as Role: Seed."
                    .to_string(),
            );
        }
        let mut seed_base = seed_field
            .as_ref()
            .and_then(|field| resolved.values.get(field))
            .and_then(input_value_as_i64);
        if let Some(field) = seed_field.as_ref() {
            seed_base = self.reserved_seed_base(asset_id, field, seed_base);
        }
        let mut seed_base_randomized = false;
        if seed_base.is_none()
            && seed_field.is_some()
            && batch.seed_strategy == SeedStrategy::Increment
        {
            seed_base = Some(random_seed_i64());
            seed_base_randomized = true;
        }

        let activate_on_success = lab_node_id.is_none();
        let lab_node_parent_id = if activate_on_success {
            None
        } else {
            lab_node_id.and_then(|node_id| {
                config_snapshot
                    .lab_graph
                    .nodes
                    .iter()
                    .find(|node| node.id == node_id)
                    .and_then(|node| node.parent_node_id)
            })
        };
        let mut next_parent_node_id = if activate_on_success {
            self.ensure_asset_lab_graph_for_versions(asset_id);
            self.editor
                .project
                .generative_config(asset_id)
                .and_then(|config| {
                    config.active_version.as_ref().and_then(|active_version| {
                        config
                            .versions
                            .iter()
                            .find(|record| record.version == *active_version)
                            .and_then(|record| record.lab_node_id)
                    })
                })
        } else {
            None
        };
        let mut jobs = Vec::new();
        let mut graph_save_needed = false;

        for index in 0..batch_count {
            let (inputs, inputs_snapshot, seed_advance) =
                match (batch.seed_strategy, seed_field.as_ref()) {
                    (SeedStrategy::Keep, _) | (_, None) => {
                        (resolved.values.clone(), resolved.snapshot.clone(), None)
                    }
                    (SeedStrategy::Increment, Some(field)) => {
                        let seed = seed_base.unwrap_or(0) + index as i64;
                        let (inputs, inputs_snapshot) =
                            update_seed_inputs(&resolved.values, &resolved.snapshot, field, seed);
                        (
                            inputs,
                            inputs_snapshot,
                            Some(GenerationSeedAdvance {
                                field: field.clone(),
                                next_seed: seed.saturating_add(1),
                            }),
                        )
                    }
                    (SeedStrategy::Random, Some(field)) => {
                        let seed = random_seed_i64();
                        let (inputs, inputs_snapshot) =
                            update_seed_inputs(&resolved.values, &resolved.snapshot, field, seed);
                        (inputs, inputs_snapshot, None)
                    }
                };

            let job_lab_node_id = if activate_on_success {
                let mut node =
                    AssetLabNode::new_with_parent(Some(provider.id), next_parent_node_id);
                node.inputs = inputs_snapshot.clone();
                node.media_bindings = resolved.media_bindings_snapshot.clone();
                let node_id = node.id;
                let updated = self
                    .editor
                    .project
                    .update_generative_config(asset_id, |config| {
                        config.lab_graph.selected_node_id = Some(node_id);
                        config.lab_graph.nodes.push(node);
                    });
                if !updated {
                    return Err("Asset does not support Asset Lab lineage.".to_string());
                }
                next_parent_node_id = Some(node_id);
                graph_save_needed = true;
                Some(node_id)
            } else if let Some(existing_node_id) = lab_node_id {
                if index == 0 {
                    let mut found = false;
                    let updated =
                        self.editor
                            .project
                            .update_generative_config(asset_id, |config| {
                                if let Some(node) = config
                                    .lab_graph
                                    .nodes
                                    .iter_mut()
                                    .find(|node| node.id == existing_node_id)
                                {
                                    found = true;
                                    node.provider_id = Some(provider.id);
                                    node.inputs = inputs_snapshot.clone();
                                    node.media_bindings = resolved.media_bindings_snapshot.clone();
                                    node.output_version = None;
                                    config.lab_graph.selected_node_id = Some(existing_node_id);
                                }
                            });
                    if !updated || !found {
                        return Err("Asset Lab step was not found.".to_string());
                    }
                    graph_save_needed = true;
                    Some(existing_node_id)
                } else {
                    let mut node =
                        AssetLabNode::new_with_parent(Some(provider.id), lab_node_parent_id);
                    node.inputs = inputs_snapshot.clone();
                    node.media_bindings = resolved.media_bindings_snapshot.clone();
                    let node_id = node.id;
                    let updated =
                        self.editor
                            .project
                            .update_generative_config(asset_id, |config| {
                                config.lab_graph.selected_node_id = Some(node_id);
                                config.lab_graph.nodes.push(node);
                            });
                    if !updated {
                        return Err("Asset does not support Asset Lab lineage.".to_string());
                    }
                    graph_save_needed = true;
                    Some(node_id)
                }
            } else {
                None
            };

            jobs.push(GenerationJob {
                id: Uuid::new_v4(),
                created_at: chrono::Utc::now(),
                status: GenerationJobStatus::Queued,
                progress_overall: None,
                progress_node: None,
                attempts: 0,
                next_attempt_at: None,
                provider: provider.clone(),
                output_type: provider.output_type,
                asset_id,
                clip_id: context_clip_id,
                asset_label: asset_label.clone(),
                folder_path: folder_path.clone(),
                inputs,
                inputs_snapshot,
                media_bindings_snapshot: resolved.media_bindings_snapshot.clone(),
                resolved_media_inputs: resolved.resolved_media_inputs.clone(),
                seed_advance,
                version: None,
                lab_node_id: job_lab_node_id,
                activate_on_success,
                error: None,
            });
        }

        if graph_save_needed {
            self.editor
                .project
                .save_generative_config(asset_id)
                .map_err(|err| format!("Failed to save generation lineage: {err}"))?;
        }

        self.editor.generation_queue.extend(jobs);

        let mut status = if batch_count > 1 {
            format!("Queued {batch_count} jobs")
        } else {
            "Queued".to_string()
        };
        if batch_count > 1 {
            if batch.seed_strategy == SeedStrategy::Keep {
                status.push_str(" (identical inputs may be cached)");
            } else if seed_field.is_none() {
                status.push_str(" (seed role missing)");
            } else if seed_base_randomized {
                status.push_str(" (seed missing, randomized base)");
            }
        }
        Ok(status)
    }

    pub(super) fn reserved_seed_base(
        &self,
        asset_id: Uuid,
        seed_field: &str,
        config_seed_base: Option<i64>,
    ) -> Option<i64> {
        self.editor
            .generation_queue
            .iter()
            .filter(|job| {
                job.asset_id == asset_id
                    && matches!(
                        job.status,
                        GenerationJobStatus::Queued
                            | GenerationJobStatus::Running
                            | GenerationJobStatus::Canceling
                    )
            })
            .filter_map(|job| {
                let seed_advance = job.seed_advance.as_ref()?;
                if seed_advance.field == seed_field {
                    Some(seed_advance.next_seed)
                } else {
                    None
                }
            })
            .fold(config_seed_base, |base, reserved_next| {
                Some(base.map_or(reserved_next, |base| base.max(reserved_next)))
            })
    }
}

fn generation_queue_slot_available(generation_active: Option<Uuid>) -> bool {
    generation_active.is_none()
}

fn request_generation_cancellation(
    job: &mut GenerationJob,
    cancel_token: Option<&AtomicBool>,
) -> Result<bool, GenerationJobStatus> {
    if !matches!(
        job.status,
        GenerationJobStatus::Queued | GenerationJobStatus::Running
    ) {
        return Err(job.status);
    }
    let was_running = job.status == GenerationJobStatus::Running;
    job.status = if was_running {
        GenerationJobStatus::Canceling
    } else {
        GenerationJobStatus::Canceled
    };
    job.progress_overall = None;
    job.progress_node = None;
    job.error = Some(if was_running {
        "Canceling; waiting for provider to stop or finish.".to_string()
    } else {
        "Cancelled by user.".to_string()
    });
    if was_running {
        if let Some(token) = cancel_token {
            token.store(true, Ordering::Relaxed);
        }
    }
    Ok(was_running)
}

fn resolve_canceling_completion(
    result: Result<GenerationOutput, GenerationFailure>,
) -> (GenerationJobStatus, String, Option<PathBuf>) {
    match result {
        Ok(output) => (
            GenerationJobStatus::Canceled,
            "Canceled; provider completed before cancellation took effect. Output was not bound."
                .to_string(),
            Some(output.path),
        ),
        Err(GenerationFailure::Canceled) => (
            GenerationJobStatus::Canceled,
            "Generation canceled.".to_string(),
            None,
        ),
        Err(GenerationFailure::Offline(err)) => (
            GenerationJobStatus::Failed,
            format!(
                "Canceling generation could not complete because the provider is offline: {err}"
            ),
            None,
        ),
        Err(GenerationFailure::Error(err)) => (
            GenerationJobStatus::Failed,
            format!("Canceling generation could not complete: {err}"),
            None,
        ),
    }
}

fn cleanup_unpublished_output(path: &PathBuf) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|err| {
        format!(
            "Cancellation completed, but an unbound output could not be removed ({:?}).",
            err.kind()
        )
    })
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;

    fn test_generation_job(status: GenerationJobStatus) -> GenerationJob {
        let provider = ProviderEntry::new(
            "Test provider",
            ProviderOutputType::Image,
            ProviderConnection::CustomHttp {
                base_url: "http://127.0.0.1".to_string(),
                api_key: None,
            },
        );
        GenerationJob {
            id: Uuid::new_v4(),
            created_at: chrono::Utc::now(),
            status,
            progress_overall: Some(0.5),
            progress_node: Some(0.5),
            attempts: 0,
            next_attempt_at: None,
            provider,
            output_type: ProviderOutputType::Image,
            asset_id: Uuid::new_v4(),
            clip_id: None,
            asset_label: "Test asset".to_string(),
            folder_path: PathBuf::new(),
            inputs: HashMap::new(),
            inputs_snapshot: HashMap::new(),
            media_bindings_snapshot: HashMap::new(),
            resolved_media_inputs: HashMap::new(),
            seed_advance: None,
            version: None,
            lab_node_id: None,
            activate_on_success: true,
            error: None,
        }
    }

    #[test]
    fn cancelled_running_job_keeps_the_queue_slot_until_provider_finishes() {
        let mut running = test_generation_job(GenerationJobStatus::Running);
        let queued = test_generation_job(GenerationJobStatus::Queued);
        let cancel = AtomicBool::new(false);

        assert!(request_generation_cancellation(&mut running, Some(&cancel)).expect("cancel"));
        assert_eq!(running.status, GenerationJobStatus::Canceling);
        assert!(cancel.load(Ordering::Relaxed));
        // cancel_generation_job keeps this active id until Finished, so a queued second job
        // cannot overlap a provider job that is still stopping or finishing.
        assert!(!generation_queue_slot_available(Some(running.id)));
        assert_eq!(queued.status, GenerationJobStatus::Queued);
        assert!(generation_queue_slot_available(None));
    }

    #[test]
    fn late_success_while_canceling_is_terminally_canceled_and_never_bound() {
        let path = PathBuf::from("C:/ignored/late-success.png");
        let (status, message, unpublished_output) =
            resolve_canceling_completion(Ok(GenerationOutput {
                version: "v1".to_string(),
                path: path.clone(),
            }));
        assert_eq!(status, GenerationJobStatus::Canceled);
        assert!(message.contains("Output was not bound"));
        assert_eq!(unpublished_output, Some(path));
    }

    #[test]
    fn late_success_output_is_removed_from_disk_before_terminal_cancellation() {
        let path = std::env::temp_dir().join(format!("latentslate-cancel-{}.png", Uuid::new_v4()));
        std::fs::write(&path, b"unbound output").expect("create test output");
        let (_, _, unpublished_output) = resolve_canceling_completion(Ok(GenerationOutput {
            version: "v1".to_string(),
            path: path.clone(),
        }));
        cleanup_unpublished_output(&unpublished_output.expect("late output"))
            .expect("remove unbound output");
        assert!(!path.exists());
    }

    #[test]
    fn cleanup_failure_is_reported_without_disclosing_output_path() {
        let directory =
            std::env::temp_dir().join(format!("latentslate-cancel-dir-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("create test directory");
        let error = cleanup_unpublished_output(&directory).expect_err("directory is not a file");
        assert!(error.contains("could not be removed"));
        assert!(!error.contains(&directory.display().to_string()));
        std::fs::remove_dir(&directory).expect("remove test directory");
    }
}

fn missing_provider_inputs_message(provider: &ProviderEntry, missing: &[String]) -> String {
    let details = missing
        .iter()
        .map(|name| {
            if let Some(input) = provider.inputs.iter().find(|input| input.name == *name) {
                match input.input_type {
                    ProviderInputType::Image => format!(
                        "{name} (image; set patch.media_bindings.{name} source/sample, or a legacy asset_ref)"
                    ),
                    ProviderInputType::Video => format!(
                        "{name} (video; set patch.media_bindings.{name} source/sample, or a legacy asset_ref)"
                    ),
                    ProviderInputType::Audio => format!(
                        "{name} (audio; set patch.media_bindings.{name} source/sample, or a legacy asset_ref)"
                    ),
                    _ => name.clone(),
                }
            } else {
                name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "Missing inputs: {details}. Provider media fields are canonical under patch.media_bindings.<field>; matching inputs/reference_slots still migrate as compatibility aliases."
    )
}
