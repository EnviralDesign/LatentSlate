use std::path::{Path, PathBuf};

use crate::state::{Project, ProjectSettings, SelectionState};

#[derive(Debug, Clone)]
pub struct ProjectListEntry {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupTab {
    Open,
    New,
}

#[derive(Debug, Clone)]
pub struct StartupDraft {
    pub name: String,
    pub parent_dir: PathBuf,
    pub width: String,
    pub height: String,
    pub fps: String,
    pub duration_minutes: String,
    pub preview_max_width: String,
    pub preview_max_height: String,
}

impl StartupDraft {
    pub fn from_settings(
        name: impl Into<String>,
        parent_dir: PathBuf,
        settings: &ProjectSettings,
    ) -> Self {
        Self {
            name: name.into(),
            parent_dir,
            width: settings.width.to_string(),
            height: settings.height.to_string(),
            fps: format!("{:.0}", settings.fps),
            duration_minutes: format_duration_minutes(settings.duration_seconds),
            preview_max_width: settings.preview_max_width.to_string(),
            preview_max_height: settings.preview_max_height.to_string(),
        }
    }

    pub fn default_parent_dir() -> PathBuf {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("projects")
    }

    pub fn default_new_project() -> Self {
        Self::from_settings(
            "My New Project",
            Self::default_parent_dir(),
            &ProjectSettings::default(),
        )
    }

    pub fn project_dir(&self) -> PathBuf {
        self.parent_dir.join(self.name.trim())
    }

    pub fn can_create(&self) -> bool {
        !self.name.trim().is_empty()
            && self.width.trim().parse::<u32>().ok().filter(|v| *v >= 16).is_some()
            && self.height.trim().parse::<u32>().ok().filter(|v| *v >= 16).is_some()
            && self.fps.trim().parse::<f64>().ok().filter(|v| *v >= 1.0).is_some()
            && self
                .duration_minutes
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| *v > 0.0)
                .is_some()
            && self
                .preview_max_width
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|v| *v >= 16)
                .is_some()
            && self
                .preview_max_height
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|v| *v >= 16)
                .is_some()
    }

    pub fn build_settings(&self) -> Result<ProjectSettings, String> {
        let width = parse_u32(&self.width, "width", 16)?;
        let height = parse_u32(&self.height, "height", 16)?;
        let fps = parse_f64(&self.fps, "fps", 1.0)?;
        let duration_minutes = parse_f64(&self.duration_minutes, "duration", 0.01)?;
        let preview_max_width = parse_u32(&self.preview_max_width, "preview width", 16)?;
        let preview_max_height = parse_u32(&self.preview_max_height, "preview height", 16)?;

        Ok(ProjectSettings {
            width,
            height,
            fps,
            duration_seconds: duration_minutes * 60.0,
            preview_max_width,
            preview_max_height,
        })
    }
}

#[derive(Debug, Clone)]
pub struct StartupState {
    pub visible: bool,
    pub can_close: bool,
    pub tab: StartupTab,
    pub draft: StartupDraft,
    pub available_projects: Vec<ProjectListEntry>,
    pub selected_project_index: i32,
    pub error_message: String,
}

impl StartupState {
    fn build(can_close: bool, tab: StartupTab) -> Self {
        let available_projects = scan_projects(&StartupDraft::default_parent_dir());
        let selected_project_index = if available_projects.is_empty() { -1 } else { 0 };
        Self {
            visible: true,
            can_close,
            tab,
            draft: StartupDraft::default_new_project(),
            available_projects,
            selected_project_index,
            error_message: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppModel {
    pub project: Project,
    pub selection: SelectionState,
    pub startup: StartupState,
    pub status_message: String,
    pub queue_status: String,
    pub playback_status: String,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            project: Project::default(),
            selection: SelectionState::default(),
            startup: StartupState::build(false, StartupTab::Open),
            status_message:
                "Slint shell bootstrapped. Open a project or create a new one to continue."
                    .to_string(),
            queue_status: "Queue idle".to_string(),
            playback_status: "Stopped".to_string(),
        }
    }
}

impl AppModel {
    pub fn show_startup_modal_new(&mut self) {
        self.startup = StartupState::build(self.has_loaded_project(), StartupTab::New);
        self.status_message = if self.has_loaded_project() {
            "Creating a new project. Current project remains untouched until you confirm."
                .to_string()
        } else {
            "Create a new project or open an existing one.".to_string()
        };
    }

    pub fn show_startup_modal_open(&mut self) {
        self.startup = StartupState::build(self.has_loaded_project(), StartupTab::Open);
        self.status_message = "Open an existing project or create a new one.".to_string();
    }

    pub fn hide_startup_modal(&mut self) {
        if self.startup.can_close {
            self.startup.visible = false;
            self.startup.error_message.clear();
            self.status_message = project_loaded_message(&self.project);
        }
    }

    pub fn update_startup_name(&mut self, value: String) {
        self.startup.draft.name = value;
        self.startup.error_message.clear();
    }

