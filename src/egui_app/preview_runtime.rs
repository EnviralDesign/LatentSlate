use super::*;

impl LatentSlateApp {
    pub(super) fn record_preview_perf_sample(
        &mut self,
        playhead_seconds: f64,
        stats: PreviewStats,
        request_id: Option<u64>,
        render_worker_ms: Option<f64>,
        delivery_ms: Option<f64>,
    ) {
        self.preview_perf_sequence = self.preview_perf_sequence.wrapping_add(1);
        self.preview_perf_samples.push_back(PreviewPerfSample {
            sequence: self.preview_perf_sequence,
            request_id,
            captured_at_ms: chrono::Utc::now().timestamp_millis(),
            playhead_seconds,
            render_worker_ms,
            delivery_ms,
            stats,
        });
        while self.preview_perf_samples.len() > PREVIEW_PERF_HISTORY_LIMIT {
            self.preview_perf_samples.pop_front();
        }
    }

    pub(super) fn clear_project_runtime_cache(&mut self) {
        self.preview_layers = None;
        self.preview_layer_textures.clear();
        self.preview_layer_texture_sequence = 0;
        self.preview_last_render_time = None;
        self.preview_last_interaction = Instant::now();
        self.preview_idle_prefetched_time = None;
        self.preview_prefetch_in_flight
            .store(false, Ordering::Relaxed);
        self.preview_stats = None;
        self.preview_perf_samples.clear();
        self.preview_perf_sequence = 0;
        self.asset_thumbnails.clear();
        self.asset_thumbnail_misses.clear();
        self.asset_source_dimensions.clear();
        self.asset_source_dimension_misses.clear();
        self.timeline_thumbnails.clear();
        self.timeline_thumbnail_misses.clear();
        self.audio_peak_caches.clear();
        self.audio_peak_builds.clear();
        if let Ok(mut cache) = self.audio_sample_cache.lock() {
            cache.clear();
        }
        if let Ok(mut in_flight) = self.audio_decode_in_flight.lock() {
            in_flight.clear();
        }
        if let Ok(mut failures) = self.audio_decode_failures.lock() {
            failures.clear();
        }
        self.audio_decode_warmup_pending = false;
        if let Some(engine) = &self.audio_engine {
            engine.pause();
            engine.set_items(Vec::new());
            engine.set_scrub_hold(false);
        }
        self.editor.is_playing = false;
        self.timeline_drag = None;
        self.timeline_snap_preview = None;
        self.timeline_scrub_was_playing = false;
        self.timeline_last_scrub_audio_time = None;
        self.preview_auto_fit = true;
        self.preview_zoom = 1.0;
        self.preview_pan = Vec2::ZERO;
        self.preview_drag = None;
        self.preview_snap_guides.clear();
        self.generation_active = None;
    }

    pub(super) fn warm_audio_playback_cache(&mut self) {
        let Some(engine) = self.audio_engine.as_ref() else {
            return;
        };
        let Some(project_root) = self.editor.project.project_path.clone() else {
            return;
        };
        let targets = audio_decode_targets_for_project(&self.editor.project, &project_root);
        if targets.is_empty() {
            return;
        }
        let decode_config = AudioDecodeConfig {
            target_rate: engine.sample_rate(),
            target_channels: engine.channels(),
        };
        schedule_audio_decode_targets(
            targets,
            decode_config,
            Arc::clone(&self.audio_sample_cache),
            Arc::clone(&self.audio_decode_in_flight),
            Arc::clone(&self.audio_decode_failures),
        );
        self.audio_decode_warmup_pending = true;
    }

    pub(super) fn service_audio_decode_warmup(&mut self, ctx: &Context) {
        if !self.audio_decode_warmup_pending {
            return;
        }

        self.refresh_audio_playback_items();
        let in_flight = self
            .audio_decode_in_flight
            .lock()
            .ok()
            .map(|in_flight| !in_flight.is_empty())
            .unwrap_or(false);
        if in_flight {
            ctx.request_repaint_after(Duration::from_millis(80));
        } else {
            self.audio_decode_warmup_pending = false;
        }
    }

