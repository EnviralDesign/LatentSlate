use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Context, Layout, RichText, Ui, Vec2};
use uuid::Uuid;

use crate::state::{ProjectProviderScope, ProjectSettings, ProviderEntry, ProviderOutputType};
use crate::ui_kit as kit;

use super::{
    automation_checkbox, inspector_drag_f64, inspector_drag_i64, inspector_two_drag_f64,
    inspector_two_drag_u32, ExportModalState, LatentSlateApp,
};
const PROJECT_WIZARD_SIZE: [f32; 2] = [760.0, 660.0];
const PROJECT_WIZARD_CARD_H: f32 = 526.0;
const PROJECT_WIZARD_MIN_SIZE: [f32; 2] = [560.0, 500.0];
pub(super) fn project_wizard_size(ctx: &Context) -> Vec2 {
    let available = ctx.content_rect().size();
    let max_w = (available.x - 24.0).max(320.0);
    let max_h = (available.y - 24.0).max(360.0);
    Vec2::new(
        PROJECT_WIZARD_SIZE[0]
            .min(max_w)
            .max(PROJECT_WIZARD_MIN_SIZE[0].min(max_w)),
        PROJECT_WIZARD_SIZE[1]
            .min(max_h)
            .max(PROJECT_WIZARD_MIN_SIZE[1].min(max_h)),
    )
}

