use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use crate::constants::{DEFAULT_CLIP_DURATION_SECONDS, PREVIEW_CACHE_BUDGET_BYTES};
use crate::core::automation::{AutomationCommand, AutomationResponse};
use crate::core::media::{probe_missing_duration, resolve_asset_duration_seconds};
use crate::core::provider_store::{
    list_local_provider_files, load_local_provider_entries_or_empty,
};
use crate::core::thumbnailer::Thumbnailer;
use crate::state::{
    next_generative_index, Asset, AssetKind, GenerationJob, Project, ProjectSettings,
    ProjectWorkspaceLayout, ProviderEntry, SelectionState, DEFAULT_GENERATIVE_VIDEO_FPS,
    DEFAULT_GENERATIVE_VIDEO_FRAME_COUNT,
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
    pub generative_video: bool,
    pub export_video: bool,
    pub api_keys: bool,
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
        }
    }

    pub fn show_startup(&self) -> bool {
        self.project.project_path.is_none() && !self.startup_done
    }

    pub fn refresh_providers(&mut self) {
        self.provider_entries = load_local_provider_entries_or_empty();
        self.provider_files = list_local_provider_files();
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
        self.status = "Saved".to_string();
        Ok(())
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
            AutomationCommand::GetState => AutomationResponse::ok(self.state_json()),
            AutomationCommand::GetUi
            | AutomationCommand::ClickUi { .. }
            | AutomationCommand::TextUi { .. }
            | AutomationCommand::Screenshot { .. }
            | AutomationCommand::GetPerformanceDiagnostics
            | AutomationCommand::ScrubTimelineProfile { .. } => AutomationResponse::with_status(
                "UI automation commands must be handled by the egui runtime.",
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
            AutomationCommand::AddAssetToTimeline {
                asset_id,
                asset_name,
                time,
            } => {
                let selected = self.resolve_asset(*asset_id, asset_name.as_deref());
                match selected {
                    Some(asset_id) => match self.add_asset_to_timeline(asset_id, *time) {
                        Ok(clip_id) => AutomationResponse::ok(json!({ "clip_id": clip_id })),
                        Err(err) => AutomationResponse::error(err),
                    },
                    None => AutomationResponse::error("No matching asset found."),
                }
            }
            AutomationCommand::Seek { time } => {
                self.seek(*time);
                AutomationResponse::ok(json!({ "current_time": self.current_time }))
            }
            AutomationCommand::SelectClip { clip_id, index } => {
                let selected = (*clip_id).or_else(|| {
                    let index = (*index).unwrap_or(0);
                    self.project.clips.get(index).map(|clip| clip.id)
                });
                match selected {
                    Some(id) => {
                        self.selection.select_clip(id);
                        AutomationResponse::ok(json!({ "clip_id": id }))
                    }
                    None => AutomationResponse::error("No matching clip found."),
                }
            }
            AutomationCommand::SelectAsset { asset_id, index } => {
                let selected = (*asset_id).or_else(|| {
                    let index = (*index).unwrap_or(0);
                    self.project.assets.get(index).map(|asset| asset.id)
                });
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
                let selected = (*track_id).or_else(|| {
                    let index = (*index).unwrap_or(0);
                    self.project.tracks.get(index).map(|track| track.id)
                });
                match selected {
                    Some(id) => {
                        self.selection.select_track(id);
                        AutomationResponse::ok(json!({ "track_id": id }))
                    }
                    None => AutomationResponse::error("No matching track found."),
                }
            }
            AutomationCommand::SelectMarker { marker_id, index } => {
                let selected = (*marker_id).or_else(|| {
                    let index = (*index).unwrap_or(0);
                    self.project.markers.get(index).map(|marker| marker.id)
                });
                match selected {
                    Some(id) => {
                        self.selection.select_marker(id);
                        AutomationResponse::ok(json!({ "marker_id": id }))
                    }
                    None => AutomationResponse::error("No matching marker found."),
                }
            }
            AutomationCommand::AddMarker { time } => {
                let marker_id = self.add_marker(*time);
                AutomationResponse::ok(json!({ "marker_id": marker_id }))
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
                AutomationResponse::empty_ok()
            }
            AutomationCommand::CloseAllOverlays => {
                self.overlays = EditorOverlays::default();
                AutomationResponse::empty_ok()
            }
        }
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

    pub fn state_json(&self) -> serde_json::Value {
        json!({
            "project": {
                "name": self.project.name.clone(),
                "path": self.project.project_path.clone(),
                "settings": self.project.settings.clone(),
                "tracks": self.project.tracks.clone(),
                "assets": self.project.assets.clone(),
                "clips": self.project.clips.clone(),
                "markers": self.project.markers.clone(),
                "workspace_layout": self.layout.workspace_layout(),
            },
            "current_time": self.current_time,
            "startup_done": self.startup_done,
            "overlays": {
                "providers_open": self.overlays.providers,
                "project_settings_open": self.overlays.project_settings,
                "new_project_open": self.overlays.new_project,
                "queue_open": self.overlays.queue,
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
        })
    }
}

pub fn default_projects_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("projects")
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
