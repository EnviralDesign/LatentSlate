use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde_json::{json, Value};
use uuid::Uuid;

use crate::constants::{DEFAULT_CLIP_DURATION_SECONDS, PREVIEW_CACHE_BUDGET_BYTES};
use crate::core::automation::{
    AutomationCommand, AutomationResponse, ClipMoveMode, ClipMoveTarget,
};
use crate::core::generation::semantic_reference_slot;
use crate::core::media::{probe_missing_duration, resolve_asset_duration_seconds};
use crate::core::provider_store::{
    default_openai_image_provider_entry, default_provider_entry, default_xai_image_provider_entry,
    default_xai_video_provider_entry, list_local_provider_files,
    load_local_provider_entries_or_empty, provider_path_for_entry, save_local_provider_entry,
};
use crate::core::thumbnailer::Thumbnailer;
use crate::core::timeline_bridge::{provider_is_timeline_bridge, resolve_timeline_bridge_clip};
use crate::state::{
    next_generative_index, Asset, AssetKind, GenerationJob, GenerativeConfig, InputRole,
    InputValue, Project, ProjectProviderScope, ProjectSettings, ProjectWorkspaceLayout,
    ProviderConnection, ProviderEntry, ProviderInputType, ProviderOutputType, SelectionState,
    DEFAULT_GENERATIVE_VIDEO_FPS, DEFAULT_GENERATIVE_VIDEO_FRAME_COUNT,
};

#[derive(Clone, Debug)]
pub struct EditorLayout {
    pub left_collapsed: bool,
    pub right_collapsed: bool,
    pub timeline_collapsed: bool,
    pub preview_stats: bool,
    pub hardware_decode: bool,
    pub left_width: f32,
    pub right_width: f32,
    pub timeline_height: f32,
    pub timeline_zoom: f32,
    pub timeline_scroll_x: f32,
    pub timeline_scroll_y: f32,
}

impl Default for EditorLayout {
    fn default() -> Self {
        Self {
            left_collapsed: false,
            right_collapsed: false,
            timeline_collapsed: false,
            preview_stats: false,
            hardware_decode: true,
            left_width: 250.0,
            right_width: 250.0,
            timeline_height: 220.0,
            timeline_zoom: 4.0,
            timeline_scroll_x: 0.0,
            timeline_scroll_y: 0.0,
        }
    }
}

impl EditorLayout {
    pub fn apply_workspace_layout(&mut self, layout: &ProjectWorkspaceLayout) {
        let defaults = ProjectWorkspaceLayout::default();
        self.left_collapsed = layout.left_collapsed;
        self.right_collapsed = layout.right_collapsed;
        self.timeline_collapsed = layout.timeline_collapsed;
        self.left_width = finite_or(layout.left_width, defaults.left_width).clamp(120.0, 720.0);
        self.right_width = finite_or(layout.right_width, defaults.right_width).clamp(120.0, 720.0);
        self.timeline_height =
            finite_or(layout.timeline_height, defaults.timeline_height).clamp(150.0, 420.0);
        self.timeline_zoom =
            finite_or(layout.timeline_zoom, defaults.timeline_zoom).clamp(0.01, 20_000.0);
        self.timeline_scroll_x = finite_or(layout.timeline_scroll_x, 0.0).max(0.0);
        self.timeline_scroll_y = finite_or(layout.timeline_scroll_y, 0.0).max(0.0);
    }