impl LatentSlateApp {
    pub(super) fn startup_modal(&mut self, ctx: &Context) {
        let wizard_size = project_wizard_size(ctx);
        kit::modal_scrim(ctx, "startup");
        egui::Window::new("startup")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .fixed_size(wizard_size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                kit::modal_header(ui, "LatentSlate", Some("From latent space to timeline."));
                kit::modal_body(ui, |ui| self.new_project_modal_contents(ui, true));
            });
    }

    pub(super) fn new_project_modal(&mut self, ctx: &Context, startup: bool) {
        let mut open = true;
        let close_enabled = !startup && self.editor.project_root().is_some();
        let mut close_clicked = false;
        let wizard_size = project_wizard_size(ctx);
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "new_project", close_enabled);
        egui::Window::new(if startup {
            "Create Project"
        } else {
            "New Project"
        })
        .title_bar(false)
        .order(egui::Order::Foreground)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(wizard_size)
        .frame(kit::modal_frame())
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            close_clicked = kit::modal_header_with_close(
                ui,
                "New Project",
                Some("Choose project settings and save location."),
                close_enabled,
            );
            kit::modal_body(ui, |ui| self.new_project_modal_contents(ui, startup));
        });
        if close_clicked || outside_clicked || (!open && close_enabled) {
            self.editor.overlays.new_project = false;
        }
    }

    pub(super) fn new_project_modal_contents(&mut self, ui: &mut Ui, _startup: bool) {
        let gap = 10.0;
        let available_w = ui.available_width();
        let card_h = ui.available_height().min(PROJECT_WIZARD_CARD_H).max(360.0);
        let left_w = ((available_w - gap) * 2.0 / 3.0).max(360.0);
        let right_w = (available_w - gap - left_w).max(180.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.allocate_ui_with_layout(
                Vec2::new(left_w, card_h),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(left_w);
                    kit::card_panel(ui, card_h, |ui| self.new_project_create_card(ui));
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(right_w, card_h),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(right_w);
                    kit::card_panel(ui, card_h, |ui| {
                        kit::field_label(ui, "Recent Projects");
                        let recent = recent_projects(&self.new_project_parent);
                        let mut selected_project: Option<PathBuf> = None;
                        let mut browse_clicked = false;
                        kit::body_with_footer(
                            ui,
                            120.0,
                            kit::SECONDARY_BUTTON_H,
                            |ui| {
                                ui.add_space(kit::FORM_ROW_GAP);
                                kit::scroll_body(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                                    if recent.is_empty() {
                                        kit::empty_state(
                                            ui,
                                            "No recent projects",
                                            "Browse to open an existing project folder.",
                                        );
                                    }
                                    for folder in recent {
                                        if kit::secondary_button(
                                            ui,
                                            folder
                                                .file_name()
                                                .and_then(|v| v.to_str())
                                                .unwrap_or("Project"),
                                            ui.available_width(),
                                        )
                                        .clicked()
                                        {
                                            selected_project = Some(folder);
                                        }
                                    }
                                });
                            },
                            |ui| {
                                if kit::secondary_button(
                                    ui,
                                    "Browse for Project...",
                                    ui.available_width(),
                                )
                                .clicked()
                                {
                                    browse_clicked = true;
                                }
                            },
                        );
                        if let Some(folder) = selected_project {
                            if self.open_project_folder(folder) {
                                self.editor.overlays.new_project = false;
                            }
                        } else if browse_clicked {
                            let initial_dir = self.new_project_parent.clone();
                            let options = kit::BrowsePathOptions::new()
                                .id_salt("new_project_open_existing")
                                .initial_dir(initial_dir.as_path())
                                .remember_last_dir();
                            if let Some(folder) = kit::pick_folder_dialog(ui, options) {
                                if self.open_project_folder(folder) {
                                    self.editor.overlays.new_project = false;
                                }
                            }
                        }
                    });
                },
            );
        });
    }

    pub(super) fn new_project_create_card(&mut self, ui: &mut Ui) {
        let footer_h =
            kit::labeled_field_height(kit::VALUE_FIELD_H) + kit::ACTION_GAP + kit::PRIMARY_BUTTON_H;
        let new_project_name = &mut self.new_project_name;
        let project_settings = &mut self.project_settings;
        let new_project_parent = &mut self.new_project_parent;
        let mut create_clicked = false;

        kit::body_with_footer(
            ui,
            180.0,
            footer_h,
            |ui| {
                kit::scroll_body(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                    kit::field_label(ui, "Create New Project");
                    ui.add_space(kit::FORM_ROW_GAP);
                    kit::labeled_text_field(ui, "Project Name", new_project_name);
                    ui.add_space(10.0);
                    settings_fields(ui, project_settings);
                });
            },
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let parent_display = new_project_parent.display().to_string();
                let options = kit::BrowsePathOptions::new()
                    .id_salt("new_project_save_location")
                    .initial_dir(new_project_parent.as_path())
                    .remember_last_dir();
                if let Some(folder) =
                    kit::labeled_browse_folder_field(ui, "Save Location", parent_display, options)
                {
                    *new_project_parent = folder;
                }
                ui.add_space(kit::ACTION_GAP);
                let create_w = ui.available_width();
                if kit::primary_button(ui, "Create Project", create_w).clicked() {
                    create_clicked = true;
                }
            },
        );

        if create_clicked {
            match self.editor.create_project(
                &self.new_project_parent,
                self.new_project_name.trim(),
                self.project_settings.clone(),
            ) {
                Ok(_) => {
                    self.clear_project_runtime_cache();
                    self.export_modal = ExportModalState::for_project(&self.editor.project);
                    self.export_preview_texture = None;
                    self.editor.overlays.new_project = false;
                }
                Err(err) => self.editor.status = err,
            }
        }
    }

    pub(super) fn project_settings_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "project_settings", true);
        let providers = self.editor.provider_entries.clone();
        egui::Window::new("Project Settings")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([700.0, 650.0])
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "Project Settings",
                    Some("Update resolution, timing, preview scale, and project provider scope."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    let body_height =
                        (ui.available_height() - kit::ACTION_GAP - kit::PRIMARY_BUTTON_H)
                            .max(320.0);
                    let column_gap = kit::ACTION_GAP;
                    let column_width = ((ui.available_width() - column_gap) * 0.5).max(260.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = column_gap;
                        ui.allocate_ui_with_layout(
                            Vec2::new(column_width, body_height),
                            Layout::top_down(Align::Min),
                            |ui| {
                                kit::card_panel(ui, body_height, |ui| {
                                    settings_fields(ui, &mut self.project_settings)
                                });
                            },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(column_width, body_height),
                            Layout::top_down(Align::Min),
                            |ui| {
                                kit::card_panel(ui, body_height, |ui| {
                                    provider_scope_fields(
                                        ui,
                                        &mut self.project_settings,
                                        &providers,
                                    )
                                });
                            },
                        );
                    });
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if kit::secondary_button(ui, "Cancel", 120.0).clicked() {
                            self.project_settings = self.editor.project.settings.clone();
                            self.editor.overlays.project_settings = false;
                        }
                        if kit::primary_button(ui, "Save Changes", 180.0).clicked() {
                            self.editor.project.settings = self.project_settings.clone();
                            self.editor.preview_dirty = true;
                            self.editor.overlays.project_settings = false;
                        }
                    });
                });
            });
        if close_clicked || outside_clicked || !open {
            self.editor.overlays.project_settings = false;
        }
    }

    pub(super) fn generative_video_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "generative_video", true);
        egui::Window::new("New Generative Video")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([480.0, 210.0])
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "New Generative Video",
                    Some("Define the target duration for this asset."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    let mut fps = self.gen_video_fps.max(1.0);
                    let mut frames = self.gen_video_frames.max(1) as i64;
                    let mut seconds = frames as f64 / fps;
                    let mut seconds_changed = false;

                    kit::field_grid_row_with_height(
                        ui,
                        &[1.0, 1.0, 1.0],
                        kit::FIELD_H,
                        kit::FORM_ROW_GAP,
                        |ui, index| match index {
                            0 => {
                                let width = ui.available_width();
                                inspector_drag_f64(ui, "FPS", &mut fps, 1.0, width);
                            }
                            1 => {
                                let width = ui.available_width();
                                inspector_drag_i64(ui, "Frames", &mut frames, 1.0, width);
                            }
                            _ => {
                                let width = ui.available_width();
                                seconds_changed =
                                    inspector_drag_f64(ui, "Seconds", &mut seconds, 0.1, width);
                            }
                        },
                    );

                    fps = fps.clamp(1.0, 240.0);
                    if seconds_changed {
                        let min_seconds = 1.0 / fps;
                        frames = (seconds.max(min_seconds) * fps).round().max(1.0) as i64;
                    }
                    self.gen_video_fps = fps;
                    self.gen_video_frames = frames.clamp(1, 1_000_000) as u32;

                    ui.add_space(kit::ACTION_GAP);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if kit::primary_button(ui, "Create", 120.0).clicked() {
                            if let Err(err) = self
                                .editor
                                .create_generative_video(self.gen_video_fps, self.gen_video_frames)
                            {
                                self.editor.status = err;
                            }
                            self.editor.overlays.generative_video = false;
                        }
                        if kit::secondary_button(ui, "Cancel", 100.0).clicked() {
                            self.editor.overlays.generative_video = false;
                        }
                    });
                });
            });
        if close_clicked || outside_clicked || !open {
            self.editor.overlays.generative_video = false;
        }
    }
}