    pub(super) fn tick_playback(&mut self, ctx: &Context) {
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        if !self.editor.is_playing {
            return;
        }
        let duration = self.editor.project.duration();

        if self.audio_engine.is_some() {
            self.refresh_audio_playback_items();
            let engine = self.audio_engine.as_ref().unwrap();
            let time = engine.playhead_seconds();
            let snapped = snap_time_to_frame(time.min(duration), self.editor.project.settings.fps);
            self.editor.current_time = snapped;
            self.editor.preview_dirty = true;
            if time >= duration {
                engine.pause();
                engine.set_scrub_hold(false);
                self.editor.is_playing = false;
            }
            ctx.request_repaint();
            return;
        }

        let next = self.editor.current_time + delta;
        if next >= duration {
            self.seek_editor(duration, false);
            self.editor.is_playing = false;
        } else {
            self.seek_editor(next, false);
        }
        ctx.request_repaint();
    }

    pub(super) fn seek_editor(&mut self, time: f64, scrub_audio: bool) {
        let duration = self.editor.project.duration();
        let snapped = snap_time_to_frame(
            time.clamp(0.0, duration),
            self.editor.project.settings.fps.max(1.0),
        );
        self.editor.seek(snapped);
        let Some(engine) = self.audio_engine.as_ref().map(Arc::clone) else {
            return;
        };

        if scrub_audio {
            self.refresh_audio_playback_items();
        } else {
            self.timeline_last_scrub_audio_time = None;
        }
        engine.seek_seconds(self.editor.current_time);
        if scrub_audio && !self.editor.is_playing {
            engine.set_scrub_hold(true);
            let frame_epsilon = (0.5 / self.editor.project.settings.fps.max(1.0)).max(0.000_001);
            let should_preview = self
                .timeline_last_scrub_audio_time
                .map(|last| (last - self.editor.current_time).abs() > frame_epsilon)
                .unwrap_or(true);
            if should_preview {
                self.timeline_last_scrub_audio_time = Some(self.editor.current_time);
                engine.trigger_scrub_preview(
                    ((engine.sample_rate() as f64) * TIMELINE_SCRUB_PREVIEW_SECONDS).round() as u64,
                );
                engine.play();
            }
        }
    }

    pub(super) fn toggle_playback(&mut self) {
        let next_playing = !self.editor.is_playing;
        if let Some(engine) = self.audio_engine.as_ref().map(Arc::clone) {
            if next_playing {
                self.refresh_audio_playback_items();
                engine.set_scrub_hold(false);
                self.timeline_last_scrub_audio_time = None;
                engine.seek_seconds(self.editor.current_time);
                engine.play();
            } else {
                engine.set_scrub_hold(false);
                self.timeline_last_scrub_audio_time = None;
                engine.pause();
            }
        }
        self.editor.is_playing = next_playing;
    }

    pub(super) fn refresh_audio_playback_items(&mut self) {
        let Some(engine) = self.audio_engine.as_ref().map(Arc::clone) else {
            return;
        };
        let Some(project_root) = self.editor.project.project_path.clone() else {
            engine.set_items(Vec::new());
            return;
        };

        let project_snapshot = self.editor.project.clone();
        let (items, missing) = build_audio_playback_items(
            &project_snapshot,
            &project_root,
            &engine,
            &self.audio_sample_cache,
            &self.audio_decode_failures,
            false,
        );
        engine.set_items(items);
        if missing.is_empty() {
            return;
        }

        let missing: HashSet<Uuid> = missing.into_iter().collect();
        let mut targets = audio_decode_targets_for_project(&project_snapshot, &project_root);
        targets.retain(|(asset_id, _)| missing.contains(asset_id));
        let decode_config = AudioDecodeConfig {
            target_rate: engine.sample_rate(),
            target_channels: engine.channels(),
        };
        schedule_audio_decode_targets(
            targets,
            decode_config,
            Arc::clone(&self.audio_sample_cache),
            Arc::clone(&self.audio_decode_in_flight),
            Arc::clone(&self.audio_decode_failures),
        );
    }