    pub fn workspace_layout(&self) -> ProjectWorkspaceLayout {
        ProjectWorkspaceLayout {
            left_collapsed: self.left_collapsed,
            right_collapsed: self.right_collapsed,
            timeline_collapsed: self.timeline_collapsed,
            left_width: finite_or(self.left_width, 250.0).clamp(120.0, 720.0),
            right_width: finite_or(self.right_width, 250.0).clamp(120.0, 720.0),
            timeline_height: finite_or(self.timeline_height, 220.0).clamp(150.0, 420.0),
            timeline_zoom: finite_or(self.timeline_zoom, 4.0).clamp(0.01, 20_000.0),
            timeline_scroll_x: finite_or(self.timeline_scroll_x, 0.0).max(0.0),
            timeline_scroll_y: finite_or(self.timeline_scroll_y, 0.0).max(0.0),
        }
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

#[derive(Clone, Debug, Default)]
pub struct EditorOverlays {
    pub startup: bool,
    pub providers: bool,
    pub project_settings: bool,
    pub new_project: bool,
    pub queue: bool,
    pub agent_api: bool,
    pub generative_video: bool,
    pub export_video: bool,
    pub asset_lab: bool,
}

pub struct EditorState {
    pub project: Project,
    pub selection: SelectionState,
    pub provider_entries: Vec<ProviderEntry>,
    pub provider_files: Vec<PathBuf>,
    pub thumbnailer: Arc<Thumbnailer>,
    pub previewer: Arc<crate::core::preview::PreviewRenderer>,
    pub current_time: f64,
    pub is_playing: bool,
    pub startup_done: bool,
    pub layout: EditorLayout,
    pub overlays: EditorOverlays,
    pub generation_queue: Vec<GenerationJob>,
    pub status: String,
    pub preview_dirty: bool,
    pub project_dirty: bool,
    project_saved_fingerprint: Option<String>,
}

impl EditorState {
    pub fn new() -> Self {
        let scratch = crate::core::paths::app_cache_root().join("scratch");
        Self {
            project: Project::default(),
            selection: SelectionState::default(),
            provider_entries: load_local_provider_entries_or_empty(),
            provider_files: list_local_provider_files(),
            thumbnailer: Arc::new(Thumbnailer::new(scratch.clone())),
            previewer: Arc::new(crate::core::preview::PreviewRenderer::new_with_limits(
                scratch,
                PREVIEW_CACHE_BUDGET_BYTES,
                ProjectSettings::default().preview_max_width,
                ProjectSettings::default().preview_max_height,
            )),
            current_time: 0.0,
            is_playing: false,
            startup_done: false,
            layout: EditorLayout::default(),
            overlays: EditorOverlays {
                startup: true,
                ..Default::default()
            },
            generation_queue: Vec::new(),
            status: "Ready".to_string(),
            preview_dirty: true,
            project_dirty: false,
            project_saved_fingerprint: None,
        }
    }

    pub fn show_startup(&self) -> bool {
        self.project.project_path.is_none() && !self.startup_done
    }

    pub fn refresh_providers(&mut self) {
        self.provider_entries = load_local_provider_entries_or_empty();
        self.provider_files = list_local_provider_files();
    }

    pub fn provider_in_project_scope(&self, provider_id: Uuid) -> bool {
        self.project.settings.provider_in_scope(provider_id)
    }

    pub fn project_scoped_provider_entries(&self) -> Vec<ProviderEntry> {
        self.provider_entries
            .iter()
            .filter(|provider| self.provider_in_project_scope(provider.id))
            .cloned()
            .collect()
    }

    pub fn sync_timeline_bridge_clips(&mut self) -> bool {
        let bridge_clip_ids: Vec<Uuid> = self
            .project
            .clips
            .iter()
            .filter(|clip| clip.bridge.is_some())
            .map(|clip| clip.id)
            .collect();
        let mut changed = false;
        for clip_id in bridge_clip_ids {
            changed |= self.sync_timeline_bridge_clip(clip_id);
        }
        if changed {
            self.preview_dirty = true;
        }
        changed
    }

    pub fn sync_timeline_bridge_clip(&mut self, clip_id: Uuid) -> bool {
        let Some(clip_snapshot) = self
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
        else {
            return false;
        };
        if clip_snapshot.bridge.is_none() {
            return false;
        }
        let Some(config) = self
            .project
            .generative_config(clip_snapshot.asset_id)
            .cloned()
        else {
            return false;
        };
        let provider = config.provider_id.and_then(|provider_id| {
            self.provider_entries
                .iter()
                .find(|provider| provider.id == provider_id)
        });
        if !provider.is_some_and(provider_is_timeline_bridge) {
            return false;
        }

        let resolution =
            resolve_timeline_bridge_clip(&self.project, provider, Some(&config), &clip_snapshot);
        if (resolution.start_time - clip_snapshot.start_time).abs() <= f64::EPSILON
            && (resolution.duration - clip_snapshot.duration).abs() <= f64::EPSILON
        {
            return false;
        }
        if let Some(clip) = self
            .project
            .clips
            .iter_mut()
            .find(|clip| clip.id == clip_id)
        {
            clip.start_time = resolution.start_time.max(0.0);
            clip.duration = resolution.duration.max(0.1);
            return true;
        }
        false
    }

    fn redacted_provider_entries_for_scope(&self, include_all: bool) -> Vec<Value> {
        self.provider_entries
            .iter()
            .filter(|provider| include_all || self.provider_in_project_scope(provider.id))
            .map(redacted_provider_entry_json)
            .collect()
    }

    pub fn project_root(&self) -> Option<&Path> {
        self.project.project_path.as_deref()
    }

    pub fn project_name(&self) -> &str {
        &self.project.name
    }

    pub fn create_project(
        &mut self,
        parent_dir: impl AsRef<Path>,
        name: impl Into<String>,
        settings: ProjectSettings,
    ) -> Result<PathBuf, String> {
        let name = name.into();
        let project_dir = parent_dir.as_ref().join(&name);
        let preview_limits = (settings.preview_max_width, settings.preview_max_height);
        let project = Project::create_in_with_settings(&project_dir, &name, settings)
            .map_err(|err| format!("Failed to create project: {err}"))?;
        let project_root = project
            .project_path
            .clone()
            .ok_or_else(|| "Created project has no project path.".to_string())?;
        self.set_project(project, project_root.clone(), preview_limits);
        Ok(project_root)
    }

    pub fn open_project(&mut self, folder: impl AsRef<Path>) -> Result<PathBuf, String> {
        let project = Project::load(folder.as_ref())
            .map_err(|err| format!("Failed to open project: {err}"))?;
        let project_root = project
            .project_path
            .clone()
            .ok_or_else(|| "Loaded project has no project path.".to_string())?;
        let preview_limits = (
            project.settings.preview_max_width,
            project.settings.preview_max_height,
        );
        self.set_project(project, project_root.clone(), preview_limits);
        Ok(project_root)
    }

    fn set_project(&mut self, project: Project, project_root: PathBuf, preview_limits: (u32, u32)) {
        self.layout
            .apply_workspace_layout(&project.workspace_layout);
        self.thumbnailer = Arc::new(Thumbnailer::new(project_root.clone()));
        self.previewer = Arc::new(crate::core::preview::PreviewRenderer::new_with_limits(
            project_root,
            PREVIEW_CACHE_BUDGET_BYTES,
            preview_limits.0,
            preview_limits.1,
        ));
        self.project = project;
        self.refresh_providers();
        self.current_time = 0.0;
        self.selection.clear();
        self.startup_done = true;
        self.overlays.startup = false;
        self.status = "Project loaded".to_string();
        self.preview_dirty = true;
        probe_missing_duration(&mut self.project);
        self.mark_project_clean();
    }

    fn project_fingerprint(&self) -> Option<String> {
        if self.project.project_path.is_none() {
            return None;
        }

        let mut project = self.project.clone();
        project.workspace_layout = self.layout.workspace_layout();
        let project_value = serde_json::to_value(&project).ok()?;
        let mut generative_configs: Vec<_> = self
            .project
            .generative_configs
            .iter()
            .filter_map(|(asset_id, config)| {
                serde_json::to_value(config)
                    .ok()
                    .map(|value| (asset_id.to_string(), value))
            })
            .collect();
        generative_configs.sort_by(|(a, _), (b, _)| a.cmp(b));

        serde_json::to_string(&json!({
            "project": project_value,
            "generative_configs": generative_configs,
        }))
        .ok()
    }

    pub fn refresh_project_dirty_state(&mut self) {
        let Some(current) = self.project_fingerprint() else {
            self.project_dirty = false;
            self.project_saved_fingerprint = None;
            return;
        };

        self.project_dirty = self
            .project_saved_fingerprint
            .as_ref()
            .is_some_and(|saved| saved != &current);
    }

    fn mark_project_clean(&mut self) {
        self.project_saved_fingerprint = self.project_fingerprint();
        self.project_dirty = false;
    }

    pub fn import_asset(&mut self, path: impl AsRef<Path>) -> Result<Uuid, String> {
        let asset_id = self
            .project
            .import_file(path.as_ref())
            .map_err(|err| format!("Failed to import asset: {err}"))?;
        self.preview_dirty = true;
        self.status = "Asset imported".to_string();
        Ok(asset_id)
    }

    pub fn rename_asset(&mut self, asset_id: Uuid, name: impl Into<String>) -> Result<(), String> {
        let name = name.into();
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("Asset name cannot be empty.".to_string());
        }
        if !self.project.rename_asset(asset_id, trimmed.to_string()) {
            return Err("Asset not found.".to_string());
        }
        self.status = "Asset renamed".to_string();
        Ok(())
    }

    pub fn duplicate_asset(&mut self, asset_id: Uuid) -> Result<Uuid, String> {
        let new_ids = self.duplicate_assets(&[asset_id])?;
        new_ids
            .first()
            .copied()
            .ok_or_else(|| "Asset could not be duplicated.".to_string())
    }

    pub fn duplicate_assets(&mut self, asset_ids: &[Uuid]) -> Result<Vec<Uuid>, String> {
        let mut unique_asset_ids = Vec::new();
        for asset_id in asset_ids {
            if !unique_asset_ids.contains(asset_id) {
                unique_asset_ids.push(*asset_id);
            }
        }
        if unique_asset_ids.is_empty() {
            return Err("No assets selected.".to_string());
        }

        let mut duplicated = Vec::new();
        for asset_id in unique_asset_ids {
            duplicated.push(self.duplicate_asset_inner(asset_id)?);
        }

        self.selection.clear();
        self.selection.asset_ids = duplicated.clone();
        self.preview_dirty = true;
        self.status = if duplicated.len() == 1 {
            "Duplicated asset".to_string()
        } else {
            format!("Duplicated {} assets", duplicated.len())
        };
        Ok(duplicated)
    }

    pub fn extract_active_generation_as_asset(&mut self, asset_id: Uuid) -> Result<Uuid, String> {
        self.extract_generation_version_as_asset(asset_id, None)
    }

    pub fn extract_generation_version_as_asset(
        &mut self,
        asset_id: Uuid,
        version: Option<&str>,
    ) -> Result<Uuid, String> {
        let project_root = self
            .project
            .project_path
            .clone()
            .ok_or_else(|| "Create or open a project first.".to_string())?;
        let source_asset = self
            .project
            .find_asset(asset_id)
            .cloned()
            .ok_or_else(|| "Asset not found.".to_string())?;

        let Some(source) = generative_source_for_extraction(&project_root, &source_asset, version)
        else {
            return Err("No active generation file was found for this asset.".to_string());
        };

        let extension = source
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or(source.default_extension)
            .to_ascii_lowercase();
        let target_dir = project_root.join(source.target_subdir);
        fs::create_dir_all(&target_dir)
            .map_err(|err| format!("Failed to create asset folder: {err}"))?;

        let version_label = source.version.as_deref().unwrap_or_else(|| {
            source
                .path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("output")
        });
        let base_name = format!("{} {}", source_asset.name, version_label);
        let target_path =
            unique_file_path(&target_dir, &sanitize_file_stem(&base_name), &extension);
        fs::copy(&source.path, &target_path)
            .map_err(|err| format!("Failed to extract generated output: {err}"))?;

        let relative_path = PathBuf::from(source.target_subdir).join(
            target_path
                .file_name()
                .ok_or_else(|| "Extracted output path has no filename.".to_string())?,
        );
        let name = unique_asset_name(&self.project.assets, &base_name);
        let mut asset = match source.output {
            ExtractedAssetOutput::Image => Asset::new_image(name, relative_path),
            ExtractedAssetOutput::Video => Asset::new_video(name, relative_path),
            ExtractedAssetOutput::Audio => Asset::new_audio(name, relative_path),
        };
        asset.duration_seconds = source_asset.duration_seconds;
        let new_id = self.project.add_asset(asset);
        self.selection.select_asset(new_id);
        self.preview_dirty = true;
        self.status = "Extracted generation as asset".to_string();
        Ok(new_id)
    }

    pub fn add_extracted_frame_asset(
        &mut self,
        source_asset_id: Uuid,
        version: Option<&str>,
        time_seconds: f64,
        frame: image::RgbaImage,
    ) -> Result<Uuid, String> {
        let project_root = self
            .project
            .project_path
            .clone()
            .ok_or_else(|| "Create or open a project first.".to_string())?;
        let source_asset = self
            .project
            .find_asset(source_asset_id)
            .cloned()
            .ok_or_else(|| "Asset not found.".to_string())?;
        let target_dir = project_root.join("images");
        fs::create_dir_all(&target_dir)
            .map_err(|err| format!("Failed to create image asset folder: {err}"))?;

        let version_label = version.unwrap_or("frame");
        let centiseconds = (time_seconds.max(0.0) * 100.0).round() as u64;
        let time_label = format!(
            "{:02}_{:02}_{:02}",
            centiseconds / 360_000,
            (centiseconds / 6_000) % 60,
            (centiseconds / 100) % 60
        );
        let base_name = format!("{} {} {}", source_asset.name, version_label, time_label);
        let target_path = unique_file_path(&target_dir, &sanitize_file_stem(&base_name), "png");
        frame
            .save(&target_path)
            .map_err(|err| format!("Failed to save extracted frame: {err}"))?;

        let relative_path = PathBuf::from("images").join(
            target_path
                .file_name()
                .ok_or_else(|| "Extracted frame path has no filename.".to_string())?,
        );
        let name = unique_asset_name(&self.project.assets, &base_name);
        let asset = Asset::new_image(name, relative_path);
        let new_id = self.project.add_asset(asset);
        self.selection.select_asset(new_id);
        self.preview_dirty = true;
        self.status = "Extracted frame as image asset".to_string();
        Ok(new_id)
    }

    pub fn add_rendered_frame_asset(
        &mut self,
        base_name: &str,
        frame: image::RgbaImage,
    ) -> Result<Uuid, String> {
        let project_root = self
            .project
            .project_path
            .clone()
            .ok_or_else(|| "Create or open a project first.".to_string())?;
        let target_dir = project_root.join("images");
        fs::create_dir_all(&target_dir)
            .map_err(|err| format!("Failed to create image asset folder: {err}"))?;

        let base_name = if base_name.trim().is_empty() {
            "Extracted Still"
        } else {
            base_name.trim()
        };
        let target_path = unique_file_path(&target_dir, &sanitize_file_stem(base_name), "png");
        frame
            .save(&target_path)
            .map_err(|err| format!("Failed to save extracted frame: {err}"))?;

        let relative_path = PathBuf::from("images").join(
            target_path
                .file_name()
                .ok_or_else(|| "Extracted frame path has no filename.".to_string())?,
        );
        let name = unique_asset_name(&self.project.assets, base_name);
        let asset = Asset::new_image(name, relative_path);
        let new_id = self.project.add_asset(asset);
        self.selection.select_asset(new_id);
        self.preview_dirty = true;
        self.status = "Extracted frame as image asset".to_string();
        Ok(new_id)
    }

    pub fn add_asset_to_timeline(
        &mut self,
        asset_id: Uuid,
        time: Option<f64>,
    ) -> Result<Uuid, String> {
        let time = time.unwrap_or(self.current_time);
        let duration = resolve_asset_duration_seconds(&mut self.project, asset_id)
            .unwrap_or(DEFAULT_CLIP_DURATION_SECONDS);
        let clip_id = self
            .project
            .add_clip_from_asset(asset_id, time, duration)
            .ok_or_else(|| {
                "Asset could not be placed on a compatible timeline track.".to_string()
            })?;
        self.selection.select_clip(clip_id);
        self.preview_dirty = true;
        Ok(clip_id)
    }

    pub fn add_asset_to_timeline_track(
        &mut self,
        asset_id: Uuid,
        track_id: Uuid,
        time: Option<f64>,
    ) -> Result<Uuid, String> {
        let time = time.unwrap_or(self.current_time);
        let duration = resolve_asset_duration_seconds(&mut self.project, asset_id)
            .unwrap_or(DEFAULT_CLIP_DURATION_SECONDS);
        let clip_id = self
            .project
            .add_clip_from_asset_to_track(asset_id, track_id, time, duration)
            .ok_or_else(|| "Asset could not be placed on that timeline track.".to_string())?;
        self.selection.select_clip(clip_id);
        self.preview_dirty = true;
        Ok(clip_id)
    }

    pub fn seek(&mut self, time: f64) {
        let duration = self.project.duration();
        let fps = self.project.settings.fps.max(1.0);
        let frame = (time * fps).round();
        self.current_time = (frame / fps).clamp(0.0, duration);
        self.preview_dirty = true;
    }

    pub fn add_marker(&mut self, time: Option<f64>) -> Uuid {
        self.add_marker_to_track(time, None)
    }

    pub fn add_marker_to_track(&mut self, time: Option<f64>, track_id: Option<Uuid>) -> Uuid {
        let time = time.unwrap_or(self.current_time);
        let mut marker = crate::state::Marker::new(time.clamp(0.0, self.project.duration()));
        marker.track_id = track_id
            .filter(|id| {
                self.project.tracks.iter().any(|track| {
                    track.id == *id && track.track_type == crate::state::TrackType::Marker
                })
            })
            .or_else(|| {
                self.selection.primary_track().filter(|id| {
                    self.project.tracks.iter().any(|track| {
                        track.id == *id && track.track_type == crate::state::TrackType::Marker
                    })
                })
            });
        let id = self.project.add_marker(marker);
        self.selection.select_marker(id);
        self.preview_dirty = true;
        id
    }

    pub fn save(&mut self) -> Result<(), String> {
        self.project.workspace_layout = self.layout.workspace_layout();
        self.project
            .save()
            .map_err(|err| format!("Failed to save project: {err}"))?;
        self.mark_project_clean();
        self.status = "Saved".to_string();
        Ok(())
    }

    pub fn apply_project_settings_patch(
        &mut self,
        patch: &crate::core::automation::ProjectSettingsPatch,
    ) -> ProjectSettings {
        if let Some(width) = patch.width {
            self.project.settings.width = width.max(1);
        }
        if let Some(height) = patch.height {
            self.project.settings.height = height.max(1);
        }
        if let Some(fps) = patch.fps {
            if fps.is_finite() && fps > 0.0 {
                self.project.settings.fps = fps;
            }
        }
        if let Some(duration) = patch.duration_seconds {
            if duration.is_finite() {
                self.project.settings.duration_seconds = duration.max(0.0);
            }
        }
        if let Some(width) = patch.preview_max_width {
            self.project.settings.preview_max_width = width.max(1);
        }
        if let Some(height) = patch.preview_max_height {
            self.project.settings.preview_max_height = height.max(1);
        }
        if let Some(provider_scope) = patch.provider_scope.clone() {
            self.project.settings.provider_scope = normalize_project_provider_scope(provider_scope);
        }

        let project_root = self
            .project
            .project_path
            .clone()
            .unwrap_or_else(|| crate::core::paths::app_cache_root().join("scratch"));
        self.previewer = Arc::new(crate::core::preview::PreviewRenderer::new_with_limits(
            project_root,
            PREVIEW_CACHE_BUDGET_BYTES,
            self.project.settings.preview_max_width,
            self.project.settings.preview_max_height,
        ));
        self.preview_dirty = true;
        self.status = "Project settings updated".to_string();
        self.project.settings.clone()
    }

    pub fn save_provider_entry(
        &mut self,
        provider: ProviderEntry,
    ) -> Result<ProviderEntry, String> {
        save_local_provider_entry(&provider)
            .map_err(|err| format!("Failed to save provider: {err}"))?;
        self.refresh_providers();
        self.status = "Provider saved".to_string();
        Ok(provider)
    }

    pub fn delete_provider_entry(&mut self, provider_id: Uuid) -> Result<ProviderEntry, String> {
        let provider = self
            .provider_entries
            .iter()
            .find(|provider| provider.id == provider_id)
            .cloned()
            .ok_or_else(|| "Provider not found.".to_string())?;
        let path = provider_path_for_entry(&provider);
        if let Err(err) = fs::remove_file(&path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                return Err(format!(
                    "Failed to delete provider {}: {err}",
                    path.display()
                ));
            }
        }
        self.refresh_providers();
        self.status = "Provider deleted".to_string();
        Ok(provider)
    }

    pub fn create_generative_image(&mut self) -> Result<Uuid, String> {
        let project_root = self
            .project
            .project_path
            .clone()
            .ok_or_else(|| "Create or open a project first.".to_string())?;
        let index = next_generative_index(&self.project.assets, "Gen Image", is_generative_image);
        let asset_id = Uuid::new_v4();
        let folder = generative_asset_folder("image", asset_id);
        std::fs::create_dir_all(project_root.join(&folder))
            .map_err(|err| format!("Failed to create generated image folder: {err}"))?;
        let mut asset = Asset::new_generative_image(format!("Gen Image {index}"), folder);
        asset.id = asset_id;
        let id = self.project.add_asset(asset);
        self.preview_dirty = true;
        Ok(id)
    }

    pub fn create_generative_audio(&mut self) -> Result<Uuid, String> {
        let project_root = self
            .project
            .project_path
            .clone()
            .ok_or_else(|| "Create or open a project first.".to_string())?;
        let index = next_generative_index(&self.project.assets, "Gen Audio", is_generative_audio);
        let asset_id = Uuid::new_v4();
        let folder = generative_asset_folder("audio", asset_id);
        std::fs::create_dir_all(project_root.join(&folder))
            .map_err(|err| format!("Failed to create generated audio folder: {err}"))?;
        let mut asset = Asset::new_generative_audio(format!("Gen Audio {index}"), folder);
        asset.id = asset_id;
        let id = self.project.add_asset(asset);
        self.preview_dirty = true;
        Ok(id)
    }