pub(super) fn settings_fields(ui: &mut Ui, settings: &mut ProjectSettings) {
    kit::field_label(ui, "Resolution");
    ui.horizontal_wrapped(|ui| {
        for preset in RESOLUTION_PRESETS {
            let selected = settings.width == preset.width && settings.height == preset.height;
            let color = if selected {
                kit::PRIMARY_HOVER
            } else {
                kit::TEXT_MUTED
            };
            if kit::media_pill(ui, preset.label, color).clicked() {
                settings.width = preset.width;
                settings.height = preset.height;
                settings.preview_max_width = preset.preview_width;
                settings.preview_max_height = preset.preview_height;
            }
        }
    });
    ui.add_space(6.0);
    let _ = inspector_two_drag_u32(
        ui,
        ("W", &mut settings.width, 8.0),
        ("H", &mut settings.height, 8.0),
    );
    ui.add_space(8.0);
    kit::field_label(ui, "Preview Downsample");
    ui.add_space(6.0);
    let _ = inspector_two_drag_u32(
        ui,
        ("W", &mut settings.preview_max_width, 8.0),
        ("H", &mut settings.preview_max_height, 8.0),
    );
    ui.add_space(8.0);
    kit::field_label(ui, "Timing");
    ui.add_space(6.0);
    let mut minutes = settings.duration_seconds / 60.0;
    if inspector_two_drag_f64(
        ui,
        ("FPS", &mut settings.fps, 1.0),
        ("Min", &mut minutes, 0.25),
    ) {
        settings.duration_seconds = (minutes * 60.0).max(1.0);
    }
}

