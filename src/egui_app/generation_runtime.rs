use super::*;
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

impl LatentSlateApp {
    pub(super) fn cancel_generation_job(&mut self, job_id: Uuid) {
        let mut cancelled_label = None;
        if let Some(job) = self
            .editor
            .generation_queue
            .iter_mut()
            .find(|job| job.id == job_id)
        {
            if !matches!(
                job.status,
                GenerationJobStatus::Queued | GenerationJobStatus::Running
            ) {
                return;
            }
            let was_running = job.status == GenerationJobStatus::Running;
            job.status = GenerationJobStatus::Canceled;
            job.progress_overall = None;
            job.progress_node = None;
            job.error = Some("Cancelled by user.".to_string());
            cancelled_label = Some((job.asset_label.clone(), was_running));
        }

        if self.generation_active == Some(job_id) {
            self.generation_active = None;
        }
        if let Some(cancel) = self.generation_cancel_tokens.remove(&job_id) {
            cancel.store(true, Ordering::Relaxed);
        }

        if let Some((label, was_running)) = cancelled_label {
            self.editor.status = if was_running {
                format!("Cancelled generation for {label}; external provider may still finish.")
            } else {
                format!("Removed queued generation for {label}.")
            };
        }
    }

    pub(super) fn service_generation_queue(&mut self, ctx: &Context) {
        while let Ok(event) = self.generation_events_rx.try_recv() {
            self.handle_generation_event(event);
        }

        if self.generation_active.is_none() {
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
                if job_snapshot.is_none()
                    || job_snapshot
                        .as_ref()
                        .is_some_and(|job| job.status == GenerationJobStatus::Canceled)
                {
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
                        node.inputs.insert(seed_advance.field.clone(), next_input);
                    }
                } else {
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
                        node.inputs = job.inputs_snapshot.clone();
                        node.output_version = Some(version.clone());
                    }
                    config.lab_graph.selected_node_id = Some(node_id);
                } else {
                    config.provider_id = Some(job.provider.id);
                    config.active_version = Some(version.clone());
                    config.inputs = job.inputs_snapshot.clone();
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

        self.editor.previewer.invalidate_folder(&job.folder_path);
        self.invalidate_asset_visual_cache(job.asset_id);
        self.editor.preview_dirty = true;
        if self.asset_lab.asset_id == Some(job.asset_id) {
            self.asset_lab.selected_version = Some(version.clone());
            self.asset_lab.pending_delete_version = None;
            self.asset_lab_preview_texture = None;
        }

        if let (Some(runtime), Some(asset)) = (
            self.generation_runtime.as_ref(),
            self.editor.project.find_asset(job.asset_id).cloned(),
        ) {
            let thumbnailer = Arc::clone(&self.editor.thumbnailer);
            runtime.spawn(async move {
                let _ = thumbnailer.generate(&asset, true).await;
            });
        }
    }

    pub(super) fn invalidate_asset_visual_cache(&mut self, asset_id: Uuid) {
        self.asset_thumbnails.remove(&asset_id);
        self.asset_thumbnail_misses.remove(&asset_id);
        self.asset_source_dimensions.remove(&asset_id);
        self.asset_source_dimension_misses.remove(&asset_id);
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
        if provider.output_type == ProviderOutputType::Audio {
            return Err("Audio generation is not supported in the queue yet.".to_string());
        }

        let resolved = resolve_provider_inputs(
            &self.editor.project,
            context_clip_id,
            &provider,
            &config_snapshot,
        );
        if !resolved.missing_required.is_empty() {
            return Err(format!(
                "Missing inputs: {}",
                resolved.missing_required.join(", ")
            ));
        }

        let batch = config_snapshot.batch.clone();
        let batch_count = batch.count.max(1).min(MAX_GENERATION_BATCH_COUNT);
        let seed_field = resolve_seed_field(&provider, batch.seed_field.as_deref());
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

            self.editor.generation_queue.push(GenerationJob {
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
                seed_advance,
                version: None,
                lab_node_id,
                error: None,
            });
        }

        let mut status = if batch_count > 1 {
            format!("Queued {batch_count} jobs")
        } else {
            "Queued".to_string()
        };
        if batch_count > 1 {
            if batch.seed_strategy == SeedStrategy::Keep {
                status.push_str(" (identical inputs may be cached)");
            } else if seed_field.is_none() {
                status.push_str(" (no seed field detected)");
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
                        GenerationJobStatus::Queued | GenerationJobStatus::Running
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