    pub(super) fn finish_timeline_scrub(&mut self) {
        let Some(engine) = self.audio_engine.as_ref() else {
            self.timeline_scrub_was_playing = false;
            self.timeline_last_scrub_audio_time = None;
            return;
        };
        engine.set_scrub_hold(false);
        self.timeline_last_scrub_audio_time = None;
        if self.timeline_scrub_was_playing {
            engine.seek_seconds(self.editor.current_time);
            engine.play();
            self.editor.is_playing = true;
        } else if !self.editor.is_playing {
            engine.pause();
        }
        self.timeline_scrub_was_playing = false;
    }

    pub(super) fn update_preview_texture(&mut self, ctx: &Context) {
        self.poll_preview_render_results(ctx);
        if !self.editor.preview_dirty && self.preview_layers.is_some() {
            return;
        }
        if self.editor.project.project_path.is_none() {
            self.preview_layers = None;
            return;
        }

        self.schedule_preview_render(ctx);
    }

    pub(super) fn render_preview_sync_for_profile(&mut self, ctx: &Context) -> PreviewStats {
        self.invalidate_preview_render_jobs();
        let decode_mode = if self.editor.is_playing {
            PreviewDecodeMode::Sequential
        } else {
            PreviewDecodeMode::Seek
        };
        let output = self.editor.previewer.render_layers(
            &self.editor.project,
            self.editor.current_time,
            decode_mode,
            self.editor.layout.hardware_decode,
        );
        let mut stats = output.stats;
        let Some(layers) = output.layers else {
            self.preview_layers = None;
            self.preview_stats = Some(stats.clone());
            self.record_preview_perf_sample(
                self.editor.current_time,
                stats.clone(),
                None,
                None,
                None,
            );
            self.editor.preview_dirty = false;
            return stats;
        };

        let upload_start = Instant::now();
        self.prepare_preview_layer_textures(ctx, &layers);
        stats.encode_ms = upload_start.elapsed().as_secs_f64() * 1000.0;
        stats.total_ms += stats.encode_ms;
        self.record_preview_perf_sample(self.editor.current_time, stats.clone(), None, None, None);
        self.preview_stats = Some(stats);
        let direction = self.preview_render_direction(self.editor.current_time);
        self.preview_last_interaction = Instant::now();
        self.preview_idle_prefetched_time = None;
        self.preview_layers = Some(layers);
        self.editor.preview_dirty = false;
        self.schedule_preview_prefetch(direction, decode_mode);
        if !self.editor.is_playing {
            ctx.request_repaint_after(Duration::from_millis(PREVIEW_IDLE_PREFETCH_DELAY_MS + 25));
        }
        self.preview_stats.clone().unwrap_or_default()
    }

    pub(super) fn poll_preview_render_results(&mut self, ctx: &Context) {
        let mut latest = None;
        while let Ok(result) = self.preview_render_rx.try_recv() {
            latest = Some(result);
        }

        let Some(result) = latest else {
            return;
        };

        self.preview_render_busy_since = None;
        let latest_id = self.preview_render_request_id.load(Ordering::Relaxed);
        let time_matches = (result.time_seconds - self.editor.current_time).abs() < 0.0001;
        if result.request_id != latest_id
            || !time_matches
            || self.editor.project.project_path.is_none()
        {
            self.preview_render_stale_count = self.preview_render_stale_count.saturating_add(1);
            self.editor.preview_dirty = true;
            ctx.request_repaint();
            return;
        }

        let worker_ms = result
            .finished_at
            .saturating_duration_since(result.requested_at)
            .as_secs_f64()
            * 1000.0;
        let delivery_ms = result.requested_at.elapsed().as_secs_f64() * 1000.0;
        let mut stats = result.output.stats;
        let Some(layers) = result.output.layers else {
            self.preview_layers = None;
            self.preview_stats = Some(stats.clone());
            self.preview_render_completed_count =
                self.preview_render_completed_count.saturating_add(1);
            self.preview_render_last_worker_ms = Some(worker_ms);
            self.preview_render_last_delivery_ms = Some(delivery_ms);
            self.record_preview_perf_sample(
                result.time_seconds,
                stats,
                Some(result.request_id),
                Some(worker_ms),
                Some(delivery_ms),
            );
            self.editor.preview_dirty = false;
            return;
        };

        let upload_start = Instant::now();
        self.prepare_preview_layer_textures(ctx, &layers);
        stats.encode_ms = upload_start.elapsed().as_secs_f64() * 1000.0;
        stats.total_ms += stats.encode_ms;
        self.preview_stats = Some(stats.clone());
        self.preview_layers = Some(layers);
        self.preview_render_completed_count = self.preview_render_completed_count.saturating_add(1);
        self.preview_render_last_worker_ms = Some(worker_ms);
        self.preview_render_last_delivery_ms = Some(delivery_ms);
        self.record_preview_perf_sample(
            result.time_seconds,
            stats,
            Some(result.request_id),
            Some(worker_ms),
            Some(delivery_ms),
        );
        let direction = self.preview_render_direction(result.time_seconds);
        self.editor.preview_dirty = false;
        self.schedule_preview_prefetch(direction, result.decode_mode);
        if !self.editor.is_playing {
            ctx.request_repaint_after(Duration::from_millis(PREVIEW_IDLE_PREFETCH_DELAY_MS + 25));
        }
    }

