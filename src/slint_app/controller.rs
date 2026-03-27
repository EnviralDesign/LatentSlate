use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::ui::MainWindow;

use super::commands::{AppCommand, StartupField};
use super::model::{AppModel, StartupDraft};
use super::presenter::AppViewModel;

pub struct SlintAppController {
    window: MainWindow,
    model: Rc<RefCell<AppModel>>,
}

impl SlintAppController {
    pub fn new(model: AppModel) -> Result<Self, slint::PlatformError> {
        let window = MainWindow::new()?;
        let controller = Self {
            window,
            model: Rc::new(RefCell::new(model)),
        };
        controller.sync_view();
        controller.wire_callbacks();
        Ok(controller)
    }

    pub fn run(self) -> Result<(), slint::PlatformError> {
        self.window.run()
    }

    fn sync_view(&self) {
        if let Ok(model) = self.model.try_borrow() {
            Self::apply_view_model(&self.window, &AppViewModel::from(&*model));
        }
    }

    fn apply_view_model(window: &MainWindow, view: &AppViewModel) {
        window.set_project_name(view.project_name.clone().into());
        window.set_project_summary(view.project_summary.clone().into());
        window.set_assets_summary(view.assets_summary.clone().into());
        window.set_queue_summary(view.queue_summary.clone().into());
        window.set_preview_status(view.preview_status.clone().into());
        window.set_timeline_status(view.timeline_status.clone().into());
        window.set_attributes_status(view.attributes_status.clone().into());
        window.set_status_text(view.status_text.clone().into());
        window.set_playback_text(view.playback_text.clone().into());
        window.set_can_save_project(view.can_save_project);
        window.set_show_startup_modal(view.show_startup_modal);
        window.set_startup_can_close(view.startup_can_close);
        window.set_startup_can_create(view.startup_can_create);
        window.set_startup_can_open_selected(view.startup_can_open_selected);
        window.set_startup_tab(view.startup_tab);
        window.set_startup_parent_dir(view.startup_parent_dir.clone().into());
        window.set_startup_name(view.startup_name.clone().into());
        window.set_startup_width(view.startup_width.clone().into());
        window.set_startup_height(view.startup_height.clone().into());
        window.set_startup_fps(view.startup_fps.clone().into());
        window.set_startup_duration_minutes(view.startup_duration_minutes.clone().into());
        window.set_startup_preview_max_width(view.startup_preview_max_width.clone().into());
        window.set_startup_preview_max_height(view.startup_preview_max_height.clone().into());
        window.set_startup_error(view.startup_error.clone().into());
        window.set_startup_projects(view.startup_projects.clone());
        window.set_startup_selected_project(view.startup_selected_project);
    }