    pub fn update_startup_width(&mut self, value: String) {
        self.startup.draft.width = value;
        self.startup.error_message.clear();
    }

    pub fn update_startup_height(&mut self, value: String) {
        self.startup.draft.height = value;
        self.startup.error_message.clear();
    }

    pub fn update_startup_fps(&mut self, value: String) {
        self.startup.draft.fps = value;
        self.startup.error_message.clear();
    }

    pub fn update_startup_duration_minutes(&mut self, value: String) {
        self.startup.draft.duration_minutes = value;
        self.startup.error_message.clear();
    }

    pub fn update_startup_preview_max_width(&mut self, value: String) {
        self.startup.draft.preview_max_width = value;
        self.startup.error_message.clear();
    }

    pub fn update_startup_preview_max_height(&mut self, value: String) {
        self.startup.draft.preview_max_height = value;
        self.startup.error_message.clear();
    }

    pub fn update_startup_tab(&mut self, value: String) {
        self.startup.tab = if value.trim() == "0" {
            StartupTab::Open
        } else {
            StartupTab::New
        };
        self.startup.error_message.clear();
    }

    pub fn update_startup_selected_project(&mut self, value: String) {
        let index = value.trim().parse::<i32>().unwrap_or(-1);
        self.startup.selected_project_index = index;
        self.startup.error_message.clear();
    }

    pub fn set_startup_parent_dir(&mut self, path: PathBuf) {
        self.startup.draft.parent_dir = path;
        self.startup.error_message.clear();
    }

    pub fn create_project_from_startup(&mut self) -> Result<(), String> {
        let name = self.startup.draft.name.trim();
        if name.is_empty() {
            return Err("Project name is required.".to_string());
        }

        let settings = self.startup.draft.build_settings()?;
        let project_dir = self.startup.draft.project_dir();
        let project =
            Project::create_in_with_settings(&project_dir, name, settings).map_err(io_error_text)?;
        self.load_project(project, format!("Created project at {}", project_dir.display()));
        Ok(())
    }

    pub fn open_project(&mut self, folder: &Path) -> Result<(), String> {
        let project = Project::load(folder).map_err(io_error_text)?;
        self.load_project(project, format!("Opened project {}", folder.display()));
        Ok(())
    }

    pub fn open_selected_startup_project(&mut self) -> Result<(), String> {
        let index = self.startup.selected_project_index;
        let Some(entry) = (index >= 0)
            .then(|| self.startup.available_projects.get(index as usize))
            .flatten()
            .cloned()
        else {
            return Err("Select a project from the list or use Browse to choose one.".to_string());
        };

        self.open_project(&entry.path)
    }

    pub fn save_project(&mut self) -> Result<(), String> {
        self.project.save().map_err(io_error_text)?;
        self.status_message = project_saved_message(&self.project);
        Ok(())
    }

    pub fn startup_can_create(&self) -> bool {
        self.startup.draft.can_create()
    }

    pub fn startup_can_open_selected(&self) -> bool {
        self.startup.selected_project_index >= 0
            && (self.startup.selected_project_index as usize) < self.startup.available_projects.len()
    }

    pub fn has_loaded_project(&self) -> bool {
        self.project.project_path.is_some()
    }

    fn load_project(&mut self, project: Project, status_message: String) {
        self.project = project;
        self.selection = SelectionState::default();
        self.startup.visible = false;
        self.startup.can_close = true;
        self.startup.error_message.clear();
        self.status_message = status_message;
    }
}

fn parse_u32(value: &str, label: &str, min: u32) -> Result<u32, String> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("Invalid {}.", label))
        .and_then(|parsed| {
            if parsed >= min {
                Ok(parsed)
            } else {
                Err(format!("{} must be at least {}.", capitalize(label), min))
            }
        })
}

fn parse_f64(value: &str, label: &str, min: f64) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("Invalid {}.", label))
        .and_then(|parsed| {
            if parsed >= min {
                Ok(parsed)
            } else {
                Err(format!("{} must be at least {}.", capitalize(label), min))
            }
        })
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn format_duration_minutes(duration_seconds: f64) -> String {
    let minutes = duration_seconds / 60.0;
    if (minutes.round() - minutes).abs() < f64::EPSILON {
        format!("{:.0}", minutes)
    } else {
        format!("{:.2}", minutes)
    }
}

fn scan_projects(root: &Path) -> Vec<ProjectListEntry> {
    let mut projects: Vec<ProjectListEntry> = std::fs::read_dir(root)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("project.json").exists())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_string();
            Some(ProjectListEntry { name, path })
        })
        .collect();
    projects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    projects
}

fn io_error_text(error: std::io::Error) -> String {
    error.to_string()
}

fn project_loaded_message(project: &Project) -> String {
    let path = project
        .project_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unsaved project".to_string());
    format!("Loaded {}", path)
}

fn project_saved_message(project: &Project) -> String {
    let path = project
        .project_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "project".to_string());
    format!("Saved {}", path)
}