    pub fn create_generative_video(&mut self, fps: f64, frame_count: u32) -> Result<Uuid, String> {
        let project_root = self
            .project
            .project_path
            .clone()
            .ok_or_else(|| "Create or open a project first.".to_string())?;
        let index = next_generative_index(&self.project.assets, "Gen Video", is_generative_video);
        let asset_id = Uuid::new_v4();
        let folder = generative_asset_folder("video", asset_id);
        std::fs::create_dir_all(project_root.join(&folder))
            .map_err(|err| format!("Failed to create generated video folder: {err}"))?;
        let fps = if fps.is_finite() && fps > 0.0 {
            fps
        } else {
            DEFAULT_GENERATIVE_VIDEO_FPS
        };
        let frame_count = frame_count.max(1);
        let mut asset =
            Asset::new_generative_video(format!("Gen Video {index}"), folder, fps, frame_count);
        asset.id = asset_id;
        let id = self.project.add_asset(asset);
        self.preview_dirty = true;
        Ok(id)
    }

    pub fn selected_asset_id(&self) -> Option<Uuid> {
        self.selection.asset_ids.first().copied()
    }

    pub fn selected_clip_id(&self) -> Option<Uuid> {
        self.selection.primary_clip()
    }

    pub fn selected_marker_id(&self) -> Option<Uuid> {
        self.selection.primary_marker()
    }

    pub fn selected_track_id(&self) -> Option<Uuid> {
        self.selection.primary_track()
    }

    pub fn delete_selected_clips(&mut self) -> usize {
        let clip_ids = self.selection.clip_ids.clone();
        if clip_ids.is_empty() {
            return 0;
        }

        let mut deleted = 0usize;
        for clip_id in clip_ids {
            if self.project.remove_clip(clip_id) {
                deleted += 1;
            }
        }

        if deleted > 0 {
            self.selection.clear();
            self.preview_dirty = true;
            self.status = if deleted == 1 {
                "Deleted clip".to_string()
            } else {
                format!("Deleted {deleted} clips")
            };
        }

        deleted
    }

    pub fn delete_assets(&mut self, asset_ids: &[Uuid]) -> (usize, usize) {
        if asset_ids.is_empty() {
            return (0, 0);
        }

        let mut unique_asset_ids = Vec::new();
        for asset_id in asset_ids {
            if !unique_asset_ids.contains(asset_id) {
                unique_asset_ids.push(*asset_id);
            }
        }

        let removed_clips = self
            .project
            .clips
            .iter()
            .filter(|clip| unique_asset_ids.contains(&clip.asset_id))
            .count();

        let project_root = self.project.project_path.clone();
        let mut removed_assets = 0usize;
        let mut folders_to_delete = Vec::new();
        for asset_id in unique_asset_ids {
            let folder_to_delete = project_root.as_ref().and_then(|root| {
                self.project
                    .find_asset(asset_id)
                    .and_then(generative_folder)
                    .and_then(|folder| project_local_folder(root, folder))
            });
            if self.project.remove_asset(asset_id) {
                removed_assets += 1;
                if let Some(folder) = folder_to_delete {
                    folders_to_delete.push(folder);
                }
            }
        }

        if removed_assets > 0 {
            self.previewer.release_media_handles();
            for folder in folders_to_delete {
                self.previewer.invalidate_folder(&folder);
                if let Err(err) = fs::remove_dir_all(&folder) {
                    if err.kind() != std::io::ErrorKind::NotFound {
                        println!(
                            "Failed to delete generated folder {}: {err}",
                            folder.display()
                        );
                    }
                }
            }
            self.selection.clear();
            self.preview_dirty = true;
            self.status = deleted_assets_status(removed_assets, removed_clips);
        }

        (removed_assets, removed_clips)
    }