    fn wire_callbacks(&self) {
        Self::wire_action(&self.window, Rc::clone(&self.model), |window, callback| {
            window.on_request_new_project(callback);
        }, AppCommand::ShowStartupModalNew);
        Self::wire_action(&self.window, Rc::clone(&self.model), |window, callback| {
            window.on_request_open_project(callback);
        }, AppCommand::ShowStartupModalOpen);
        Self::wire_action(&self.window, Rc::clone(&self.model), |window, callback| {
            window.on_request_save_project(callback);
        }, AppCommand::SaveProject);
        Self::wire_action(&self.window, Rc::clone(&self.model), |window, callback| {
            window.on_request_open_queue(callback);
        }, AppCommand::OpenQueue);
        Self::wire_action(&self.window, Rc::clone(&self.model), |window, callback| {
            window.on_request_open_preferences(callback);
        }, AppCommand::OpenPreferences);
        Self::wire_action(&self.window, Rc::clone(&self.model), |window, callback| {
            window.on_request_startup_close(callback);
        }, AppCommand::HideStartupModal);
        Self::wire_action(&self.window, Rc::clone(&self.model), |window, callback| {
            window.on_request_startup_browse_parent(callback);
        }, AppCommand::BrowseStartupParentFolder);
        Self::wire_action(&self.window, Rc::clone(&self.model), |window, callback| {
            window.on_request_startup_browse_existing_project(callback);
        }, AppCommand::OpenProjectDialog);
        Self::wire_action(&self.window, Rc::clone(&self.model), |window, callback| {
            window.on_request_startup_open_project(callback);
        }, AppCommand::OpenSelectedStartupProject);
        Self::wire_action(&self.window, Rc::clone(&self.model), |window, callback| {
            window.on_request_startup_create_project(callback);
        }, AppCommand::CreateProjectFromStartup);

        Self::wire_startup_field(&self.window, Rc::clone(&self.model), StartupField::Name);
        Self::wire_startup_field(&self.window, Rc::clone(&self.model), StartupField::Width);
        Self::wire_startup_field(&self.window, Rc::clone(&self.model), StartupField::Height);
        Self::wire_startup_field(&self.window, Rc::clone(&self.model), StartupField::Fps);
        Self::wire_startup_field(
            &self.window,
            Rc::clone(&self.model),
            StartupField::DurationMinutes,
        );
        Self::wire_startup_field(
            &self.window,
            Rc::clone(&self.model),
            StartupField::PreviewMaxWidth,
        );
        Self::wire_startup_field(
            &self.window,
            Rc::clone(&self.model),
            StartupField::PreviewMaxHeight,
        );
        Self::wire_startup_field(&self.window, Rc::clone(&self.model), StartupField::TabIndex);
        Self::wire_startup_field(
            &self.window,
            Rc::clone(&self.model),
            StartupField::SelectedProject,
        );
    }

    fn wire_action(
        window: &MainWindow,
        model: Rc<RefCell<AppModel>>,
        connect: impl Fn(&MainWindow, Box<dyn Fn()>) + 'static,
        command: AppCommand,
    ) {
        let weak = window.as_weak();
        connect(window, Box::new(move || {
            Self::dispatch(&model, &weak, command.clone());
        }));
    }

    fn wire_startup_field(window: &MainWindow, model: Rc<RefCell<AppModel>>, field: StartupField) {
        let weak = window.as_weak();
        match field {
            StartupField::Name => window.on_startup_name_edited(move |value| {
                Self::dispatch(
                    &model,
                    &weak,
                    AppCommand::UpdateStartupField {
                        field: StartupField::Name,
                        value: value.to_string(),
                    },
                );
            }),
            StartupField::Width => window.on_startup_width_edited(move |value| {
                Self::dispatch(
                    &model,
                    &weak,
                    AppCommand::UpdateStartupField {
                        field: StartupField::Width,
                        value: value.to_string(),
                    },
                );
            }),
            StartupField::Height => window.on_startup_height_edited(move |value| {
                Self::dispatch(
                    &model,
                    &weak,
                    AppCommand::UpdateStartupField {
                        field: StartupField::Height,
                        value: value.to_string(),
                    },
                );
            }),
            StartupField::Fps => window.on_startup_fps_edited(move |value| {
                Self::dispatch(
                    &model,
                    &weak,
                    AppCommand::UpdateStartupField {
                        field: StartupField::Fps,
                        value: value.to_string(),
                    },
                );
            }),
            StartupField::DurationMinutes => {
                window.on_startup_duration_minutes_edited(move |value| {
                    Self::dispatch(
                        &model,
                        &weak,
                        AppCommand::UpdateStartupField {
                            field: StartupField::DurationMinutes,
                            value: value.to_string(),
                        },
                    );
                })
            }
            StartupField::PreviewMaxWidth => {
                window.on_startup_preview_max_width_edited(move |value| {
                    Self::dispatch(
                        &model,
                        &weak,
                        AppCommand::UpdateStartupField {
                            field: StartupField::PreviewMaxWidth,
                            value: value.to_string(),
                        },
                    );
                })
            }
            StartupField::PreviewMaxHeight => {
                window.on_startup_preview_max_height_edited(move |value| {
                    Self::dispatch(
                        &model,
                        &weak,
                        AppCommand::UpdateStartupField {
                            field: StartupField::PreviewMaxHeight,
                            value: value.to_string(),
                        },
                    );
                })
            }
            StartupField::TabIndex => window.on_startup_tab_changed(move |value| {
                Self::dispatch(
                    &model,
                    &weak,
                    AppCommand::UpdateStartupField {
                        field: StartupField::TabIndex,
                        value: value.to_string(),
                    },
                );
            }),
            StartupField::SelectedProject => window.on_startup_selected_project_changed(move |value| {
                Self::dispatch(
                    &model,
                    &weak,
                    AppCommand::UpdateStartupField {
                        field: StartupField::SelectedProject,
                        value: value.to_string(),
                    },
                );
            }),
        }
    }

