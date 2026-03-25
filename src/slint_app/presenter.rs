use super::model::AppModel;

#[derive(Debug, Clone)]
pub struct AppViewModel {
    pub project_name: String,
    pub project_summary: String,
    pub assets_summary: String,
    pub queue_summary: String,
    pub preview_status: String,
    pub timeline_status: String,
    pub attributes_status: String,
    pub status_text: String,
    pub playback_text: String,
    pub can_save_project: bool,
    pub show_startup_modal: bool,
    pub startup_can_close: bool,
    pub startup_can_create: bool,
    pub startup_parent_dir: String,
    pub startup_name: String,
    pub startup_width: String,
    pub startup_height: String,
    pub startup_fps: String,
    pub startup_duration_minutes: String,
    pub startup_preview_max_width: String,
    pub startup_preview_max_height: String,
    pub startup_error: String,
}

impl From<&AppModel> for AppViewModel {
    fn from(model: &AppModel) -> Self {
        let project = &model.project;
        let settings = &project.settings;
        let duration_seconds = project.duration().max(settings.duration_seconds);
        let fps_whole = (settings.fps.max(1.0).round() as u64).max(1);
        let duration_frames = (duration_seconds * settings.fps.max(1.0)).round() as u64;
        let total_seconds = duration_frames / fps_whole;
        let seconds = total_seconds % 60;
        let total_minutes = total_seconds / 60;
        let minutes = total_minutes % 60;
        let hours = total_minutes / 60;
        let selected_count =
            model.selection.clip_ids.len() + model.selection.track_ids.len() + model.selection.marker_ids.len();

        Self {
            project_name: project.name.clone(),
            project_summary: format!(
                "{} x {} at {:.0} fps",
                settings.width, settings.height, settings.fps
            ),
            assets_summary: format!(
                "{} assets, {} tracks, {} clips",
                project.assets.len(),
                project.tracks.len(),
                project.clips.len()
            ),
            queue_summary: model.queue_status.clone(),
            preview_status: "Preview shell ready. Next step is moving the real preview service and compositor bridge into this panel."
                .to_string(),
            timeline_status: format!(
                "Timeline shell ready. Current project duration {:02}:{:02}:{:02}.",
                hours, minutes, seconds
            ),
            attributes_status: if selected_count == 0 {
                "No selection yet. The shared app model is now in place and the attributes inspector can bind into it next."
                    .to_string()
            } else {
                format!("{} selected item(s).", selected_count)
            },
            status_text: model.status_message.clone(),
            playback_text: model.playback_status.clone(),
            can_save_project: model.has_loaded_project(),
            show_startup_modal: model.startup.visible,
            startup_can_close: model.startup.can_close,
            startup_can_create: model.startup_can_create(),
            startup_parent_dir: model.startup.draft.parent_dir.display().to_string(),
            startup_name: model.startup.draft.name.clone(),
            startup_width: model.startup.draft.width.clone(),
            startup_height: model.startup.draft.height.clone(),
            startup_fps: model.startup.draft.fps.clone(),
            startup_duration_minutes: model.startup.draft.duration_minutes.clone(),
            startup_preview_max_width: model.startup.draft.preview_max_width.clone(),
            startup_preview_max_height: model.startup.draft.preview_max_height.clone(),
            startup_error: model.startup.error_message.clone(),
        }
    }
}
