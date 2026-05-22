use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use crate::constants::{DEFAULT_CLIP_DURATION_SECONDS, PREVIEW_CACHE_BUDGET_BYTES};
use crate::core::automation::{AutomationCommand, AutomationResponse};
use crate::core::media::{probe_missing_duration, resolve_asset_duration_seconds};
use crate::core::provider_store::{
    list_global_provider_files, load_global_provider_entries_or_empty,
};
use crate::core::thumbnailer::Thumbnailer;
use crate::state::{
    next_generative_index, Asset, AssetKind, GenerationJob, Project, ProjectSettings,
    ProviderEntry, SelectionState, DEFAULT_GENERATIVE_VIDEO_FPS,
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
        }
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
            provider_entries: load_global_provider_entries_or_empty(),
            provider_files: list_global_provider_files(),
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
        self.provider_entries = load_global_provider_entries_or_empty();
        self.provider_files = list_global_provider_files();
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
        let time = time.unwrap_or(self.current_time);
        let marker = crate::state::Marker::new(time.clamp(0.0, self.project.duration()));
        let id = self.project.add_marker(marker);
        self.selection.select_marker(id);
        self.preview_dirty = true;
        id
    }

    pub fn save(&mut self) -> Result<(), String> {
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
        let folder = PathBuf::from("generated")
            .join("image")
            .join(format!("gen_image_{index:03}"));
        std::fs::create_dir_all(project_root.join(&folder))
            .map_err(|err| format!("Failed to create generated image folder: {err}"))?;
        let asset = Asset::new_generative_image(format!("Gen Image {index}"), folder);
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
        let folder = PathBuf::from("generated")
            .join("audio")
            .join(format!("gen_audio_{index:03}"));
        std::fs::create_dir_all(project_root.join(&folder))
            .map_err(|err| format!("Failed to create generated audio folder: {err}"))?;
        let asset = Asset::new_generative_audio(format!("Gen Audio {index}"), folder);
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
        let folder = PathBuf::from("generated")
            .join("video")
            .join(format!("gen_video_{index:03}"));
        std::fs::create_dir_all(project_root.join(&folder))
            .map_err(|err| format!("Failed to create generated video folder: {err}"))?;
        let fps = if fps.is_finite() && fps > 0.0 {
            fps
        } else {
            DEFAULT_GENERATIVE_VIDEO_FPS
        };
        let frame_count = frame_count.max(1);
        let asset =
            Asset::new_generative_video(format!("Gen Video {index}"), folder, fps, frame_count);
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
            },
            "layout": {
                "left_collapsed": self.layout.left_collapsed,
                "right_collapsed": self.layout.right_collapsed,
                "timeline_collapsed": self.layout.timeline_collapsed,
                "timeline_zoom": self.layout.timeline_zoom,
                "timeline_scroll_x": self.layout.timeline_scroll_x,
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

fn is_generative_video(kind: &AssetKind) -> bool {
    matches!(kind, AssetKind::GenerativeVideo { .. })
}