    fn dispatch(model: &Rc<RefCell<AppModel>>, window: &slint::Weak<MainWindow>, command: AppCommand) {
        if let Ok(mut model) = model.try_borrow_mut() {
            Self::handle_command(&mut model, command);
        }

        if let Some(window) = window.upgrade() {
            if let Ok(model) = model.try_borrow() {
                Self::apply_view_model(&window, &AppViewModel::from(&*model));
            }
        }
    }

    fn handle_command(model: &mut AppModel, command: AppCommand) {
        match command {
            AppCommand::ShowStartupModalNew => model.show_startup_modal_new(),
            AppCommand::ShowStartupModalOpen => model.show_startup_modal_open(),
            AppCommand::HideStartupModal => model.hide_startup_modal(),
            AppCommand::UpdateStartupField { field, value } => match field {
                StartupField::Name => model.update_startup_name(value),
                StartupField::Width => model.update_startup_width(value),
                StartupField::Height => model.update_startup_height(value),
                StartupField::Fps => model.update_startup_fps(value),
                StartupField::DurationMinutes => model.update_startup_duration_minutes(value),
                StartupField::PreviewMaxWidth => model.update_startup_preview_max_width(value),
                StartupField::PreviewMaxHeight => model.update_startup_preview_max_height(value),
                StartupField::TabIndex => model.update_startup_tab(value),
                StartupField::SelectedProject => model.update_startup_selected_project(value),
            },
            AppCommand::BrowseStartupParentFolder => {
                let start_dir = model.startup.draft.parent_dir.clone();
                if let Some(folder) = rfd::FileDialog::new().set_directory(start_dir).pick_folder() {
                    model.set_startup_parent_dir(folder);
                }
            }
            AppCommand::CreateProjectFromStartup => {
                if let Err(error) = model.create_project_from_startup() {
                    model.startup.error_message = error;
                }
            }
            AppCommand::OpenProjectDialog => {
                let start_dir = model
                    .project
                    .project_path
                    .clone()
                    .unwrap_or_else(StartupDraft::default_parent_dir);
                if let Some(folder) = rfd::FileDialog::new().set_directory(start_dir).pick_folder() {
                    if let Err(error) = model.open_project(&folder) {
                        model.startup.visible = true;
                        model.startup.error_message = error;
                    }
                }
            }
            AppCommand::OpenSelectedStartupProject => {
                if let Err(error) = model.open_selected_startup_project() {
                    model.startup.error_message = error;
                }
            }
            AppCommand::SaveProject => {
                if let Err(error) = model.save_project() {
                    model.status_message = format!("Save failed: {}", error);
                }
            }
            AppCommand::OpenQueue => {
                model.status_message =
                    "Queue UI has not been ported yet. The command boundary is ready for it."
                        .to_string();
            }
            AppCommand::OpenPreferences => {
                model.status_message =
                    "Preferences UI has not been ported yet. Project startup flow is the active migration path."
                        .to_string();
            }
        }
    }
}