    pub(super) fn schedule_preview_render(&mut self, ctx: &Context) {
        if self
            .preview_render_in_flight
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            ctx.request_repaint_after(Duration::from_millis(PREVIEW_RENDER_RETRY_MS));
            return;
        }

        let request_id = self
            .preview_render_request_id
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        let time_seconds = self.editor.current_time;
        let decode_mode = if self.editor.is_playing {
            PreviewDecodeMode::Sequential
        } else {
            PreviewDecodeMode::Seek
        };
        let project = self.editor.project.clone();
        let renderer = Arc::clone(&self.editor.previewer);
        let allow_hw_decode = self.editor.layout.hardware_decode;
        let tx = self.preview_render_tx.clone();
        let flag = Arc::clone(&self.preview_render_in_flight);
        let repaint_ctx = ctx.clone();
        let requested_at = Instant::now();
        self.preview_render_busy_since = Some(requested_at);
        self.preview_last_interaction = requested_at;
        self.preview_idle_prefetched_time = None;

        std::thread::spawn(move || {
            let output =
                renderer.render_layers(&project, time_seconds, decode_mode, allow_hw_decode);
            let finished_at = Instant::now();
            let _ = tx.send(PreviewRenderResult {
                request_id,
                time_seconds,
                decode_mode,
                requested_at,
                finished_at,
                output,
            });
            flag.store(false, Ordering::Relaxed);
            repaint_ctx.request_repaint();
        });
    }

    pub(super) fn invalidate_preview_render_jobs(&mut self) {
        self.preview_render_request_id
            .fetch_add(1, Ordering::Relaxed);
        self.preview_render_in_flight
            .store(false, Ordering::Relaxed);
        self.preview_render_busy_since = None;
        while self.preview_render_rx.try_recv().is_ok() {}
    }

    pub(super) fn preview_render_direction(&mut self, time: f64) -> i32 {
        let direction = match self.preview_last_render_time {
            Some(last) if time > last + 0.0001 => 1,
            Some(last) if time < last - 0.0001 => -1,
            _ => 0,
        };
        self.preview_last_render_time = Some(time);
        direction
    }

    pub(super) fn schedule_preview_prefetch(
        &mut self,
        direction: i32,
        decode_mode: PreviewDecodeMode,
    ) {
        if direction == 0 || self.editor.project.project_path.is_none() {
            return;
        }
        let seconds = if self.editor.is_playing {
            PREVIEW_PREFETCH_PLAYBACK_SECONDS
        } else {
            PREVIEW_PREFETCH_SCRUB_SECONDS
        };
        let fps = self.editor.project.settings.fps.max(1.0);
        let frames = (fps * seconds).round().max(1.0) as u32;
        self.schedule_preview_prefetch_windows(vec![(direction, frames)], decode_mode);
    }

    pub(super) fn schedule_preview_prefetch_windows(
        &mut self,
        windows: Vec<(i32, u32)>,
        decode_mode: PreviewDecodeMode,
    ) -> bool {
        if windows.is_empty() || self.editor.project.project_path.is_none() {
            return false;
        }
        if self
            .preview_prefetch_in_flight
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }

        let project = self.editor.project.clone();
        let renderer = Arc::clone(&self.editor.previewer);
        let time = self.editor.current_time;
        let allow_hw_decode = self.editor.layout.hardware_decode;
        let flag = Arc::clone(&self.preview_prefetch_in_flight);
        std::thread::spawn(move || {
            for (direction, frames) in windows {
                renderer.prefetch_frames(
                    &project,
                    time,
                    direction,
                    frames,
                    decode_mode,
                    allow_hw_decode,
                );
            }
            flag.store(false, Ordering::Relaxed);
        });
        true
    }

    pub(super) fn service_preview_idle_prefetch(&mut self, ctx: &Context) {
        if self.editor.project.project_path.is_none()
            || self.editor.is_playing
            || self.editor.preview_dirty
            || self.preview_layers.is_none()
        {
            return;
        }

        let elapsed = self.preview_last_interaction.elapsed();
        let delay = Duration::from_millis(PREVIEW_IDLE_PREFETCH_DELAY_MS);
        if elapsed < delay {
            ctx.request_repaint_after(delay - elapsed);
            return;
        }

        let time = self.editor.current_time;
        if self
            .preview_idle_prefetched_time
            .map(|last| (last - time).abs() < 0.0001)
            .unwrap_or(false)
        {
            return;
        }

        let fps = self.editor.project.settings.fps.max(1.0);
        let ahead_frames = (fps * PREVIEW_IDLE_PREFETCH_AHEAD_SECONDS).round().max(1.0) as u32;
        let behind_frames = (fps * PREVIEW_IDLE_PREFETCH_BEHIND_SECONDS)
            .round()
            .max(1.0) as u32;
        let scheduled = self.schedule_preview_prefetch_windows(
            vec![(1, ahead_frames), (-1, behind_frames)],
            PreviewDecodeMode::Sequential,
        );
        if scheduled {
            self.preview_idle_prefetched_time = Some(time);
        } else {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    pub(super) fn prepare_preview_layer_textures(
        &mut self,
        ctx: &Context,
        layers: &PreviewLayerStack,
    ) {
        self.preview_layer_texture_sequence = self.preview_layer_texture_sequence.wrapping_add(1);
        let sequence = self.preview_layer_texture_sequence;

        for layer in layers.layers.iter() {
            let size = [
                layer.image.width().max(1) as usize,
                layer.image.height().max(1) as usize,
            ];
            if let Some(existing) = self.preview_layer_textures.get_mut(&layer.texture_key) {
                if existing.size == size {
                    existing.last_used = sequence;
                    continue;
                }
            }

            let image = ColorImage::from_rgba_unmultiplied(size, layer.image.as_raw());
            let texture = ctx.load_texture(
                format!("preview-layer-{}", layer.texture_key),
                image,
                TextureOptions::LINEAR,
            );
            self.preview_layer_textures.insert(
                layer.texture_key,
                PreviewLayerTexture {
                    texture,
                    size,
                    last_used: sequence,
                },
            );
        }

        if self.preview_layer_textures.len() > PREVIEW_LAYER_TEXTURE_LIMIT {
            let mut entries: Vec<(u64, u64)> = self
                .preview_layer_textures
                .iter()
                .map(|(key, texture)| (*key, texture.last_used))
                .collect();
            entries.sort_by_key(|(_, last_used)| *last_used);
            let evict_count = entries
                .len()
                .saturating_sub(PREVIEW_LAYER_TEXTURE_LIMIT)
                .max(PREVIEW_LAYER_TEXTURE_LIMIT / 8);
            for (key, _) in entries.into_iter().take(evict_count) {
                self.preview_layer_textures.remove(&key);
            }
        }
    }

    pub(super) fn asset_thumbnail(
        &mut self,
        ctx: &Context,
        asset: &Asset,
    ) -> Option<(egui::TextureId, Vec2)> {
        if let Some(thumbnail) = self.asset_thumbnails.get(&asset.id) {
            return Some((thumbnail.texture.id(), thumbnail.size));
        }
        if self.asset_thumbnail_misses.contains(&asset.id) {
            return None;
        }

        let project_root = self.editor.project.project_path.as_deref()?;
        for path in asset_thumbnail_candidates(project_root, asset) {
            if let Some((image, size)) = load_thumbnail_image(&path) {
                let texture = ctx.load_texture(
                    format!("asset-thumbnail-{}", asset.id),
                    image,
                    TextureOptions::LINEAR,
                );
                let texture_id = texture.id();
                self.asset_thumbnails
                    .insert(asset.id, AssetThumbnail { texture, size });
                return Some((texture_id, size));
            }
        }

        self.asset_thumbnail_misses.insert(asset.id);
        None
    }

    pub(super) fn asset_source_dimensions(&mut self, asset: &Asset) -> Option<Vec2> {
        if let Some(size) = self.asset_source_dimensions.get(&asset.id) {
            return Some(*size);
        }
        if self.asset_source_dimension_misses.contains(&asset.id) {
            return None;
        }

        let project_root = self.editor.project.project_path.as_deref()?;
        for path in asset_thumbnail_candidates(project_root, asset) {
            if let Ok((width, height)) = image::image_dimensions(&path) {
                let size = Vec2::new(width.max(1) as f32, height.max(1) as f32);
                self.asset_source_dimensions.insert(asset.id, size);
                return Some(size);
            }
        }

        self.asset_source_dimension_misses.insert(asset.id);
        None
    }

    pub(super) fn timeline_thumbnail(
        &mut self,
        ctx: &Context,
        asset: &Asset,
        time_seconds: f64,
    ) -> Option<(egui::TextureId, Vec2)> {
        let bucket_millis = (time_seconds.max(0.0).floor() * 1000.0) as u64;
        let key = TimelineThumbnailKey {
            asset_id: asset.id,
            bucket_millis,
        };
        if let Some(thumbnail) = self.timeline_thumbnails.get(&key) {
            return Some((thumbnail.texture.id(), thumbnail.size));
        }
        if self.timeline_thumbnail_misses.contains(&key) {
            return self.asset_thumbnail(ctx, asset);
        }

        let Some(path) = self
            .editor
            .thumbnailer
            .get_thumbnail_path(asset.id, time_seconds)
        else {
            return self.asset_thumbnail(ctx, asset);
        };
        if let Some((image, size)) = load_thumbnail_image(&path) {
            let texture = ctx.load_texture(
                format!("timeline-thumbnail-{}-{}", asset.id, bucket_millis),
                image,
                TextureOptions::LINEAR,
            );
            let texture_id = texture.id();
            self.timeline_thumbnails
                .insert(key, AssetThumbnail { texture, size });
            return Some((texture_id, size));
        }

        self.timeline_thumbnail_misses.insert(key);
        self.asset_thumbnail(ctx, asset)
    }

    pub(super) fn timeline_clip_thumbnail_tiles(
        &mut self,
        ctx: &Context,
        asset: &Asset,
        clip: &Clip,
        clip_rect: Rect,
        zoom: f32,
    ) -> Vec<TimelineThumbTile> {
        if !asset.is_visual() || clip_rect.width() <= 8.0 {
            return Vec::new();
        }

        let fallback = self.asset_thumbnail(ctx, asset);
        let mut tile_w = TIMELINE_THUMB_TILE_W.max(1.0);
        let estimated = (clip_rect.width() / tile_w).ceil().max(1.0) as usize;
        if estimated > TIMELINE_MAX_THUMB_TILES {
            tile_w = (clip_rect.width() / TIMELINE_MAX_THUMB_TILES as f32)
                .ceil()
                .max(1.0);
        }
        let tile_count = (clip_rect.width() / tile_w).ceil().max(1.0) as usize;
        let tile_time = tile_w as f64 / zoom.max(TIMELINE_MIN_ZOOM_FLOOR) as f64;
        let mut tiles = Vec::with_capacity(tile_count);

        for index in 0..tile_count {
            let time_in_clip = (index as f64 * tile_time).min(clip.duration.max(0.0));
            let source_time = clip.source_time_for_local(time_in_clip, asset.duration_seconds);
            let tile = self
                .timeline_thumbnail(ctx, asset, source_time)
                .or(fallback);
            if let Some((texture_id, size)) = tile {
                tiles.push(TimelineThumbTile { texture_id, size });
            }
        }

        tiles
    }
}
