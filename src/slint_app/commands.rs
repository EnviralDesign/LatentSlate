#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupField {
    Name,
    Width,
    Height,
    Fps,
    DurationMinutes,
    PreviewMaxWidth,
    PreviewMaxHeight,
}

#[derive(Debug, Clone)]
pub enum AppCommand {
    ShowStartupModal,
    HideStartupModal,
    UpdateStartupField { field: StartupField, value: String },
    BrowseStartupParentFolder,
    CreateProjectFromStartup,
    OpenProjectDialog,
    SaveProject,
    OpenQueue,
    OpenPreferences,
}