fn provider_scope_fields(ui: &mut Ui, settings: &mut ProjectSettings, providers: &[ProviderEntry]) {
    kit::field_label(ui, "Provider Scope");
    ui.add_space(kit::FORM_ROW_GAP);

    let all_selected = settings.provider_scope.is_all();
    ui.horizontal_wrapped(|ui| {
        let all_color = if all_selected {
            kit::PRIMARY_HOVER
        } else {
            kit::TEXT_MUTED
        };
        if kit::media_pill(ui, "All Providers", all_color).clicked() {
            settings.provider_scope = ProjectProviderScope::All;
        }

        let selected_color = if all_selected {
            kit::TEXT_MUTED
        } else {
            kit::PRIMARY_HOVER
        };
        if kit::media_pill(ui, "Selected Providers", selected_color).clicked() && all_selected {
            settings.provider_scope = ProjectProviderScope::Selected {
                provider_ids: providers.iter().map(|provider| provider.id).collect(),
            };
        }
    });

    ui.add_space(kit::FORM_ROW_GAP);
    let selected_count = match &settings.provider_scope {
        ProjectProviderScope::All => providers.len(),
        ProjectProviderScope::Selected { provider_ids } => providers
            .iter()
            .filter(|provider| provider_ids.contains(&provider.id))
            .count(),
    };
    ui.label(kit::caption(format!(
        "{} of {} installed providers visible to this project and the Agent API.",
        selected_count,
        providers.len()
    )));

    let ProjectProviderScope::Selected { provider_ids } = &settings.provider_scope else {
        return;
    };

    ui.add_space(kit::ACTION_GAP);
    let mut next_ids = dedup_provider_ids(provider_ids.iter().copied());
    ui.horizontal(|ui| {
        if kit::secondary_button(ui, "Select All", 96.0).clicked() {
            next_ids = providers.iter().map(|provider| provider.id).collect();
        }
        if kit::secondary_button(ui, "Clear", 72.0).clicked() {
            next_ids.clear();
        }
    });
    ui.add_space(kit::FORM_ROW_GAP);

    if providers.is_empty() {
        ui.label(kit::caption("No providers installed yet."));
    } else {
        let list_height = ui.available_height().max(96.0);
        egui::ScrollArea::vertical()
            .id_salt("project_provider_scope_list")
            .max_height(list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for provider in providers {
                    let mut enabled = next_ids.contains(&provider.id);
                    provider_scope_row(ui, provider, &mut enabled);
                    if enabled {
                        if !next_ids.contains(&provider.id) {
                            next_ids.push(provider.id);
                        }
                    } else {
                        next_ids.retain(|id| *id != provider.id);
                    }
                    ui.add_space(6.0);
                }
            });
    }

    let stale_count = next_ids
        .iter()
        .filter(|id| !providers.iter().any(|provider| provider.id == **id))
        .count();
    if stale_count > 0 {
        ui.add_space(kit::FORM_ROW_GAP);
        ui.label(
            RichText::new(format!(
                "{stale_count} selected provider ID(s) are not installed right now."
            ))
            .color(kit::MARKER)
            .size(11.0),
        );
    }

    settings.provider_scope = ProjectProviderScope::Selected {
        provider_ids: dedup_provider_ids(next_ids),
    };
}

fn provider_scope_row(ui: &mut Ui, provider: &ProviderEntry, enabled: &mut bool) {
    ui.horizontal(|ui| {
        let label = format!("Enable {}", provider.name);
        let _ = automation_checkbox(ui, enabled, "");
        let badge_w = 58.0;
        let gap = kit::FIELD_COMPOUND_GAP * 2.0;
        let name_w = (ui.available_width() - badge_w - gap).max(90.0);
        ui.add_sized(
            [name_w, kit::FIELD_H],
            egui::Label::new(kit::body(provider.name.clone())).truncate(),
        )
        .on_hover_text(label);
        kit::media_pill_sized(
            ui,
            provider_output_label(provider),
            kit::TEXT_MUTED,
            badge_w,
        );
    });
}

fn provider_output_label(provider: &ProviderEntry) -> &'static str {
    match provider.output_type {
        ProviderOutputType::Image => "Image",
        ProviderOutputType::Video => "Video",
        ProviderOutputType::Audio => "Audio",
    }
}

fn dedup_provider_ids(ids: impl IntoIterator<Item = Uuid>) -> Vec<Uuid> {
    let mut deduped = Vec::new();
    for id in ids {
        if !deduped.contains(&id) {
            deduped.push(id);
        }
    }
    deduped
}

struct ResolutionPreset {
    label: &'static str,
    width: u32,
    height: u32,
    preview_width: u32,
    preview_height: u32,
}

const RESOLUTION_PRESETS: &[ResolutionPreset] = &[
    ResolutionPreset {
        label: "1080p",
        width: 1920,
        height: 1080,
        preview_width: 960,
        preview_height: 540,
    },
    ResolutionPreset {
        label: "4K",
        width: 3840,
        height: 2160,
        preview_width: 1280,
        preview_height: 720,
    },
    ResolutionPreset {
        label: "9:16",
        width: 1080,
        height: 1920,
        preview_width: 540,
        preview_height: 960,
    },
    ResolutionPreset {
        label: "1:1",
        width: 1024,
        height: 1024,
        preview_width: 512,
        preview_height: 512,
    },
];

pub(super) fn recent_projects(parent: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(parent)
        .ok()
        .into_iter()
        .flat_map(|read_dir| read_dir.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.join("project.json").exists())
        .take(8)
        .collect()
}
