mod controller;
mod commands;
mod model;
mod presenter;

pub use model::AppModel;

pub fn run() -> Result<(), slint::PlatformError> {
    controller::SlintAppController::new(AppModel::default())?.run()
}
