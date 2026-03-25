use slint::ComponentHandle;

use crate::ui::MainWindow;

use super::model::AppShellModel;

pub struct SlintAppController {
    window: MainWindow,
}

impl SlintAppController {
    pub fn new(model: AppShellModel) -> Result<Self, slint::PlatformError> {
        let window = MainWindow::new()?;
        Self::apply_model(&window, &model);
        Self::wire_callbacks(&window);
        Ok(Self { window })
    }

    pub fn run(self) -> Result<(), slint::PlatformError> {
        self.window.run()
    }

    fn apply_model(window: &MainWindow, model: &AppShellModel) {
        window.set_project_name(model.project_name.clone().into());
        window.set_project_summary(model.project_summary.clone().into());
        window.set_assets_summary(model.assets_summary.clone().into());
        window.set_queue_summary(model.queue_summary.clone().into());
        window.set_preview_status(model.preview_status.clone().into());
        window.set_timeline_status(model.timeline_status.clone().into());
        window.set_attributes_status(model.attributes_status.clone().into());
        window.set_status_text(model.status_text.clone().into());
        window.set_playback_text(model.playback_text.clone().into());
    }

    fn wire_callbacks(window: &MainWindow) {
        window.on_request_new_project(|| {
            println!("[slint-shell] New Project requested");
        });
        window.on_request_open_project(|| {
            println!("[slint-shell] Open Project requested");
        });
        window.on_request_save_project(|| {
            println!("[slint-shell] Save Project requested");
        });
        window.on_request_open_queue(|| {
            println!("[slint-shell] Queue requested");
        });
        window.on_request_open_preferences(|| {
            println!("[slint-shell] Preferences requested");
        });
    }
}