    pub fn apply_automation_command(&mut self, command: &AutomationCommand) -> AutomationResponse {
        match command {
            AutomationCommand::GetState { include } => {
                AutomationResponse::ok(self.state_json(include))
            }
            AutomationCommand::GetCapabilities => {
                AutomationResponse::ok(crate::core::automation::agent_capabilities_json())
            }
            AutomationCommand::ListProjects { root } => match list_project_folders(root.as_deref())
            {
                Ok(projects) => AutomationResponse::ok(projects),
                Err(err) => AutomationResponse::error(err),
            },
            AutomationCommand::GetUi
            | AutomationCommand::ClickUi { .. }
            | AutomationCommand::TextUi { .. }
            | AutomationCommand::Screenshot { .. }
            | AutomationCommand::GetPerformanceDiagnostics
            | AutomationCommand::ScrubTimelineProfile { .. }
            | AutomationCommand::Capture { .. }
            | AutomationCommand::CreateI2iFromClip { .. }
            | AutomationCommand::CreateI2vFromClip { .. }
            | AutomationCommand::CreateBridgeFromClips { .. }
            | AutomationCommand::ExtractStillToAsset { .. }
            | AutomationCommand::SetPlayback { .. }
            | AutomationCommand::StepTimeline { .. }
            | AutomationCommand::StartGeneration { .. }
            | AutomationCommand::CancelJob { .. }
            | AutomationCommand::ExportVideo { .. }
            | AutomationCommand::GetExportStatus
            | AutomationCommand::CancelExport
            | AutomationCommand::TestProvider { .. }
            | AutomationCommand::SetActiveGenerationVersion { .. }
            | AutomationCommand::DuplicateGenerationVersion { .. }
            | AutomationCommand::DeleteGenerationVersion { .. }
            | AutomationCommand::GetAssetLabGraph { .. }
            | AutomationCommand::AddAssetLabNode { .. }
            | AutomationCommand::SetAssetLabNode { .. }
            | AutomationCommand::DeleteAssetLabNode { .. }
            | AutomationCommand::GenerateAssetLabNode { .. } => AutomationResponse::with_status(
                "Runtime automation commands must be handled by the egui runtime.",
                500,
            ),
            AutomationCommand::CreateProject {
                parent_dir,
                name,
                settings,
            } => match self.create_project(
                parent_dir,
                name.clone(),
                settings.clone().unwrap_or_default(),
            ) {
                Ok(path) => AutomationResponse::ok(json!({ "project_path": path })),
                Err(err) => AutomationResponse::error(err),
            },
            AutomationCommand::OpenProject { folder } => match self.open_project(folder) {
                Ok(path) => AutomationResponse::ok(json!({ "project_path": path })),
                Err(err) => AutomationResponse::error(err),
            },
            AutomationCommand::SetProjectSettings { patch } => {
                let settings = self.apply_project_settings_patch(patch);
                AutomationResponse::ok(json!({ "settings": settings }))
            }
            AutomationCommand::ImportAsset { path } => match self.import_asset(path) {
                Ok(asset_id) => AutomationResponse::ok(json!({ "asset_id": asset_id })),
                Err(err) => AutomationResponse::error(err),
            },
            AutomationCommand::RenameAsset {
                asset_id,
                asset_name,
                name,
            } => {
                let selected = self.resolve_asset_or_selected(*asset_id, asset_name.as_deref());
                match selected {
                    Some(asset_id) => match self.rename_asset(asset_id, name.clone()) {
                        Ok(()) => AutomationResponse::ok(json!({ "asset_id": asset_id })),
                        Err(err) => AutomationResponse::error(err),
                    },
                    None => AutomationResponse::error("No matching asset found."),
                }
            }
            AutomationCommand::DuplicateAsset {
                asset_id,
                asset_name,
            } => {
                let selected = self.resolve_asset_or_selected(*asset_id, asset_name.as_deref());
                match selected {
                    Some(asset_id) => match self.duplicate_asset(asset_id) {
                        Ok(new_asset_id) => {
                            AutomationResponse::ok(json!({ "asset_id": new_asset_id }))
                        }
                        Err(err) => AutomationResponse::error(err),
                    },
                    None => AutomationResponse::error("No matching asset found."),
                }
            }
            AutomationCommand::DeleteAssets { asset_ids } => {
                let missing: Vec<_> = asset_ids
                    .iter()
                    .filter(|asset_id| self.project.find_asset(**asset_id).is_none())
                    .copied()
                    .collect();
                if !missing.is_empty() {
                    return AutomationResponse::not_found(format!(
                        "Assets not found: {}",
                        missing
                            .iter()
                            .map(Uuid::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                let (removed_assets, removed_clips) = self.delete_assets(asset_ids);
                AutomationResponse::ok(json!({
                    "removed_assets": removed_assets,
                    "removed_clips": removed_clips,
                }))
            }
            AutomationCommand::SetAssetDuration {
                asset_id,
                duration_seconds,
            } => {
                if self
                    .project
                    .set_asset_duration(*asset_id, *duration_seconds)
                {
                    let _ = sync_generative_video_timing_inputs(
                        &mut self.project,
                        &self.provider_entries,
                        *asset_id,
                    )
                    .map_err(|err| {
                        self.status =
                            format!("Updated duration, but timing input sync failed: {err}");
                    });
                    self.preview_dirty = true;
                    AutomationResponse::ok(json!({ "asset_id": asset_id }))
                } else {
                    AutomationResponse::not_found("Asset not found.")
                }
            }
            AutomationCommand::SetGenerativeVideoTiming {
                asset_id,
                fps,
                duration_seconds,
                frame_count,
            } => {
                let Some(asset) = self.project.find_asset(*asset_id) else {
                    return AutomationResponse::not_found("Asset not found.");
                };
                let AssetKind::GenerativeVideo {
                    fps: current_fps,
                    frame_count: current_frame_count,
                    ..
                } = &asset.kind
                else {
                    return AutomationResponse::conflict("Asset is not a generative video.");
                };
                let next_fps = fps.unwrap_or(*current_fps).max(1.0);
                let next_frame_count = frame_count
                    .or_else(|| {
                        duration_seconds.map(|duration| {
                            (duration.max(1.0 / next_fps) * next_fps).round() as u32
                        })
                    })
                    .unwrap_or(*current_frame_count)
                    .max(1);
                if self
                    .project
                    .set_generative_video_timing(*asset_id, next_fps, next_frame_count)
                {
                    let next_duration = next_frame_count as f64 / next_fps;
                    let hollow = self
                        .project
                        .generative_config(*asset_id)
                        .is_none_or(|config| {
                            config.active_version.is_none() && config.versions.is_empty()
                        });
                    let clip_ids: Vec<_> = self
                        .project
                        .clips
                        .iter()
                        .filter(|clip| clip.asset_id == *asset_id)
                        .map(|clip| clip.id)
                        .collect();
                    if hollow && clip_ids.len() == 1 {
                        if let Some(clip_id) = clip_ids.first().copied() {
                            if let Some(clip) = self
                                .project
                                .clips
                                .iter_mut()
                                .find(|clip| clip.id == clip_id)
                            {
                                clip.duration = next_duration.max(0.1);
                            }
                        }
                    }
                    if let Err(err) = sync_generative_video_timing_inputs(
                        &mut self.project,
                        &self.provider_entries,
                        *asset_id,
                    ) {
                        self.status =
                            format!("Updated timing, but timing input sync failed: {err}");
                    }
                    self.preview_dirty = true;
                    AutomationResponse::ok(json!({
                        "asset_id": asset_id,
                        "fps": next_fps,
                        "frame_count": next_frame_count,
                        "duration_seconds": next_duration,
                    }))
                } else {
                    AutomationResponse::not_found("Generative video not found.")
                }
            }
            AutomationCommand::ExtractActiveGeneration {
                asset_id,
                asset_name,
            } => {
                let selected = self.resolve_asset_or_selected(*asset_id, asset_name.as_deref());
                match selected {
                    Some(asset_id) => match self.extract_active_generation_as_asset(asset_id) {
                        Ok(new_asset_id) => {
                            AutomationResponse::ok(json!({ "asset_id": new_asset_id }))
                        }
                        Err(err) => AutomationResponse::error(err),
                    },
                    None => AutomationResponse::error("No matching asset found."),
                }
            }
            AutomationCommand::ExtractGenerationVersion { asset_id, version } => {
                match self.extract_generation_version_as_asset(*asset_id, version.as_deref()) {
                    Ok(new_asset_id) => AutomationResponse::ok(json!({ "asset_id": new_asset_id })),
                    Err(err) => AutomationResponse::error(err),
                }
            }
            AutomationCommand::AddAssetToTimeline {
                asset_id,
                asset_name,
                track_id,
                time,
                duration_seconds,
            } => {
                let selected = self.resolve_asset(*asset_id, asset_name.as_deref());
                match selected {
                    Some(asset_id) => {
                        let result = if let Some(track_id) = track_id {
                            self.add_asset_to_timeline_track(asset_id, *track_id, *time)
                        } else {
                            self.add_asset_to_timeline(asset_id, *time)
                        };
                        match result {
                            Ok(clip_id) => {
                                if let Some(duration) = duration_seconds {
                                    if let Some(clip) = self
                                        .project
                                        .clips
                                        .iter()
                                        .find(|clip| clip.id == clip_id)
                                        .cloned()
                                    {
                                        self.project.resize_clip(
                                            clip_id,
                                            clip.start_time,
                                            (*duration).max(0.1),
                                        );
                                        self.preview_dirty = true;
                                    }
                                }
                                AutomationResponse::ok(json!({ "clip_id": clip_id }))
                            }
                            Err(err) => AutomationResponse::error(err),
                        }
                    }
                    None => AutomationResponse::error("No matching asset found."),
                }
            }
            AutomationCommand::CreateGenerativeAsset {
                output_type,
                name,
                fps,
                duration_seconds,
                frame_count,
            } => {
                let result = match output_type {
                    ProviderOutputType::Image => self.create_generative_image(),
                    ProviderOutputType::Video => self.create_generative_video(
                        fps.unwrap_or(DEFAULT_GENERATIVE_VIDEO_FPS),
                        frame_count.unwrap_or_else(|| {
                            let fps = fps.unwrap_or(DEFAULT_GENERATIVE_VIDEO_FPS).max(1.0);
                            duration_seconds
                                .map(|duration| (duration.max(1.0 / fps) * fps).round() as u32)
                                .unwrap_or(DEFAULT_GENERATIVE_VIDEO_FRAME_COUNT)
                                .max(1)
                        }),
                    ),
                    ProviderOutputType::Audio => self.create_generative_audio(),
                };
                match result {
                    Ok(asset_id) => {
                        if let Some(name) = name {
                            let _ = self.rename_asset(asset_id, name.clone());
                        }
                        self.selection.select_asset(asset_id);
                        AutomationResponse::ok(json!({ "asset_id": asset_id }))
                    }
                    Err(err) => AutomationResponse::error(err),
                }
            }
            AutomationCommand::Seek { time } => {
                self.seek(*time);
                AutomationResponse::ok(json!({ "current_time": self.current_time }))
            }
            AutomationCommand::SelectClip { clip_id, index } => {
                let selected = if let Some(id) = *clip_id {
                    if !self.project.clips.iter().any(|clip| clip.id == id) {
                        return AutomationResponse::not_found("Clip not found.");
                    }
                    Some(id)
                } else {
                    let index = (*index).unwrap_or(0);
                    self.project.clips.get(index).map(|clip| clip.id)
                };
                match selected {
                    Some(id) => {
                        self.selection.select_clip(id);
                        AutomationResponse::ok(json!({ "clip_id": id }))
                    }
                    None => AutomationResponse::error("No matching clip found."),
                }
            }
            AutomationCommand::SelectAsset { asset_id, index } => {
                let selected = if let Some(id) = *asset_id {
                    if !self.project.assets.iter().any(|asset| asset.id == id) {
                        return AutomationResponse::not_found("Asset not found.");
                    }
                    Some(id)
                } else {
                    let index = (*index).unwrap_or(0);
                    self.project.assets.get(index).map(|asset| asset.id)
                };
                match selected {
                    Some(id) => {
                        self.selection.clear();
                        self.selection.asset_ids.push(id);
                        AutomationResponse::ok(json!({ "asset_id": id }))
                    }
                    None => AutomationResponse::error("No matching asset found."),
                }
            }
            AutomationCommand::SelectTrack { track_id, index } => {
                let selected = if let Some(id) = *track_id {
                    if !self.project.tracks.iter().any(|track| track.id == id) {
                        return AutomationResponse::not_found("Track not found.");
                    }
                    Some(id)
                } else {
                    let index = (*index).unwrap_or(0);
                    self.project.tracks.get(index).map(|track| track.id)
                };
                match selected {
                    Some(id) => {
                        self.selection.select_track(id);
                        AutomationResponse::ok(json!({ "track_id": id }))
                    }
                    None => AutomationResponse::error("No matching track found."),
                }
            }
            AutomationCommand::SelectMarker { marker_id, index } => {
                let selected = if let Some(id) = *marker_id {
                    if !self.project.markers.iter().any(|marker| marker.id == id) {
                        return AutomationResponse::not_found("Marker not found.");
                    }
                    Some(id)
                } else {
                    let index = (*index).unwrap_or(0);
                    self.project.markers.get(index).map(|marker| marker.id)
                };
                match selected {
                    Some(id) => {
                        self.selection.select_marker(id);
                        AutomationResponse::ok(json!({ "marker_id": id }))
                    }
                    None => AutomationResponse::error("No matching marker found."),
                }
            }
            AutomationCommand::SetSelection {
                clips,
                assets,
                tracks,
                markers,
            } => {
                if let Some(id) = clips
                    .iter()
                    .find(|id| !self.project.clips.iter().any(|clip| clip.id == **id))
                {
                    return AutomationResponse::not_found(format!("Clip not found: {id}"));
                }
                if let Some(id) = assets
                    .iter()
                    .find(|id| !self.project.assets.iter().any(|asset| asset.id == **id))
                {
                    return AutomationResponse::not_found(format!("Asset not found: {id}"));
                }
                if let Some(id) = tracks
                    .iter()
                    .find(|id| !self.project.tracks.iter().any(|track| track.id == **id))
                {
                    return AutomationResponse::not_found(format!("Track not found: {id}"));
                }
                if let Some(id) = markers
                    .iter()
                    .find(|id| !self.project.markers.iter().any(|marker| marker.id == **id))
                {
                    return AutomationResponse::not_found(format!("Marker not found: {id}"));
                }
                self.selection.clip_ids = clips.clone();
                self.selection.asset_ids = assets.clone();
                self.selection.track_ids = tracks.clone();
                self.selection.marker_ids = markers.clone();
                self.preview_dirty = true;
                AutomationResponse::ok(json!({ "selection": {
                    "clips": self.selection.clip_ids,
                    "assets": self.selection.asset_ids,
                    "tracks": self.selection.track_ids,
                    "markers": self.selection.marker_ids,
                }}))
            }
            AutomationCommand::AddMarker {
                time,
                track_id,
                label,
            } => {
                if let Some(track_id) = track_id {
                    let Some(track) = self.project.find_track(*track_id) else {
                        return AutomationResponse::not_found("Track not found.");
                    };
                    if track.track_type != crate::state::TrackType::Marker {
                        return AutomationResponse::conflict("Track is not a marker track.");
                    }
                }
                let marker_id = self.add_marker_to_track(*time, *track_id);
                if let Some(label) = label {
                    let _ = self
                        .project
                        .set_marker_label(marker_id, Some(label.clone()));
                }
                AutomationResponse::ok(json!({ "marker_id": marker_id }))
            }
            AutomationCommand::SetMarker { marker_id, patch } => {
                if !self
                    .project
                    .markers
                    .iter()
                    .any(|marker| marker.id == *marker_id)
                {
                    return AutomationResponse::not_found("Marker not found.");
                }
                if let Some(track_id) = patch.track_id {
                    let Some(track) = self.project.find_track(track_id) else {
                        return AutomationResponse::not_found("Track not found.");
                    };
                    if track.track_type != crate::state::TrackType::Marker {
                        return AutomationResponse::conflict("Track is not a marker track.");
                    }
                    if let Some(marker) = self
                        .project
                        .markers
                        .iter_mut()
                        .find(|marker| marker.id == *marker_id)
                    {
                        marker.track_id = Some(track_id);
                    }
                }
                if let Some(time) = patch.time {
                    let _ = self.project.move_marker(*marker_id, time);
                }
                if patch.label.is_some() {
                    let _ = self
                        .project
                        .set_marker_label(*marker_id, patch.label.clone());
                }
                if patch.description.is_some() {
                    let _ = self
                        .project
                        .set_marker_description(*marker_id, patch.description.clone());
                }
                if patch.color.is_some() {
                    let _ = self
                        .project
                        .set_marker_color(*marker_id, patch.color.clone());
                }
                self.selection.select_marker(*marker_id);
                self.preview_dirty = true;
                AutomationResponse::ok(json!({ "marker_id": marker_id }))
            }
            AutomationCommand::DeleteMarker { marker_id } => {
                if self.project.remove_marker(*marker_id) {
                    self.selection.marker_ids.retain(|id| id != marker_id);
                    self.preview_dirty = true;
                    AutomationResponse::ok(json!({ "marker_id": marker_id }))
                } else {
                    AutomationResponse::not_found("Marker not found.")
                }
            }
            AutomationCommand::AddTrack {
                track_type,
                index,
                name,
            } => {
                let track_id = if let Some(index) = index {
                    self.project.insert_track(*track_type, *index)
                } else {
                    self.project.add_track(*track_type)
                };
                if let Some(name) = name {
                    if let Some(track) = self
                        .project
                        .tracks
                        .iter_mut()
                        .find(|track| track.id == track_id)
                    {
                        track.name = name.clone();
                    }
                }
                self.selection.select_track(track_id);
                self.preview_dirty = true;
                AutomationResponse::ok(json!({ "track_id": track_id }))
            }
            AutomationCommand::SetTrack { track_id, patch } => {
                let Some(track) = self
                    .project
                    .tracks
                    .iter_mut()
                    .find(|track| track.id == *track_id)
                else {
                    return AutomationResponse::not_found("Track not found.");
                };
                if let Some(name) = patch.name.as_ref().filter(|name| !name.trim().is_empty()) {
                    track.name = name.trim().to_string();
                }
                if let Some(muted) = patch.muted {
                    track.muted = muted;
                }
                if let Some(volume) = patch.volume {
                    track.volume = volume.clamp(0.0, 4.0);
                }
                self.selection.select_track(*track_id);
                self.preview_dirty = true;
                AutomationResponse::ok(json!({ "track_id": track_id }))
            }
            AutomationCommand::MoveTrack { track_id, index } => {
                if self.project.move_track_to_index(*track_id, *index) {
                    self.selection.select_track(*track_id);
                    self.preview_dirty = true;
                    AutomationResponse::ok(json!({ "track_id": track_id, "index": index }))
                } else {
                    AutomationResponse::not_found("Track not found or already at that index.")
                }
            }
            AutomationCommand::DeleteTrack { track_id, dry_run } => {
                if self.project.find_track(*track_id).is_none() {
                    return AutomationResponse::not_found("Track not found.");
                }
                let (clip_count, marker_count) = self.project.track_delete_counts(*track_id);
                if *dry_run {
                    return AutomationResponse::ok(json!({
                        "track_id": track_id,
                        "clip_count": clip_count,
                        "marker_count": marker_count,
                    }));
                }
                if self.project.remove_track(*track_id) {
                    self.selection.track_ids.retain(|id| id != track_id);
                    self.preview_dirty = true;
                    AutomationResponse::ok(json!({
                        "track_id": track_id,
                        "removed_clips": clip_count,
                        "removed_markers": marker_count,
                    }))
                } else {
                    AutomationResponse::not_found("Track not found.")
                }
            }
            AutomationCommand::SetClip { clip_id, patch } => {
                let clip_snapshot = match self
                    .project
                    .clips
                    .iter()
                    .find(|clip| clip.id == *clip_id)
                    .cloned()
                {
                    Some(clip) => clip,
                    None => return AutomationResponse::not_found("Clip not found."),
                };
                if let Some(track_id) = patch.track_id {
                    if self.project.find_track(track_id).is_none() {
                        return AutomationResponse::not_found("Track not found.");
                    }
                    if clip_snapshot.track_id != track_id
                        && !self
                            .project
                            .asset_compatible_with_track(clip_snapshot.asset_id, track_id)
                    {
                        return AutomationResponse::conflict(
                            "Clip asset is not compatible with that track.",
                        );
                    }
                    if clip_snapshot.track_id != track_id
                        && !self.project.move_clip_to_track(*clip_id, track_id)
                    {
                        return AutomationResponse::conflict(
                            "Clip could not be moved to that track.",
                        );
                    }
                }
                if let Some(mode) = patch.time_mode {
                    let _ = self.project.set_clip_time_mode(*clip_id, mode);
                }
                if patch.start_time.is_some() || patch.duration.is_some() {
                    let start = patch.start_time.unwrap_or(clip_snapshot.start_time);
                    let duration = patch.duration.unwrap_or(clip_snapshot.duration);
                    let _ = self.project.resize_clip(*clip_id, start, duration);
                }
                if let Some(trim) = patch.trim_in_seconds {
                    let _ = self.project.set_clip_trim_in_seconds(*clip_id, trim);
                }
                if let Some(volume) = patch.volume {
                    if let Some(clip) = self
                        .project
                        .clips
                        .iter_mut()
                        .find(|clip| clip.id == *clip_id)
                    {
                        clip.volume = volume.clamp(0.0, 4.0);
                    }
                }
                if patch.label.is_some() {
                    let _ = self.project.set_clip_label(*clip_id, patch.label.clone());
                }
                if let Some(mode) = patch.image_mode {
                    let _ = self.project.set_clip_image_mode(*clip_id, mode);
                }
                if let Some(transform) = patch.transform {
                    let _ = self.project.set_clip_transform(*clip_id, transform);
                }
                if let Some(bridge) = patch.bridge.clone() {
                    let _ = self.project.set_clip_bridge(*clip_id, bridge);
                }
                self.sync_timeline_bridge_clips();
                self.selection.select_clip(*clip_id);
                self.preview_dirty = true;
                AutomationResponse::ok(json!({ "clip_id": clip_id }))
            }
            AutomationCommand::MoveClip {
                clip_id,
                start_time,
                track_id,
            } => {
                let clip_snapshot = match self
                    .project
                    .clips
                    .iter()
                    .find(|clip| clip.id == *clip_id)
                    .cloned()
                {
                    Some(clip) => clip,
                    None => return AutomationResponse::not_found("Clip not found."),
                };
                if let Some(track_id) = *track_id {
                    if self.project.find_track(track_id).is_none() {
                        return AutomationResponse::not_found("Track not found.");
                    }
                    if clip_snapshot.track_id != track_id
                        && !self
                            .project
                            .asset_compatible_with_track(clip_snapshot.asset_id, track_id)
                    {
                        return AutomationResponse::conflict(
                            "Clip asset is not compatible with that track.",
                        );
                    }
                    if clip_snapshot.track_id != track_id
                        && !self.project.move_clip_to_track(*clip_id, track_id)
                    {
                        return AutomationResponse::conflict(
                            "Clip could not be moved to that track.",
                        );
                    }
                }
                if !self.project.move_clip(*clip_id, *start_time) {
                    return AutomationResponse::not_found("Clip not found.");
                }
                self.sync_timeline_bridge_clips();
                self.selection.select_clip(*clip_id);
                self.preview_dirty = true;
                AutomationResponse::ok(json!({ "clip_id": clip_id }))
            }
            AutomationCommand::MoveClips {
                mode,
                moves,
                clip_ids,
                delta_seconds,
                track_delta,
                track_id,
            } => {
                let response = self.apply_move_clips_command(
                    *mode,
                    moves,
                    clip_ids,
                    *delta_seconds,
                    *track_delta,
                    *track_id,
                );
                self.sync_timeline_bridge_clips();
                response
            }
            AutomationCommand::ResizeClip {
                clip_id,
                start_time,
                duration,
            } => {
                if self.project.resize_clip(*clip_id, *start_time, *duration) {
                    self.sync_timeline_bridge_clips();
                    self.selection.select_clip(*clip_id);
                    self.preview_dirty = true;
                    AutomationResponse::ok(json!({ "clip_id": clip_id }))
                } else {
                    AutomationResponse::not_found("Clip not found.")
                }
            }
            AutomationCommand::DeleteClips { clip_ids } => {
                let missing: Vec<_> = clip_ids
                    .iter()
                    .filter(|clip_id| !self.project.clips.iter().any(|clip| clip.id == **clip_id))
                    .copied()
                    .collect();
                if !missing.is_empty() {
                    return AutomationResponse::not_found(format!(
                        "Clips not found: {}",
                        missing
                            .iter()
                            .map(Uuid::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                let mut removed = 0usize;
                for clip_id in clip_ids {
                    if self.project.remove_clip(*clip_id) {
                        removed += 1;
                    }
                }
                if removed > 0 {
                    self.selection
                        .clip_ids
                        .retain(|id| !clip_ids.iter().any(|clip_id| clip_id == id));
                    self.sync_timeline_bridge_clips();
                    self.preview_dirty = true;
                }
                AutomationResponse::ok(json!({ "removed_clips": removed }))
            }
            AutomationCommand::ListProviders { include_all } => AutomationResponse::ok(json!({
                "providers": self.redacted_provider_entries_for_scope(*include_all),
                "scope": self.project.settings.provider_scope.clone(),
                "include_all": include_all,
            })),
            AutomationCommand::RefreshProviders { include_all } => {
                self.refresh_providers();
                AutomationResponse::ok(json!({
                    "providers": self.redacted_provider_entries_for_scope(*include_all),
                    "scope": self.project.settings.provider_scope.clone(),
                    "include_all": include_all,
                }))
            }
            AutomationCommand::CreateProviderFromTemplate { template } => {
                let provider = match template {
                    crate::core::automation::ProviderTemplate::ComfyUi => default_provider_entry(),
                    crate::core::automation::ProviderTemplate::OpenAiImage => {
                        default_openai_image_provider_entry()
                    }
                    crate::core::automation::ProviderTemplate::XaiImage => {
                        default_xai_image_provider_entry()
                    }
                    crate::core::automation::ProviderTemplate::XaiVideo => {
                        default_xai_video_provider_entry()
                    }
                };
                match self.save_provider_entry(provider) {
                    Ok(provider) => AutomationResponse::ok(
                        json!({ "provider": redacted_provider_entry_json(&provider) }),
                    ),
                    Err(err) => AutomationResponse::error(err),
                }
            }
            AutomationCommand::CreateProvider { provider } => {
                match self.save_provider_entry(provider.clone()) {
                    Ok(provider) => AutomationResponse::ok(
                        json!({ "provider": redacted_provider_entry_json(&provider) }),
                    ),
                    Err(err) => AutomationResponse::error(err),
                }
            }
            AutomationCommand::UpdateProvider {
                provider_id,
                provider,
            } => {
                if !self
                    .provider_entries
                    .iter()
                    .any(|entry| entry.id == *provider_id)
                {
                    return AutomationResponse::not_found("Provider not found.");
                }
                let mut next = provider.clone();
                next.id = *provider_id;
                match self.save_provider_entry(next) {
                    Ok(provider) => AutomationResponse::ok(
                        json!({ "provider": redacted_provider_entry_json(&provider) }),
                    ),
                    Err(err) => AutomationResponse::error(err),
                }
            }
            AutomationCommand::DeleteProvider { provider_id } => {
                match self.delete_provider_entry(*provider_id) {
                    Ok(provider) => AutomationResponse::ok(
                        json!({ "provider": redacted_provider_entry_json(&provider) }),
                    ),
                    Err(err) => AutomationResponse::error(err),
                }
            }
            AutomationCommand::GetGenerativeConfig { asset_id } => {
                match self.project.generative_config(*asset_id) {
                    Some(config) => AutomationResponse::ok(json!({ "config": config })),
                    None => AutomationResponse::not_found("Generative config not found."),
                }
            }
            AutomationCommand::SetGenerativeConfig { asset_id, patch } => {
                let Some(asset) = self.project.find_asset(*asset_id).cloned() else {
                    return AutomationResponse::not_found("Asset not found.");
                };
                let Some(config_snapshot) = self.project.generative_config(*asset_id).cloned()
                else {
                    return AutomationResponse::not_found("Generative asset not found.");
                };
                let target_provider_id = patch.provider_id.or(config_snapshot.provider_id);
                if let Some(provider_id) = patch.provider_id {
                    match self
                        .provider_entries
                        .iter()
                        .find(|provider| provider.id == provider_id)
                    {
                        Some(provider)
                            if provider_matches_asset_output(&asset, provider)
                                && self.provider_in_project_scope(provider.id) => {}
                        Some(provider) if provider_matches_asset_output(&asset, provider) => {
                            return AutomationResponse::conflict(
                                "Provider is outside this project's provider scope.",
                            );
                        }
                        Some(_) => {
                            return AutomationResponse::conflict(
                                "Provider output type does not match this asset.",
                            );
                        }
                        None => return AutomationResponse::not_found("Provider not found."),
                    }
                }
                if let Some(inputs) = patch.inputs.as_ref() {
                    if let Err(err) = validate_generative_input_refs(&self.project, inputs) {
                        return AutomationResponse::not_found(err);
                    }
                }
                if let Some(reference_slots) = patch.reference_slots.as_ref() {
                    if let Err(err) = validate_generative_input_refs(&self.project, reference_slots)
                    {
                        return AutomationResponse::not_found(err);
                    }
                }
                if let Some(active_version) = patch.active_version.as_ref() {
                    if !config_snapshot
                        .versions
                        .iter()
                        .any(|record| record.version == *active_version)
                    {
                        return AutomationResponse::not_found("Generation version not found.");
                    }
                }
                let updated = self.project.update_generative_config(*asset_id, |config| {
                    if let Some(provider_id) = patch.provider_id {
                        config.provider_id = Some(provider_id);
                    }
                    if let Some(inputs) = patch.inputs.clone() {
                        config.inputs.extend(inputs);
                    }
                    if let Some(reference_slots) = patch.reference_slots.clone() {
                        config.reference_slots.extend(reference_slots);
                    }
                    if let Some(batch) = patch.batch.clone() {
                        config.batch = batch;
                    }
                    if let Some(active_version) = patch.active_version.clone() {
                        let _ = apply_active_generation_version_to_config(config, &active_version);
                    }
                    normalize_media_reference_slots_to_inputs(
                        config,
                        target_provider_id.and_then(|provider_id| {
                            self.provider_entries
                                .iter()
                                .find(|provider| provider.id == provider_id)
                        }),
                    );
                });
                if !updated {
                    return AutomationResponse::not_found("Generative asset not found.");
                }
                if let Err(err) = self.project.save_generative_config(*asset_id) {
                    return AutomationResponse::error(format!(
                        "Failed to save generative config: {err}"
                    ));
                }
                self.preview_dirty = true;
                AutomationResponse::ok(json!({
                    "asset_id": asset_id,
                    "config": self.project.generative_config(*asset_id),
                }))
            }
            AutomationCommand::ReplaceGenerativeConfig { asset_id, config } => {
                let Some(asset) = self.project.find_asset(*asset_id).cloned() else {
                    return AutomationResponse::not_found("Asset not found.");
                };
                if let Some(provider_id) = config.provider_id {
                    match self
                        .provider_entries
                        .iter()
                        .find(|provider| provider.id == provider_id)
                    {
                        Some(provider)
                            if provider_matches_asset_output(&asset, provider)
                                && self.provider_in_project_scope(provider.id) => {}
                        Some(provider) if provider_matches_asset_output(&asset, provider) => {
                            return AutomationResponse::conflict(
                                "Provider is outside this project's provider scope.",
                            );
                        }
                        Some(_) => {
                            return AutomationResponse::conflict(
                                "Provider output type does not match this asset.",
                            );
                        }
                        None => return AutomationResponse::not_found("Provider not found."),
                    }
                }
                if let Err(err) = validate_generative_input_refs(&self.project, &config.inputs) {
                    return AutomationResponse::not_found(err);
                }
                if let Err(err) =
                    validate_generative_input_refs(&self.project, &config.reference_slots)
                {
                    return AutomationResponse::not_found(err);
                }
                if let Some(active_version) = config.active_version.as_ref() {
                    if !config
                        .versions
                        .iter()
                        .any(|record| record.version == *active_version)
                    {
                        return AutomationResponse::not_found("Generation version not found.");
                    }
                }
                let mut next = config.clone();
                if let Some(active_version) = next.active_version.clone() {
                    let _ = apply_active_generation_version_to_config(&mut next, &active_version);
                }
                let next_provider = next.provider_id.and_then(|provider_id| {
                    self.provider_entries
                        .iter()
                        .find(|provider| provider.id == provider_id)
                });
                normalize_media_reference_slots_to_inputs(&mut next, next_provider);
                let updated = self.project.update_generative_config(*asset_id, |config| {
                    *config = next;
                });
                if !updated {
                    return AutomationResponse::not_found("Generative asset not found.");
                }
                if let Err(err) = self.project.save_generative_config(*asset_id) {
                    return AutomationResponse::error(format!(
                        "Failed to save generative config: {err}"
                    ));
                }
                self.preview_dirty = true;
                AutomationResponse::ok(json!({
                    "asset_id": asset_id,
                    "config": self.project.generative_config(*asset_id),
                }))
            }
            AutomationCommand::ListJobs => AutomationResponse::ok(
                json!({ "jobs": redacted_generation_jobs_json(&self.generation_queue) }),
            ),
            AutomationCommand::GetJob { job_id } => {
                match self.generation_queue.iter().find(|job| job.id == *job_id) {
                    Some(job) => {
                        AutomationResponse::ok(json!({ "job": redacted_generation_job_json(job) }))
                    }
                    None => AutomationResponse::not_found("Job not found."),
                }
            }
            AutomationCommand::SaveProject => match self.save() {
                Ok(()) => AutomationResponse::empty_ok(),
                Err(err) => AutomationResponse::error(err),
            },
            AutomationCommand::OpenProviders => {
                self.refresh_providers();
                self.overlays.providers = true;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::CloseProviders => {
                self.overlays.providers = false;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::OpenProjectSettings => {
                self.overlays.project_settings = true;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::CloseProjectSettings => {
                self.overlays.project_settings = false;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::OpenNewProject => {
                self.overlays.new_project = true;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::CloseNewProject => {
                self.overlays.new_project = false;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::OpenQueue => {
                self.overlays.queue = true;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::CloseQueue => {
                self.overlays.queue = false;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::OpenGenerativeVideo => {
                self.overlays.generative_video = true;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::CloseGenerativeVideo => {
                self.overlays.generative_video = false;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::OpenExportVideo => {
                self.overlays.export_video = true;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::CloseExportVideo => {
                self.overlays.export_video = false;
                AutomationResponse::empty_ok()
            }
            AutomationCommand::SetLayout {
                left_collapsed,
                right_collapsed,
                timeline_collapsed,
                preview_stats,
                hardware_decode,
                left_width,
                right_width,
                timeline_height,
                timeline_zoom,
                timeline_scroll_x,
                timeline_scroll_y,
            } => {
                if let Some(value) = *left_collapsed {
                    self.layout.left_collapsed = value;
                }
                if let Some(value) = *right_collapsed {
                    self.layout.right_collapsed = value;
                }
                if let Some(value) = *timeline_collapsed {
                    self.layout.timeline_collapsed = value;
                }
                if let Some(value) = *preview_stats {
                    self.layout.preview_stats = value;
                }
                if let Some(value) = *hardware_decode {
                    self.layout.hardware_decode = value;
                }
                if let Some(value) = *left_width {
                    self.layout.left_width = value.max(150.0);
                }
                if let Some(value) = *right_width {
                    self.layout.right_width = value.max(150.0);
                }
                if let Some(value) = *timeline_height {
                    self.layout.timeline_height = value.clamp(150.0, 420.0);
                }
                if let Some(value) = *timeline_zoom {
                    self.layout.timeline_zoom = value.clamp(0.01, 20_000.0);
                }
                if let Some(value) = *timeline_scroll_x {
                    self.layout.timeline_scroll_x = value.max(0.0);
                }
                if let Some(value) = *timeline_scroll_y {
                    self.layout.timeline_scroll_y = value.max(0.0);
                }
                AutomationResponse::empty_ok()
            }
            AutomationCommand::CloseAllOverlays => {
                self.overlays = EditorOverlays::default();
                AutomationResponse::empty_ok()
            }
        }
    }

    fn apply_move_clips_command(
        &mut self,
        mode: ClipMoveMode,
        moves: &[ClipMoveTarget],
        clip_ids: &[Uuid],
        delta_seconds: Option<f64>,
        track_delta: Option<i32>,
        track_id: Option<Uuid>,
    ) -> AutomationResponse {
        let targets = match mode {
            ClipMoveMode::Absolute => self.resolve_absolute_clip_moves(moves),
            ClipMoveMode::Relative => {
                self.resolve_relative_clip_moves(clip_ids, delta_seconds, track_delta, track_id)
            }
        };
        let targets = match targets {
            Ok(targets) => targets,
            Err(response) => return response,
        };

        for target in &targets {
            if let Some(clip) = self
                .project
                .clips
                .iter()
                .find(|clip| clip.id == target.clip_id)
            {
                if clip.track_id != target.track_id {
                    let _ = self
                        .project
                        .move_clip_to_track(target.clip_id, target.track_id);
                }
            }
            let _ = self.project.move_clip(target.clip_id, target.start_time);
        }

        let moved_clips: Vec<_> = targets
            .iter()
            .map(|target| {
                json!({
                    "clip_id": target.clip_id,
                    "start_time": target.start_time,
                    "track_id": target.track_id,
                })
            })
            .collect();
        self.selection.clip_ids = targets.iter().map(|target| target.clip_id).collect();
        self.selection.asset_ids.clear();
        self.selection.track_ids.clear();
        self.selection.marker_ids.clear();
        self.preview_dirty = true;
        AutomationResponse::ok(json!({ "clips": moved_clips }))
    }

    fn resolve_absolute_clip_moves(
        &self,
        moves: &[ClipMoveTarget],
    ) -> Result<Vec<ResolvedClipMove>, AutomationResponse> {
        if moves.is_empty() {
            return Err(AutomationResponse::error(
                "Absolute move_clips requires at least one move target.",
            ));
        }
        if let Some(duplicate) = first_duplicate_uuid(moves.iter().map(|target| target.clip_id)) {
            return Err(AutomationResponse::conflict(format!(
                "Duplicate clip_id in move targets: {duplicate}"
            )));
        }

        let mut targets = Vec::with_capacity(moves.len());
        for target in moves {
            if target.start_time.is_none() && target.track_id.is_none() {
                return Err(AutomationResponse::error(
                    "Each absolute move target needs start_time and/or track_id.",
                ));
            }
            let Some(clip) = self
                .project
                .clips
                .iter()
                .find(|clip| clip.id == target.clip_id)
            else {
                return Err(AutomationResponse::not_found(format!(
                    "Clip not found: {}",
                    target.clip_id
                )));
            };
            let target_track_id = target.track_id.unwrap_or(clip.track_id);
            let target_start_time = target.start_time.unwrap_or(clip.start_time).max(0.0);
            self.validate_clip_move_target(clip.asset_id, target_track_id)?;
            targets.push(ResolvedClipMove {
                clip_id: clip.id,
                start_time: target_start_time,
                track_id: target_track_id,
            });
        }
        Ok(targets)
    }

    fn resolve_relative_clip_moves(
        &self,
        clip_ids: &[Uuid],
        delta_seconds: Option<f64>,
        track_delta: Option<i32>,
        track_id: Option<Uuid>,
    ) -> Result<Vec<ResolvedClipMove>, AutomationResponse> {
        if clip_ids.is_empty() {
            return Err(AutomationResponse::error(
                "Relative move_clips requires at least one clip_id.",
            ));
        }
        if let Some(duplicate) = first_duplicate_uuid(clip_ids.iter().copied()) {
            return Err(AutomationResponse::conflict(format!(
                "Duplicate clip_id in clip_ids: {duplicate}"
            )));
        }
        if track_delta.is_some() && track_id.is_some() {
            return Err(AutomationResponse::conflict(
                "Use either track_delta or track_id for relative move_clips, not both.",
            ));
        }
        let time_delta = delta_seconds.unwrap_or(0.0);
        let vertical_move_requested = track_delta.unwrap_or(0) != 0 || track_id.is_some();
        if time_delta == 0.0 && !vertical_move_requested {
            return Err(AutomationResponse::error(
                "Relative move_clips requires delta_seconds, track_delta, or track_id.",
            ));
        }

        let mut targets = Vec::with_capacity(clip_ids.len());
        for clip_id in clip_ids {
            let Some(clip) = self.project.clips.iter().find(|clip| clip.id == *clip_id) else {
                return Err(AutomationResponse::not_found(format!(
                    "Clip not found: {clip_id}"
                )));
            };
            let target_track_id = if let Some(track_id) = track_id {
                track_id
            } else if let Some(delta) = track_delta {
                self.relative_track_id(clip.track_id, delta)?
            } else {
                clip.track_id
            };
            self.validate_clip_move_target(clip.asset_id, target_track_id)?;
            targets.push(ResolvedClipMove {
                clip_id: clip.id,
                start_time: (clip.start_time + time_delta).max(0.0),
                track_id: target_track_id,
            });
        }
        Ok(targets)
    }

    fn relative_track_id(
        &self,
        current_track_id: Uuid,
        delta: i32,
    ) -> Result<Uuid, AutomationResponse> {
        if delta == 0 {
            return Ok(current_track_id);
        }
        let Some(index) = self
            .project
            .tracks
            .iter()
            .position(|track| track.id == current_track_id)
        else {
            return Err(AutomationResponse::not_found(format!(
                "Current track not found: {current_track_id}"
            )));
        };
        let target_index = index as i64 + delta as i64;
        if target_index < 0 || target_index >= self.project.tracks.len() as i64 {
            return Err(AutomationResponse::conflict(format!(
                "track_delta {delta} moves clip outside the timeline track range."
            )));
        }
        Ok(self.project.tracks[target_index as usize].id)
    }

    fn validate_clip_move_target(
        &self,
        asset_id: Uuid,
        track_id: Uuid,
    ) -> Result<(), AutomationResponse> {
        if self.project.find_track(track_id).is_none() {
            return Err(AutomationResponse::not_found(format!(
                "Track not found: {track_id}"
            )));
        }
        if !self.project.asset_compatible_with_track(asset_id, track_id) {
            return Err(AutomationResponse::conflict(format!(
                "Clip asset is not compatible with target track: {track_id}"
            )));
        }
        Ok(())
    }

    fn resolve_asset(&self, asset_id: Option<Uuid>, asset_name: Option<&str>) -> Option<Uuid> {
        asset_id
            .or_else(|| {
                asset_name.and_then(|name| {
                    self.project
                        .assets
                        .iter()
                        .find(|asset| asset.name == name)
                        .map(|asset| asset.id)
                })
            })
            .or_else(|| self.project.assets.first().map(|asset| asset.id))
    }

    fn resolve_asset_or_selected(
        &self,
        asset_id: Option<Uuid>,
        asset_name: Option<&str>,
    ) -> Option<Uuid> {
        asset_id
            .or_else(|| {
                asset_name.and_then(|name| {
                    self.project
                        .assets
                        .iter()
                        .find(|asset| asset.name == name)
                        .map(|asset| asset.id)
                })
            })
            .or_else(|| self.selection.asset_ids.first().copied())
    }

    fn duplicate_asset_inner(&mut self, asset_id: Uuid) -> Result<Uuid, String> {
        let project_root = self.project.project_path.clone();
        let source = self
            .project
            .find_asset(asset_id)
            .cloned()
            .ok_or_else(|| "Asset not found.".to_string())?;
        let mut asset = source.clone();
        asset.id = Uuid::new_v4();
        asset.name = unique_asset_copy_name(&self.project.assets, &source.name);

        if source.is_generative() {
            let project_root =
                project_root.ok_or_else(|| "Create or open a project first.".to_string())?;
            let source_folder = generative_folder(&source)
                .cloned()
                .ok_or_else(|| "Generative asset has no output folder.".to_string())?;
            let media_type = generative_media_type(&source)
                .ok_or_else(|| "Generative asset has no media type.".to_string())?;
            let target_folder = generative_asset_folder(media_type, asset.id);
            copy_dir_recursive(
                &project_root.join(&source_folder),
                &project_root.join(&target_folder),
            )?;
            set_generative_folder(&mut asset, target_folder.clone());
            let config = self
                .project
                .generative_configs
                .get(&source.id)
                .cloned()
                .unwrap_or_default();
            let new_id = self.project.add_asset(asset);
            self.project.generative_configs.insert(new_id, config);
            self.project
                .save_generative_config(new_id)
                .map_err(|err| format!("Failed to save duplicated generative config: {err}"))?;
            Ok(new_id)
        } else {
            Ok(self.project.add_asset(asset))
        }
    }

    pub fn state_json(&self, include: &[String]) -> serde_json::Value {
        let mut state = json!({
            "project": {
                "name": self.project.name.clone(),
                "path": self.project.project_path.clone(),
                "settings": self.project.settings.clone(),
                "duration_seconds": self.project.duration(),
                "tracks": self.project.tracks.clone(),
                "assets": self.project.assets.clone(),
                "clips": self.project.clips.clone(),
                "markers": self.project.markers.clone(),
                "generative_configs": self.project.generative_configs.clone(),
                "workspace_layout": self.layout.workspace_layout(),
            },
            "providers": self.redacted_provider_entries_for_scope(false),
            "queue": redacted_generation_jobs_json(&self.generation_queue),
            "current_time": self.current_time,
            "timeline": {
                "current_time": self.current_time,
                "is_playing": self.is_playing,
            },
            "startup_done": self.startup_done,
            "overlays": {
                "providers_open": self.overlays.providers,
                "project_settings_open": self.overlays.project_settings,
                "new_project_open": self.overlays.new_project,
                "queue_open": self.overlays.queue,
                "agent_api_open": self.overlays.agent_api,
                "generative_video_open": self.overlays.generative_video,
                "export_video_open": self.overlays.export_video,
                "asset_lab_open": self.overlays.asset_lab,
            },
            "layout": {
                "left_collapsed": self.layout.left_collapsed,
                "right_collapsed": self.layout.right_collapsed,
                "timeline_collapsed": self.layout.timeline_collapsed,
                "left_width": self.layout.left_width,
                "right_width": self.layout.right_width,
                "timeline_height": self.layout.timeline_height,
                "timeline_zoom": self.layout.timeline_zoom,
                "timeline_scroll_x": self.layout.timeline_scroll_x,
                "timeline_scroll_y": self.layout.timeline_scroll_y,
                "preview_stats": self.layout.preview_stats,
                "hardware_decode": self.layout.hardware_decode,
            },
            "selection": {
                "clips": self.selection.clip_ids.clone(),
                "assets": self.selection.asset_ids.clone(),
                "tracks": self.selection.track_ids.clone(),
                "markers": self.selection.marker_ids.clone(),
            }
        });

        if include_requested(include, "diagnostics") {
            let queued_jobs = self
                .generation_queue
                .iter()
                .filter(|job| matches!(job.status, crate::state::GenerationJobStatus::Queued))
                .count();
            let running_jobs = self
                .generation_queue
                .iter()
                .filter(|job| matches!(job.status, crate::state::GenerationJobStatus::Running))
                .count();
            let terminal_jobs = self
                .generation_queue
                .iter()
                .filter(|job| {
                    matches!(
                        job.status,
                        crate::state::GenerationJobStatus::Succeeded
                            | crate::state::GenerationJobStatus::Failed
                            | crate::state::GenerationJobStatus::Canceled
                    )
                })
                .count();
            if let Some(object) = state.as_object_mut() {
                object.insert(
                    "diagnostics".to_string(),
                    json!({
                        "project_dirty": self.project_dirty,
                        "preview_dirty": self.preview_dirty,
                        "project_loaded": self.project.project_path.is_some(),
                        "counts": {
                            "providers": self.provider_entries.len(),
                            "scoped_providers": self
                                .provider_entries
                                .iter()
                                .filter(|provider| self.provider_in_project_scope(provider.id))
                                .count(),
                            "provider_files": self.provider_files.len(),
                            "assets": self.project.assets.len(),
                            "tracks": self.project.tracks.len(),
                            "clips": self.project.clips.len(),
                            "markers": self.project.markers.len(),
                            "generative_configs": self.project.generative_configs.len(),
                            "queue": self.generation_queue.len(),
                            "queued_jobs": queued_jobs,
                            "running_jobs": running_jobs,
                            "terminal_jobs": terminal_jobs,
                        },
                        "automation": {
                            "server_started": crate::core::automation::is_enabled(),
                            "enabled": crate::core::automation::is_active(),
                            "port": crate::core::automation::current_port(),
                            "bind": "127.0.0.1",
                        },
                        "preview_cache": self.previewer.cache_stats(),
                    }),
                );
            }
        }

        if include_requested(include, "all_providers") {
            if let Some(object) = state.as_object_mut() {
                object.insert(
                    "all_providers".to_string(),
                    json!(redacted_provider_entries_json(&self.provider_entries)),
                );
            }
        }

        state
    }
}

fn normalize_project_provider_scope(scope: ProjectProviderScope) -> ProjectProviderScope {
    match scope {
        ProjectProviderScope::All => ProjectProviderScope::All,
        ProjectProviderScope::Selected { provider_ids } => {
            let mut deduped = Vec::new();
            for provider_id in provider_ids {
                if !deduped.contains(&provider_id) {
                    deduped.push(provider_id);
                }
            }
            ProjectProviderScope::Selected {
                provider_ids: deduped,
            }
        }
    }
}

#[derive(Clone, Debug)]
struct ResolvedClipMove {
    clip_id: Uuid,
    start_time: f64,
    track_id: Uuid,
}

fn first_duplicate_uuid(ids: impl IntoIterator<Item = Uuid>) -> Option<Uuid> {
    let mut seen = Vec::new();
    for id in ids {
        if seen.contains(&id) {
            return Some(id);
        }
        seen.push(id);
    }
    None
}

pub(crate) fn redacted_generation_jobs_json(jobs: &[GenerationJob]) -> Vec<Value> {
    jobs.iter().map(redacted_generation_job_json).collect()
}

pub(crate) fn redacted_generation_job_json(job: &GenerationJob) -> Value {
    let mut value = serde_json::to_value(job).unwrap_or_else(|_| json!({}));
    if let Some(provider) = value
        .as_object_mut()
        .and_then(|object| object.get_mut("provider"))
    {
        *provider = redacted_provider_entry_json(&job.provider);
    }
    value
}

fn redacted_provider_entries_json(providers: &[ProviderEntry]) -> Vec<Value> {
    providers.iter().map(redacted_provider_entry_json).collect()
}

fn redacted_provider_entry_json(provider: &ProviderEntry) -> Value {
    let mut value = serde_json::to_value(provider).unwrap_or_else(|_| json!({}));
    let api_key_present = match &provider.connection {
        ProviderConnection::OpenAiImage { api_key, .. }
        | ProviderConnection::XaiImage { api_key, .. }
        | ProviderConnection::XaiVideo { api_key, .. }
        | ProviderConnection::CustomHttp { api_key, .. } => Some(api_key.is_some()),
        ProviderConnection::ComfyUi { .. } => None,
    };
    if let Some(api_key_present) = api_key_present {
        if let Some(connection) = value
            .as_object_mut()
            .and_then(|object| object.get_mut("connection"))
            .and_then(|connection| connection.as_object_mut())
        {
            connection.remove("api_key");
            connection.insert("api_key_present".to_string(), json!(api_key_present));
        }
    }
    value
}

fn provider_matches_asset_output(asset: &Asset, provider: &ProviderEntry) -> bool {
    asset_provider_output_type(asset) == Some(provider.output_type)
}

fn asset_provider_output_type(asset: &Asset) -> Option<ProviderOutputType> {
    match asset.kind {
        AssetKind::Image { .. } | AssetKind::GenerativeImage { .. } => {
            Some(ProviderOutputType::Image)
        }
        AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. } => {
            Some(ProviderOutputType::Video)
        }
        AssetKind::Audio { .. } | AssetKind::GenerativeAudio { .. } => {
            Some(ProviderOutputType::Audio)
        }
    }
}

fn sync_generative_video_timing_inputs(
    project: &mut Project,
    providers: &[ProviderEntry],
    asset_id: Uuid,
) -> Result<bool, String> {
    let Some(asset) = project.find_asset(asset_id) else {
        return Ok(false);
    };
    let AssetKind::GenerativeVideo {
        fps, frame_count, ..
    } = &asset.kind
    else {
        return Ok(false);
    };
    let fps = (*fps).max(1.0);
    let frame_count = (*frame_count).max(1);
    let duration = asset
        .duration_seconds
        .filter(|duration| *duration > 0.0)
        .unwrap_or(frame_count as f64 / fps);
    let Some(provider_id) = project
        .generative_config(asset_id)
        .and_then(|config| config.provider_id)
    else {
        return Ok(false);
    };
    let Some(provider) = providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .cloned()
    else {
        return Ok(false);
    };

    let mut changed = false;
    project.update_generative_config(asset_id, |config| {
        for input in provider.inputs.iter() {
            let Some(value) = provider_timing_role_value(input, duration, fps, frame_count) else {
                continue;
            };
            let next = InputValue::Literal { value };
            if config.inputs.get(&input.name) != Some(&next) {
                config.inputs.insert(input.name.clone(), next);
                changed = true;
            }
        }
    });
    if changed {
        project
            .save_generative_config(asset_id)
            .map_err(|err| err.to_string())?;
    }
    Ok(changed)
}

fn provider_timing_role_value(
    input: &crate::state::ProviderInputField,
    duration: f64,
    fps: f64,
    frame_count: u32,
) -> Option<Value> {
    let role = input.role?;
    let raw = match role {
        InputRole::DurationSeconds => duration,
        InputRole::Fps => fps,
        InputRole::FrameCount => frame_count as f64,
        InputRole::Width
        | InputRole::Height
        | InputRole::Seed
        | InputRole::LeftVideo
        | InputRole::RightVideo
        | InputRole::LeftReplaceFrames
        | InputRole::RightReplaceFrames
        | InputRole::EdgeBlendFrames => return None,
    };
    let raw = clamp_provider_input_number(raw, input);
    match input.input_type {
        ProviderInputType::Integer => Some(Value::Number((raw.round() as i64).into())),
        ProviderInputType::Number => serde_json::Number::from_f64(raw).map(Value::Number),
        _ => None,
    }
}

fn clamp_provider_input_number(value: f64, input: &crate::state::ProviderInputField) -> f64 {
    let mut value = value;
    if let Some(min) = input.ui.as_ref().and_then(|ui| ui.min) {
        value = value.max(min);
    }
    if let Some(max) = input.ui.as_ref().and_then(|ui| ui.max) {
        value = value.min(max);
    }
    value
}

fn normalize_media_reference_slots_to_inputs(
    config: &mut GenerativeConfig,
    provider: Option<&ProviderEntry>,
) {
    let Some(provider) = provider else {
        return;
    };

    for input in provider.inputs.iter() {
        if !matches!(
            input.input_type,
            ProviderInputType::Image | ProviderInputType::Video | ProviderInputType::Audio
        ) || config.inputs.contains_key(&input.name)
        {
            continue;
        }

        let value = config
            .reference_slots
            .get(&input.name)
            .cloned()
            .or_else(|| {
                semantic_reference_slot(input)
                    .and_then(|slot| config.reference_slots.get(slot).cloned())
            });

        if let Some(value) = value {
            config.inputs.insert(input.name.clone(), value);
        }
    }
}

fn apply_active_generation_version_to_config(config: &mut GenerativeConfig, version: &str) -> bool {
    let Some(record) = config
        .versions
        .iter()
        .find(|record| record.version == version)
        .cloned()
    else {
        return false;
    };
    config.active_version = Some(version.to_string());
    config.provider_id = Some(record.provider_id);
    config.inputs = record.inputs_snapshot;
    if let Some(node_id) = record.lab_node_id {
        config.lab_graph.selected_node_id = Some(node_id);
        if let Some(node) = config
            .lab_graph
            .nodes
            .iter_mut()
            .find(|node| node.id == node_id)
        {
            node.output_version = Some(version.to_string());
        }
    }
    true
}

fn validate_generative_input_refs(
    project: &Project,
    values: &std::collections::HashMap<String, InputValue>,
) -> Result<(), String> {
    for (name, value) in values {
        match value {
            InputValue::AssetRef {
                asset_id,
                source_clip_id,
                ..
            } => {
                if project.find_asset(*asset_id).is_none() {
                    return Err(format!("Input {name} references missing asset {asset_id}."));
                }
                if let Some(source_clip_id) = source_clip_id {
                    let Some(clip) = project.clips.iter().find(|clip| clip.id == *source_clip_id)
                    else {
                        return Err(format!(
                            "Input {name} references missing source clip {source_clip_id}."
                        ));
                    };
                    if clip.asset_id != *asset_id {
                        return Err(format!(
                            "Input {name} source clip {source_clip_id} does not belong to asset {asset_id}."
                        ));
                    }
                }
            }
            InputValue::GenerationRef {
                asset_id, version, ..
            } => {
                if project.find_asset(*asset_id).is_none() {
                    return Err(format!("Input {name} references missing asset {asset_id}."));
                }
                let Some(config) = project.generative_config(*asset_id) else {
                    return Err(format!(
                        "Input {name} references non-generative asset {asset_id}."
                    ));
                };
                if !config
                    .versions
                    .iter()
                    .any(|record| record.version == *version)
                {
                    return Err(format!(
                        "Input {name} references missing generation version {version}."
                    ));
                }
            }
            InputValue::Literal { .. } => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_custom_http_provider_api_key() {
        let provider = ProviderEntry::new(
            "custom",
            ProviderOutputType::Image,
            ProviderConnection::CustomHttp {
                base_url: "http://localhost:1234".to_string(),
                api_key: Some("secret-token".to_string()),
            },
        );

        let value = redacted_provider_entry_json(&provider);
        let connection = value["connection"].as_object().expect("connection object");
        assert_eq!(connection.get("api_key"), None);
        assert_eq!(
            connection
                .get("api_key_present")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn move_clip_validates_target_track_before_changing_start_time() {
        let mut editor = EditorState::new();
        editor.project = Project::new("move-test");
        let asset_id = editor
            .project
            .add_asset(Asset::new_image("still", PathBuf::from("still.png")));
        let video_track_id = editor
            .project
            .tracks
            .iter()
            .find(|track| track.track_type == crate::state::TrackType::Video)
            .map(|track| track.id)
            .expect("video track");
        let audio_track_id = editor
            .project
            .tracks
            .iter()
            .find(|track| track.track_type == crate::state::TrackType::Audio)
            .map(|track| track.id)
            .expect("audio track");
        let clip_id = editor
            .project
            .add_clip_from_asset_to_track(asset_id, video_track_id, 1.0, 2.0)
            .expect("clip");

        let response = editor.apply_automation_command(&AutomationCommand::MoveClip {
            clip_id,
            start_time: 5.0,
            track_id: Some(audio_track_id),
        });

        assert!(!response.ok);
        let clip = editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .expect("clip retained");
        assert_eq!(clip.start_time, 1.0);
        assert_eq!(clip.track_id, video_track_id);
    }

    #[test]
    fn move_clips_absolute_moves_multiple_clips_atomically() {
        let mut editor = EditorState::new();
        editor.project = Project::new("batch-move-test");
        let asset_a = editor
            .project
            .add_asset(Asset::new_image("a", PathBuf::from("a.png")));
        let asset_b = editor
            .project
            .add_asset(Asset::new_image("b", PathBuf::from("b.png")));
        let video_track_a = editor
            .project
            .tracks
            .iter()
            .find(|track| track.track_type == crate::state::TrackType::Video)
            .map(|track| track.id)
            .expect("video track");
        let video_track_b = editor.project.add_video_track();
        let clip_a = editor
            .project
            .add_clip_from_asset_to_track(asset_a, video_track_a, 1.0, 2.0)
            .expect("clip a");
        let clip_b = editor
            .project
            .add_clip_from_asset_to_track(asset_b, video_track_a, 3.0, 2.0)
            .expect("clip b");

        let response = editor.apply_automation_command(&AutomationCommand::MoveClips {
            mode: ClipMoveMode::Absolute,
            moves: vec![
                ClipMoveTarget {
                    clip_id: clip_a,
                    start_time: Some(5.0),
                    track_id: Some(video_track_b),
                },
                ClipMoveTarget {
                    clip_id: clip_b,
                    start_time: Some(7.5),
                    track_id: Some(video_track_b),
                },
            ],
            clip_ids: Vec::new(),
            delta_seconds: None,
            track_delta: None,
            track_id: None,
        });

        assert!(response.ok);
        let moved_a = editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_a)
            .expect("moved a");
        let moved_b = editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_b)
            .expect("moved b");
        assert_eq!(moved_a.start_time, 5.0);
        assert_eq!(moved_b.start_time, 7.5);
        assert_eq!(moved_a.track_id, video_track_b);
        assert_eq!(moved_b.track_id, video_track_b);
        assert_eq!(editor.selection.clip_ids, vec![clip_a, clip_b]);
    }

    #[test]
    fn move_clips_relative_preserves_group_and_moves_track_delta() {
        let mut editor = EditorState::new();
        editor.project = Project::new("relative-move-test");
        let asset_a = editor
            .project
            .add_asset(Asset::new_image("a", PathBuf::from("a.png")));
        let asset_b = editor
            .project
            .add_asset(Asset::new_image("b", PathBuf::from("b.png")));
        let top_video = editor
            .project
            .tracks
            .iter()
            .position(|track| track.track_type == crate::state::TrackType::Video)
            .expect("video track index");
        let video_track_a = editor.project.tracks[top_video].id;
        let video_track_b = editor
            .project
            .insert_track(crate::state::TrackType::Video, top_video + 1);
        let clip_a = editor
            .project
            .add_clip_from_asset_to_track(asset_a, video_track_a, 1.0, 2.0)
            .expect("clip a");
        let clip_b = editor
            .project
            .add_clip_from_asset_to_track(asset_b, video_track_a, 3.0, 2.0)
            .expect("clip b");

        let response = editor.apply_automation_command(&AutomationCommand::MoveClips {
            mode: ClipMoveMode::Relative,
            moves: Vec::new(),
            clip_ids: vec![clip_a, clip_b],
            delta_seconds: Some(2.5),
            track_delta: Some(1),
            track_id: None,
        });

        assert!(response.ok);
        let moved_a = editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_a)
            .expect("moved a");
        let moved_b = editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_b)
            .expect("moved b");
        assert_eq!(moved_a.start_time, 3.5);
        assert_eq!(moved_b.start_time, 5.5);
        assert_eq!(moved_a.track_id, video_track_b);
        assert_eq!(moved_b.track_id, video_track_b);
    }

    #[test]
    fn move_clips_rejects_incompatible_target_without_partial_mutation() {
        let mut editor = EditorState::new();
        editor.project = Project::new("batch-move-conflict-test");
        let asset_a = editor
            .project
            .add_asset(Asset::new_image("a", PathBuf::from("a.png")));
        let asset_b = editor
            .project
            .add_asset(Asset::new_image("b", PathBuf::from("b.png")));
        let video_track = editor
            .project
            .tracks
            .iter()
            .find(|track| track.track_type == crate::state::TrackType::Video)
            .map(|track| track.id)
            .expect("video track");
        let audio_track = editor
            .project
            .tracks
            .iter()
            .find(|track| track.track_type == crate::state::TrackType::Audio)
            .map(|track| track.id)
            .expect("audio track");
        let clip_a = editor
            .project
            .add_clip_from_asset_to_track(asset_a, video_track, 1.0, 2.0)
            .expect("clip a");
        let clip_b = editor
            .project
            .add_clip_from_asset_to_track(asset_b, video_track, 3.0, 2.0)
            .expect("clip b");

        let response = editor.apply_automation_command(&AutomationCommand::MoveClips {
            mode: ClipMoveMode::Absolute,
            moves: vec![
                ClipMoveTarget {
                    clip_id: clip_a,
                    start_time: Some(5.0),
                    track_id: None,
                },
                ClipMoveTarget {
                    clip_id: clip_b,
                    start_time: Some(7.0),
                    track_id: Some(audio_track),
                },
            ],
            clip_ids: Vec::new(),
            delta_seconds: None,
            track_delta: None,
            track_id: None,
        });

        assert!(!response.ok);
        assert_eq!(response.http_status, 409);
        let retained_a = editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_a)
            .expect("retained a");
        let retained_b = editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_b)
            .expect("retained b");
        assert_eq!(retained_a.start_time, 1.0);
        assert_eq!(retained_b.start_time, 3.0);
        assert_eq!(retained_a.track_id, video_track);
        assert_eq!(retained_b.track_id, video_track);
    }

    #[test]
    fn delete_commands_report_missing_ids() {
        let mut editor = EditorState::new();
        editor.project = Project::new("delete-test");
        let missing_asset = Uuid::new_v4();
        let response = editor.apply_automation_command(&AutomationCommand::DeleteAssets {
            asset_ids: vec![missing_asset],
        });
        assert!(!response.ok);
        assert_eq!(response.http_status, 404);

        let missing_clip = Uuid::new_v4();
        let response = editor.apply_automation_command(&AutomationCommand::DeleteClips {
            clip_ids: vec![missing_clip],
        });
        assert!(!response.ok);
        assert_eq!(response.http_status, 404);
    }

    #[test]
    fn generative_config_rejects_mismatched_provider_output_type() {
        let mut editor = EditorState::new();
        editor.project = Project::new("provider-mismatch-test");
        let asset_id = Uuid::new_v4();
        let mut asset = Asset::new_generative_image("gen image", PathBuf::from("generated/image"));
        asset.id = asset_id;
        editor.project.add_asset(asset);
        editor
            .project
            .generative_configs
            .insert(asset_id, GenerativeConfig::default());
        let provider = ProviderEntry::new(
            "video provider",
            ProviderOutputType::Video,
            ProviderConnection::ComfyUi {
                base_url: "http://127.0.0.1:8188".to_string(),
                workflow_path: None,
                manifest: None,
            },
        );
        let provider_id = provider.id;
        editor.provider_entries.push(provider);

        let response = editor.apply_automation_command(&AutomationCommand::SetGenerativeConfig {
            asset_id,
            patch: crate::core::automation::GenerativeConfigPatch {
                provider_id: Some(provider_id),
                ..Default::default()
            },
        });

        assert!(!response.ok);
        assert_eq!(response.http_status, 409);
    }
}

pub fn default_projects_dir() -> PathBuf {
    crate::core::paths::app_projects_root()
}

fn list_project_folders(root: Option<&Path>) -> Result<serde_json::Value, String> {
    let root = root
        .map(Path::to_path_buf)
        .unwrap_or_else(default_projects_dir);
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({
                "root": root,
                "projects": [],
            }));
        }
        Err(err) => {
            return Err(format!(
                "Failed to list projects under {}: {err}",
                root.display()
            ));
        }
    };

    let mut projects = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let project_file = path.join("project.json");
        if !project_file.is_file() {
            continue;
        }

        let modified_unix_ms = fs::metadata(&project_file)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis());

        let project_json = fs::read_to_string(&project_file)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        let name = project_json
            .as_ref()
            .and_then(|value| value.get("name"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "Untitled".to_string());
        let settings = project_json
            .as_ref()
            .and_then(|value| value.get("settings"))
            .cloned()
            .unwrap_or_else(|| json!({}));

        projects.push(json!({
            "name": name,
            "path": path,
            "project_file": project_file,
            "settings": settings,
            "modified_unix_ms": modified_unix_ms,
        }));
    }
    projects.sort_by(|a, b| {
        let left = a
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let right = b
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        left.cmp(&right)
    });

    Ok(json!({
        "root": root,
        "projects": projects,
    }))
}

fn include_requested(include: &[String], name: &str) -> bool {
    include
        .iter()
        .any(|value| value.eq_ignore_ascii_case(name) || value.eq_ignore_ascii_case("all"))
}

pub fn default_generative_video_fps() -> f64 {
    DEFAULT_GENERATIVE_VIDEO_FPS
}

pub fn default_generative_video_frames() -> u32 {
    DEFAULT_GENERATIVE_VIDEO_FRAME_COUNT
}

fn is_generative_image(kind: &AssetKind) -> bool {
    matches!(kind, AssetKind::GenerativeImage { .. })
}

fn is_generative_audio(kind: &AssetKind) -> bool {
    matches!(kind, AssetKind::GenerativeAudio { .. })
}

fn generative_asset_folder(media_type: &str, asset_id: Uuid) -> PathBuf {
    PathBuf::from("generated")
        .join(media_type)
        .join(asset_id.to_string())
}

fn generative_folder(asset: &Asset) -> Option<&PathBuf> {
    match &asset.kind {
        AssetKind::GenerativeVideo { folder, .. }
        | AssetKind::GenerativeImage { folder, .. }
        | AssetKind::GenerativeAudio { folder, .. } => Some(folder),
        _ => None,
    }
}

fn generative_media_type(asset: &Asset) -> Option<&'static str> {
    match &asset.kind {
        AssetKind::GenerativeVideo { .. } => Some("video"),
        AssetKind::GenerativeImage { .. } => Some("image"),
        AssetKind::GenerativeAudio { .. } => Some("audio"),
        _ => None,
    }
}

fn set_generative_folder(asset: &mut Asset, next_folder: PathBuf) {
    match &mut asset.kind {
        AssetKind::GenerativeVideo { folder, .. }
        | AssetKind::GenerativeImage { folder, .. }
        | AssetKind::GenerativeAudio { folder, .. } => *folder = next_folder,
        _ => {}
    }
}

fn project_local_folder(project_root: &Path, folder: &Path) -> Option<PathBuf> {
    if folder.is_absolute()
        || folder.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return None;
    }
    Some(project_root.join(folder))
}

fn unique_asset_copy_name(assets: &[Asset], source_name: &str) -> String {
    unique_asset_name(assets, &format!("{source_name} Copy"))
}

fn unique_asset_name(assets: &[Asset], base_name: &str) -> String {
    let base = base_name.trim();
    let base = if base.is_empty() { "Asset" } else { base };
    if !assets.iter().any(|asset| asset.name == base) {
        return base.to_string();
    }

    let mut index = 2u32;
    loop {
        let candidate = format!("{base} {index}");
        if !assets.iter().any(|asset| asset.name == candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|err| format!("Failed to create copy folder: {err}"))?;
    if !source.exists() {
        return Ok(());
    }

    for entry in
        fs::read_dir(source).map_err(|err| format!("Failed to read source folder: {err}"))?
    {
        let entry = entry.map_err(|err| format!("Failed to read source folder entry: {err}"))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if source_path.is_file() {
            fs::copy(&source_path, &target_path)
                .map_err(|err| format!("Failed to copy {}: {err}", source_path.display()))?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ExtractedAssetOutput {
    Image,
    Video,
    Audio,
}

struct GenerativeExtractionSource {
    path: PathBuf,
    output: ExtractedAssetOutput,
    target_subdir: &'static str,
    default_extension: &'static str,
    version: Option<String>,
}

const EXTRACT_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const EXTRACT_VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi"];
const EXTRACT_AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg"];

fn generative_source_for_extraction(
    project_root: &Path,
    asset: &Asset,
    version_override: Option<&str>,
) -> Option<GenerativeExtractionSource> {
    match &asset.kind {
        AssetKind::GenerativeImage {
            folder,
            active_version,
        } => {
            let version = version_override.or(active_version.as_deref());
            Some(GenerativeExtractionSource {
                path: resolve_generative_output_file(
                    project_root,
                    folder,
                    version,
                    EXTRACT_IMAGE_EXTENSIONS,
                )?,
                output: ExtractedAssetOutput::Image,
                target_subdir: "images",
                default_extension: "png",
                version: version
                    .map(str::to_string)
                    .or_else(|| active_version.clone()),
            })
        }
        AssetKind::GenerativeVideo {
            folder,
            active_version,
            ..
        } => {
            let version = version_override.or(active_version.as_deref());
            Some(GenerativeExtractionSource {
                path: resolve_generative_output_file(
                    project_root,
                    folder,
                    version,
                    EXTRACT_VIDEO_EXTENSIONS,
                )?,
                output: ExtractedAssetOutput::Video,
                target_subdir: "video",
                default_extension: "mp4",
                version: version
                    .map(str::to_string)
                    .or_else(|| active_version.clone()),
            })
        }
        AssetKind::GenerativeAudio {
            folder,
            active_version,
        } => {
            let version = version_override.or(active_version.as_deref());
            Some(GenerativeExtractionSource {
                path: resolve_generative_output_file(
                    project_root,
                    folder,
                    version,
                    EXTRACT_AUDIO_EXTENSIONS,
                )?,
                output: ExtractedAssetOutput::Audio,
                target_subdir: "audio",
                default_extension: "wav",
                version: version
                    .map(str::to_string)
                    .or_else(|| active_version.clone()),
            })
        }
        _ => None,
    }
}

fn resolve_generative_output_file(
    project_root: &Path,
    folder: &Path,
    active_version: Option<&str>,
    extensions: &[&str],
) -> Option<PathBuf> {
    let active_version = active_version?;
    let folder_path = project_root.join(folder);
    for extension in extensions {
        let candidate = folder_path.join(format!("{active_version}.{extension}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn sanitize_file_stem(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_') {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }
    let sanitized = sanitized.trim().trim_matches('_').trim();
    if sanitized.is_empty() {
        "asset".to_string()
    } else {
        sanitized.to_string()
    }
}

fn unique_file_path(parent: &Path, stem: &str, extension: &str) -> PathBuf {
    let extension = extension.trim_start_matches('.');
    let mut index = 0u32;
    loop {
        let file_name = if index == 0 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem}_{index}.{extension}")
        };
        let candidate = parent.join(file_name);
        if !candidate.exists() {
            return candidate;
        }
        index += 1;
    }
}

fn deleted_assets_status(assets: usize, clips: usize) -> String {
    let asset_label = if assets == 1 {
        "Deleted asset".to_string()
    } else {
        format!("Deleted {assets} assets")
    };
    if clips == 0 {
        asset_label
    } else if clips == 1 {
        format!("{asset_label} and 1 timeline clip")
    } else {
        format!("{asset_label} and {clips} timeline clips")
    }
}

fn is_generative_video(kind: &AssetKind) -> bool {
    matches!(kind, AssetKind::GenerativeVideo { .. })
}
