mod controller;
mod model;

pub use model::AppShellModel;

pub fn run() -> Result<(), slint::PlatformError> {
    controller::SlintAppController::new(AppShellModel::default())?.run()
}
