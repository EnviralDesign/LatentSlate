#[derive(Debug, Clone)]
pub struct AppShellModel {
    pub project_name: String,
    pub project_summary: String,
    pub assets_summary: String,
    pub queue_summary: String,
    pub preview_status: String,
    pub timeline_status: String,
    pub attributes_status: String,
    pub status_text: String,
    pub playback_text: String,
}

impl Default for AppShellModel {
    fn default() -> Self {
        Self::from_project(&crate::state::Project::default())
    }
}

impl AppShellModel {
    pub fn from_project(project: &crate::state::Project) -> Self {
        let settings = &project.settings;
        let duration_seconds = project.duration().max(settings.duration_seconds);
        let fps_whole = (settings.fps.max(1.0).round() as u64).max(1);
        let duration_frames = (duration_seconds * settings.fps.max(1.0)).round() as u64;
        let total_seconds = duration_frames / fps_whole;
        let seconds = total_seconds % 60;
        let total_minutes = total_seconds / 60;
        let minutes = total_minutes % 60;
        let hours = total_minutes / 60;

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
            queue_summary: "Queue idle".to_string(),
            preview_status: format!(
                "Preview shell online. GPU compositor will move here after the model/controller split."
            ),
            timeline_status: format!(
                "Timeline shell online. Current project duration {:02}:{:02}:{:02}.",
                hours, minutes, seconds
            ),
            attributes_status: "Attributes inspector will bind to the shared app model.".to_string(),
            status_text: "Slint shell bootstrapped. Dioxus UI remains on disk only as migration reference.".to_string(),
            playback_text: "Stopped".to_string(),
        }
    }
}
