#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupField {
    Name,
    Width,
    Height,
    Fps,
    DurationMinutes,
    PreviewMaxWidth,
    PreviewMaxHeight,
    TabIndex,
    SelectedProject,
}

#[derive(Debug, Clone)]
pub enum AppCommand {
    ShowStartupModalNew,
    ShowStartupModalOpen,
    HideStartupModal,
    UpdateStartupField { field: StartupField, value: String },
    BrowseStartupParentFolder,
    CreateProjectFromStartup,
    OpenProjectDialog,
    OpenSelectedStartupProject,
    SaveProject,
    OpenQueue,
    OpenPreferences,
}
