use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, ColorImage, Context, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke,
    TextureHandle, TextureOptions, Ui, Vec2,
};
use uuid::Uuid;

use crate::core::preview::{PreviewDecodeMode, PreviewFrameInfo, PreviewStats};
use crate::core::preview_store;
use crate::editor::{
    default_generative_video_fps, default_generative_video_frames, default_projects_dir,
    generative_video_duration_label, EditorState,
};
use crate::state::{
    Asset, AssetKind, Clip, ClipTransform, ProjectSettings, ProviderEntry, TrackType,
};
use crate::ui_kit as kit;

const PROJECT_WIZARD_SIZE: [f32; 2] = [760.0, 660.0];
const PROJECT_WIZARD_CARD_H: f32 = 526.0;
const PROJECT_WIZARD_MIN_SIZE: [f32; 2] = [560.0, 500.0];

fn project_wizard_size(ctx: &Context) -> Vec2 {
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

pub fn run() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NLA AI Video Creator")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 620.0]),
        ..Default::default()
    };

    eframe::run_native(
        "NLA AI Video Creator",
        native_options,
        Box::new(|cc| Ok(Box::new(NlaEguiApp::new(cc)))),
    )
}

pub struct NlaEguiApp {
    editor: EditorState,
    preview_texture: Option<TextureHandle>,
    preview_frame: Option<PreviewFrameInfo>,
    preview_stats: Option<PreviewStats>,
    last_tick: Instant,
    new_project_name: String,
    new_project_parent: PathBuf,
    project_settings: ProjectSettings,
    gen_video_fps: f64,
    gen_video_frames: u32,
    selected_provider_file: Option<PathBuf>,
}

impl NlaEguiApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        kit::configure_style(&cc.egui_ctx);
        let editor = EditorState::new();
        Self {
            project_settings: editor.project.settings.clone(),
            editor,
            preview_texture: None,
            preview_frame: None,
            preview_stats: None,
            last_tick: Instant::now(),
            new_project_name: "My New Project".to_string(),
            new_project_parent: default_projects_dir(),
            gen_video_fps: default_generative_video_fps(),
            gen_video_frames: default_generative_video_frames(),
            selected_provider_file: None,
        }
    }

    fn poll_automation(&mut self) {
        if !crate::core::automation::is_enabled() {
            return;
        }
        while let Some(envelope) = crate::core::automation::try_recv_command() {
            let response = self.editor.apply_automation_command(&envelope.command);
            self.project_settings = self.editor.project.settings.clone();
            envelope.respond(response);
        }
    }

    fn keep_automation_responsive(&self, ctx: &Context) {
        if crate::core::automation::is_enabled() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn tick_playback(&mut self, ctx: &Context) {
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        if !self.editor.is_playing {
            return;
        }
        let next = self.editor.current_time + delta;
        let duration = self.editor.project.duration();
        if next >= duration {
            self.editor.current_time = duration;
            self.editor.is_playing = false;
        } else {
            self.editor.seek(next);
        }
        ctx.request_repaint();
    }

    fn update_preview_texture(&mut self, ctx: &Context) {
        if !self.editor.preview_dirty && self.preview_texture.is_some() {
            return;
        }
        if self.editor.project.project_path.is_none() {
            self.preview_texture = None;
            self.preview_frame = None;
            return;
        }

        let output = self.editor.previewer.render_frame(
            &self.editor.project,
            self.editor.current_time,
            PreviewDecodeMode::Seek,
            self.editor.layout.hardware_decode,
        );
        self.preview_stats = Some(output.stats);
        let Some(frame) = output.frame else {
            self.preview_texture = None;
            self.preview_frame = None;
            self.editor.preview_dirty = false;
            return;
        };
        let Some(bytes) = preview_store::get_preview_bytes(frame.version) else {
            return;
        };
        let image = ColorImage::from_rgba_unmultiplied(
            [frame.width as usize, frame.height as usize],
            &bytes,
        );
        if let Some(texture) = self.preview_texture.as_mut() {
            texture.set(image, TextureOptions::LINEAR);
        } else {
            self.preview_texture =
                Some(ctx.load_texture("preview-frame", image, TextureOptions::LINEAR));
        }
        self.preview_frame = Some(frame);
        self.editor.preview_dirty = false;
    }

    fn top_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("top_bar")
            .exact_height(kit::TOP_BAR_H)
            .frame(kit::chrome_frame())
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    menu_button(
                        ui,
                        "File",
                        |ui, this: &mut Self| {
                            if ui.button("New Project...").clicked() {
                                this.editor.overlays.new_project = true;
                                ui.close();
                            }
                            if ui.button("Open Project...").clicked() {
                                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                    if let Err(err) = this.editor.open_project(folder) {
                                        this.editor.status = err;
                                    }
                                }
                                ui.close();
                            }
                            ui.add_enabled_ui(this.editor.project.project_path.is_some(), |ui| {
                                if ui.button("Project Settings...").clicked() {
                                    this.project_settings = this.editor.project.settings.clone();
                                    this.editor.overlays.project_settings = true;
                                    ui.close();
                                }
                                if ui.button("Save").clicked() {
                                    if let Err(err) = this.editor.save() {
                                        this.editor.status = err;
                                    }
                                    ui.close();
                                }
                            });
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "Edit",
                        |ui, this: &mut Self| {
                            if ui.button("Add Marker").clicked() {
                                this.editor.add_marker(None);
                                ui.close();
                            }
                            if ui.button("Create Generative Video...").clicked() {
                                this.editor.overlays.generative_video = true;
                                ui.close();
                            }
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "View",
                        |ui, this: &mut Self| {
                            ui.checkbox(&mut this.editor.layout.preview_stats, "Preview Stats");
                            ui.checkbox(&mut this.editor.layout.left_collapsed, "Collapse Assets");
                            ui.checkbox(
                                &mut this.editor.layout.right_collapsed,
                                "Collapse Attributes",
                            );
                            ui.checkbox(
                                &mut this.editor.layout.timeline_collapsed,
                                "Collapse Timeline",
                            );
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "Settings",
                        |ui, this: &mut Self| {
                            if ui.button("AI Providers...").clicked() {
                                this.editor.refresh_providers();
                                this.editor.overlays.providers = true;
                                ui.close();
                            }
                            ui.checkbox(&mut this.editor.layout.hardware_decode, "Hardware Decode");
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "Help",
                        |ui, this: &mut Self| {
                            ui.label(RichText::new("NLA AI Video Creator").strong());
                            ui.label(
                                RichText::new("egui migration build")
                                    .small()
                                    .color(kit::TEXT_MUTED),
                            );
                            if ui.button("Open Harness Docs").clicked() {
                                this.editor.status = "See docs/DESKTOP_TEST_HARNESS.md".to_string();
                                ui.close();
                            }
                        },
                        self,
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let running =
                            self.editor.generation_queue.iter().any(|job| {
                                job.status == crate::state::GenerationJobStatus::Running
                            });
                        let queue_text = if self.editor.generation_queue.is_empty() {
                            "QUE".to_string()
                        } else {
                            format!("QUE {}", self.editor.generation_queue.len())
                        };
                        if ui
                            .add(
                                egui::Button::new(queue_text)
                                    .selected(self.editor.overlays.queue || running),
                            )
                            .clicked()
                        {
                            self.editor.overlays.queue = !self.editor.overlays.queue;
                        }
                        ui.label(
                            RichText::new(self.editor.project_name())
                                .color(kit::TEXT_MUTED)
                                .size(12.0),
                        );
                    });
                });
            });
    }

    fn left_panel(&mut self, ctx: &Context) {
        if self.editor.layout.left_collapsed {
            egui::SidePanel::left("assets_collapsed")
                .exact_width(36.0)
                .frame(kit::dock_frame())
                .show(ctx, |ui| {
                    if kit::collapsed_rail(ui, "ASSETS", kit::VIDEO).clicked() {
                        self.editor.layout.left_collapsed = false;
                    }
                });
            return;
        }

        egui::SidePanel::left("assets")
            .resizable(true)
            .default_width(self.editor.layout.left_width)
            .width_range(180.0..=420.0)
            .frame(kit::dock_frame())
            .show(ctx, |ui| self.assets_panel(ui));
    }

    fn assets_panel(&mut self, ui: &mut Ui) {
        kit::panel_header(ui, "ASSETS", Some("◀"), || {
            self.editor.layout.left_collapsed = true;
        });
        ui.add_space(8.0);
        if kit::secondary_button(ui, "Import Files...", ui.available_width()).clicked() {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                match self.editor.import_asset(path) {
                    Ok(asset_id) => self.editor.selection.asset_ids = vec![asset_id],
                    Err(err) => self.editor.status = err,
                }
            }
        }

        ui.add_space(12.0);
        kit::card_frame().show(ui, |ui| {
            kit::field_label(ui, "New Generative");
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if kit::media_pill(ui, "Video", kit::VIDEO).clicked() {
                    self.editor.overlays.generative_video = true;
                }
                if kit::media_pill(ui, "Image", kit::IMAGE).clicked() {
                    if let Err(err) = self.editor.create_generative_image() {
                        self.editor.status = err;
                    }
                }
                if kit::media_pill(ui, "Audio", kit::AUDIO).clicked() {
                    if let Err(err) = self.editor.create_generative_audio() {
                        self.editor.status = err;
                    }
                }
            });
        });

        ui.add_space(14.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let assets: Vec<Asset> = self.editor.project.assets.clone();
            for asset in assets {
                let selected = self.editor.selection.asset_ids.contains(&asset.id);
                let response = asset_row(ui, &asset, selected);
                if response.clicked() {
                    self.editor.selection.clear();
                    self.editor.selection.asset_ids.push(asset.id);
                }
                response.context_menu(|ui| {
                    if ui.button("Add to timeline").clicked() {
                        if let Err(err) = self.editor.add_asset_to_timeline(asset.id, None) {
                            self.editor.status = err;
                        }
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        self.editor.project.remove_asset(asset.id);
                        self.editor.selection.clear();
                        self.editor.preview_dirty = true;
                        ui.close();
                    }
                });
            }
        });
    }

    fn right_panel(&mut self, ctx: &Context) {
        if self.editor.layout.right_collapsed {
            egui::SidePanel::right("attributes_collapsed")
                .exact_width(36.0)
                .frame(kit::dock_frame())
                .show(ctx, |ui| {
                    if kit::collapsed_rail(ui, "ATTR", kit::AUDIO).clicked() {
                        self.editor.layout.right_collapsed = false;
                    }
                });
            return;
        }

        egui::SidePanel::right("attributes")
            .resizable(true)
            .default_width(self.editor.layout.right_width)
            .width_range(200.0..=440.0)
            .frame(kit::dock_frame())
            .show(ctx, |ui| self.attributes_panel(ui));
    }

    fn attributes_panel(&mut self, ui: &mut Ui) {
        kit::panel_header(ui, "ATTRIBUTES", Some("▶"), || {
            self.editor.layout.right_collapsed = true;
        });
        ui.add_space(8.0);

        if let Some(clip_id) = self.editor.selected_clip_id() {
            self.clip_attributes(ui, clip_id);
        } else if let Some(asset_id) = self.editor.selected_asset_id() {
            self.asset_attributes(ui, asset_id);
        } else if let Some(marker_id) = self.editor.selected_marker_id() {
            self.marker_attributes(ui, marker_id);
        } else if let Some(track_id) = self.editor.selected_track_id() {
            self.track_attributes(ui, track_id);
        } else {
            kit::sunken_frame().show(ui, |ui| {
                kit::empty_state(
                    ui,
                    "Nothing selected",
                    "Select a clip, asset, marker, or track.",
                );
            });
        }
    }

    fn clip_attributes(&mut self, ui: &mut Ui, clip_id: Uuid) {
        let asset_name = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .and_then(|clip| self.editor.project.find_asset(clip.asset_id))
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| "Unknown asset".to_string());
        let mut preview_dirty = false;
        kit::card_frame().show(ui, |ui| {
            kit::field_label(ui, "Clip");
            ui.add_sized(
                [ui.available_width(), 18.0],
                egui::Label::new(kit::value(asset_name)).truncate(),
            );
            ui.add_space(12.0);
            if let Some(clip) = self
                .editor
                .project
                .clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
            {
                let mut label = clip.label.clone().unwrap_or_default();
                if inspector_text_field(ui, "Clip Name", &mut label) {
                    clip.label = if label.trim().is_empty() {
                        None
                    } else {
                        Some(label)
                    };
                }
                ui.add_space(12.0);
                transform_editor(ui, &mut clip.transform, &mut preview_dirty);
                ui.add_space(12.0);
                kit::field_label(ui, "Timing");
                ui.add_space(6.0);
                preview_dirty |= inspector_two_drag_f64(
                    ui,
                    ("Start", &mut clip.start_time, 0.05),
                    ("Duration", &mut clip.duration, 0.05),
                );
            }
        });
        if preview_dirty {
            self.editor.preview_dirty = true;
        }
    }

    fn asset_attributes(&mut self, ui: &mut Ui, asset_id: Uuid) {
        let mut add_to_timeline = false;
        if let Some(asset) = self
            .editor
            .project
            .assets
            .iter_mut()
            .find(|asset| asset.id == asset_id)
        {
            let kind_label = asset_kind_label(&asset.kind).to_string();
            let duration = asset.duration_seconds;
            kit::card_frame().show(ui, |ui| {
                kit::field_label(ui, "Asset");
                kit::singleline_text_field(ui, &mut asset.name, ui.available_width());
                ui.add_space(8.0);
                ui.label(kit::caption(kind_label));
                if let Some(duration) = duration {
                    ui.label(kit::body(format!("Duration: {duration:.2}s")));
                }
                ui.add_space(10.0);
                if kit::secondary_button(ui, "Add to timeline", ui.available_width()).clicked() {
                    add_to_timeline = true;
                }
            });
        }
        if add_to_timeline {
            if let Err(err) = self.editor.add_asset_to_timeline(asset_id, None) {
                self.editor.status = err;
            }
        }
    }

    fn marker_attributes(&mut self, ui: &mut Ui, marker_id: Uuid) {
        let mut should_sort = false;
        let mut delete_marker = false;
        if let Some(marker) = self
            .editor
            .project
            .markers
            .iter_mut()
            .find(|marker| marker.id == marker_id)
        {
            kit::card_frame().show(ui, |ui| {
                kit::field_label(ui, "Marker");
                let mut changed = false;
                ui.add_space(6.0);
                changed |= inspector_drag_f64(ui, "Time", &mut marker.time, 0.05, 104.0);
                ui.add_space(10.0);
                let mut label = marker.label.clone().unwrap_or_default();
                if inspector_text_field(ui, "Label", &mut label) {
                    marker.label = if label.trim().is_empty() {
                        None
                    } else {
                        Some(label)
                    };
                }
                if changed {
                    should_sort = true;
                }
                ui.add_space(12.0);
                if kit::danger_button(ui, "Delete Marker", ui.available_width()).clicked() {
                    delete_marker = true;
                }
            });
        }
        if delete_marker {
            self.editor.project.remove_marker(marker_id);
            self.editor.selection.clear();
            self.editor.preview_dirty = true;
            return;
        }
        if should_sort {
            self.editor
                .project
                .markers
                .sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
            self.editor.preview_dirty = true;
        }
    }

    fn track_attributes(&mut self, ui: &mut Ui, track_id: Uuid) {
        if let Some(track) = self
            .editor
            .project
            .tracks
            .iter_mut()
            .find(|track| track.id == track_id)
        {
            kit::card_frame().show(ui, |ui| {
                kit::field_label(ui, "Track");
                kit::singleline_text_field(ui, &mut track.name, ui.available_width());
                ui.label(kit::caption(format!("{:?}", track.track_type)));
                if track.track_type != TrackType::Marker {
                    ui.add_space(10.0);
                    let _ = inspector_drag_f32(ui, "Volume", &mut track.volume, 0.01, 104.0);
                }
            });
        }
    }

    fn central_preview(&mut self, ctx: &Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(kit::PANEL_SUNKEN))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(kit::section_label("Preview"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let s = &self.editor.project.settings;
                        ui.label(kit::caption(format!(
                            "{} x {} @ {:.0}",
                            s.width, s.height, s.fps
                        )));
                    });
                });
                ui.separator();
                let available = ui.available_size();
                let preview_height = available.y.max(160.0);
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(available.x, preview_height), Sense::hover());
                self.paint_preview(ui, rect);
            });
    }

    fn paint_preview(&mut self, ui: &mut Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
        ui.painter().rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, kit::BORDER),
            egui::StrokeKind::Inside,
        );
        if let (Some(texture), Some(frame)) = (&self.preview_texture, self.preview_frame) {
            let scale = (rect.width() / frame.width as f32)
                .min(rect.height() / frame.height as f32)
                .max(0.01);
            let size = Vec2::new(frame.width as f32 * scale, frame.height as f32 * scale);
            let image_rect = Rect::from_center_size(rect.center(), size);
            ui.painter().image(
                texture.id(),
                image_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No preview frame",
                FontId::proportional(14.0),
                kit::TEXT_DIM,
            );
        }

        if self.editor.layout.preview_stats {
            if let Some(stats) = &self.preview_stats {
                let text = format!(
                    "total {:.1}ms\nscan {:.1}ms\ncomp {:.1}ms\nstill {:.1}ms\nhit {}\nmiss {}\nlayers {}",
                    stats.total_ms,
                    stats.collect_ms,
                    stats.composite_ms,
                    stats.still_load_ms,
                    stats.cache_hits,
                    stats.cache_misses,
                    stats.layers,
                );
                let stats_rect = Rect::from_min_size(
                    rect.right_top() + Vec2::new(-188.0, 12.0),
                    Vec2::new(172.0, 106.0),
                );
                ui.painter().rect_filled(
                    stats_rect,
                    6.0,
                    Color32::from_rgba_unmultiplied(13, 14, 16, 220),
                );
                ui.painter().rect_stroke(
                    stats_rect,
                    6.0,
                    Stroke::new(1.0, kit::BORDER_SOFT),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    stats_rect.min + Vec2::new(10.0, 8.0),
                    egui::Align2::LEFT_TOP,
                    text,
                    FontId::monospace(11.0),
                    kit::TEXT_MUTED,
                );
            }
        }
    }

    fn timeline_panel(&mut self, ctx: &Context) {
        if self.editor.layout.timeline_collapsed {
            egui::TopBottomPanel::bottom("timeline")
                .exact_height(34.0)
                .frame(kit::timeline_frame())
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if kit::secondary_button(ui, "TIMELINE", 94.0).clicked() {
                            self.editor.layout.timeline_collapsed = false;
                        }
                        ui.label(kit::caption(timecode(self.editor.current_time)));
                    });
                });
            return;
        }

        let response = egui::TopBottomPanel::bottom("timeline")
            .resizable(true)
            .default_height(self.editor.layout.timeline_height)
            .height_range(150.0..=420.0)
            .frame(kit::timeline_frame())
            .show(ctx, |ui| {
                ui.set_min_height(150.0);
                self.timeline_header(ui);
                self.paint_timeline(ui);
            });
        self.editor.layout.timeline_height = response.response.rect.height().clamp(150.0, 420.0);
    }

    fn timeline_header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(kit::section_label("Timeline"));
            if kit::icon_button(ui, "—").clicked() {
                self.editor.layout.timeline_collapsed = true;
            }
            ui.separator();
            if kit::icon_button(ui, if self.editor.is_playing { "II" } else { "▶" }).clicked() {
                self.editor.is_playing = !self.editor.is_playing;
            }
            if kit::icon_button(ui, "‹").clicked() {
                self.editor.seek((self.editor.current_time - 1.0).max(0.0));
            }
            if kit::icon_button(ui, "›").clicked() {
                self.editor.seek(self.editor.current_time + 1.0);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(timecode(self.editor.current_time))
                        .monospace()
                        .color(kit::TEXT_MUTED)
                        .size(11.0),
                );
            });
        });
        ui.separator();
    }

    fn paint_timeline(&mut self, ui: &mut Ui) {
        let label_width = 140.0;
        let row_h = 36.0;
        let ruler_h = 24.0;
        let duration = self.editor.project.duration().max(10.0);
        let min_h = ruler_h + self.editor.project.tracks.len() as f32 * row_h + 42.0;
        let total_h = ui.available_height().max(min_h);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), total_h),
            Sense::click_and_drag(),
        );
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let painter = ui.painter_at(rect);
        let timeline_rect = Rect::from_min_max(
            Pos2::new(rect.left() + label_width, rect.top() + ruler_h),
            rect.right_bottom(),
        );
        painter.rect_filled(rect, 0.0, Color32::from_rgb(12, 13, 15));
        painter.line_segment(
            [
                Pos2::new(rect.left() + label_width, rect.top()),
                Pos2::new(rect.left() + label_width, rect.bottom()),
            ],
            Stroke::new(1.0, kit::BORDER),
        );

        for i in 0..=6 {
            let t = duration * i as f64 / 6.0;
            let x = timeline_rect.left() + timeline_rect.width() * i as f32 / 6.0;
            painter.line_segment(
                [
                    Pos2::new(x, rect.top() + ruler_h - 6.0),
                    Pos2::new(x, rect.bottom()),
                ],
                Stroke::new(1.0, Color32::from_rgb(31, 33, 38)),
            );
            painter.text(
                Pos2::new(x + 4.0, rect.top() + 4.0),
                egui::Align2::LEFT_TOP,
                timecode(t),
                FontId::monospace(10.0),
                kit::TEXT_MUTED,
            );
        }

        for (row, track) in self.editor.project.tracks.iter().enumerate() {
            let y = timeline_rect.top() + row as f32 * row_h;
            let row_rect = Rect::from_min_max(
                Pos2::new(rect.left(), y),
                Pos2::new(rect.right(), y + row_h),
            );
            let selected = self.editor.selection.track_ids.contains(&track.id);
            let label_rect = Rect::from_min_max(
                row_rect.left_top(),
                Pos2::new(rect.left() + label_width, row_rect.bottom()),
            );
            painter.rect_filled(
                label_rect,
                0.0,
                if selected {
                    Color32::from_rgb(28, 51, 40)
                } else {
                    Color32::from_rgb(18, 19, 22)
                },
            );
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(rect.left() + label_width, row_rect.top()),
                    row_rect.right_bottom(),
                ),
                0.0,
                if row % 2 == 0 {
                    Color32::from_rgb(14, 15, 17)
                } else {
                    Color32::from_rgb(11, 12, 14)
                },
            );
            painter.line_segment(
                [
                    Pos2::new(rect.left(), row_rect.bottom()),
                    Pos2::new(rect.right(), row_rect.bottom()),
                ],
                Stroke::new(1.0, kit::BORDER_SOFT),
            );
            let color = match track.track_type {
                TrackType::Video => kit::VIDEO,
                TrackType::Audio => kit::AUDIO,
                TrackType::Marker => kit::MARKER,
            };
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(rect.left() + 10.0, y + 10.0),
                    Vec2::new(3.0, 16.0),
                ),
                1.0,
                color,
            );
            painter.text(
                Pos2::new(rect.left() + 24.0, y + 10.0),
                egui::Align2::LEFT_TOP,
                &track.name,
                FontId::proportional(12.5),
                kit::TEXT,
            );
        }

        let clips = self.editor.project.clips.clone();
        for clip in clips {
            if let Some(track_index) = self
                .editor
                .project
                .tracks
                .iter()
                .position(|track| track.id == clip.track_id)
            {
                let clip_rect = self.clip_rect(&clip, timeline_rect, duration, track_index, row_h);
                let selected = self.editor.selection.clip_ids.contains(&clip.id);
                painter.rect_filled(clip_rect, 4.0, Color32::from_rgb(19, 146, 94));
                painter.rect_stroke(
                    clip_rect,
                    4.0,
                    Stroke::new(
                        if selected { 2.0 } else { 1.0 },
                        if selected {
                            kit::BORDER_FOCUS
                        } else {
                            Color32::from_rgb(45, 194, 121)
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                let label = self
                    .editor
                    .project
                    .find_asset(clip.asset_id)
                    .map(|asset| asset.name.as_str())
                    .unwrap_or("clip");
                painter.text(
                    clip_rect.left_center() + Vec2::new(8.0, -7.0),
                    egui::Align2::LEFT_TOP,
                    label,
                    FontId::proportional(11.0),
                    Color32::WHITE,
                );
            }
        }

        if let Some(marker_row) = self
            .editor
            .project
            .tracks
            .iter()
            .position(|track| track.track_type == TrackType::Marker)
        {
            for marker in self.editor.project.markers.iter() {
                let x = timeline_rect.left()
                    + (marker.time as f32 / duration as f32) * timeline_rect.width();
                let y = timeline_rect.top() + marker_row as f32 * row_h;
                painter.line_segment(
                    [Pos2::new(x, y + 4.0), Pos2::new(x, y + row_h - 4.0)],
                    Stroke::new(2.0, kit::MARKER),
                );
                painter.circle_filled(Pos2::new(x, y + 8.0), 4.0, kit::MARKER);
            }
        }

        let playhead_x = timeline_rect.left()
            + (self.editor.current_time as f32 / duration as f32) * timeline_rect.width();
        painter.line_segment(
            [
                Pos2::new(playhead_x, rect.top() + ruler_h - 2.0),
                Pos2::new(playhead_x, rect.bottom()),
            ],
            Stroke::new(2.0, kit::PLAYHEAD),
        );
        painter.circle_filled(
            Pos2::new(playhead_x, rect.top() + ruler_h - 2.0),
            5.0,
            kit::PLAYHEAD,
        );

        painter.text(
            Pos2::new(rect.left() + 14.0, rect.bottom() - 26.0),
            egui::Align2::LEFT_TOP,
            "+ Video    + Audio",
            FontId::proportional(11.0),
            kit::TEXT_DIM,
        );

        if let Some(pos) = response.interact_pointer_pos() {
            if response.dragged() || response.drag_started() {
                self.scrub_timeline(pos, timeline_rect, duration);
            } else if response.clicked() {
                self.handle_timeline_click(pos, timeline_rect, duration, row_h);
            }
        }
    }

    fn clip_rect(
        &self,
        clip: &Clip,
        timeline_rect: Rect,
        duration: f64,
        row: usize,
        row_h: f32,
    ) -> Rect {
        let x1 = timeline_rect.left()
            + (clip.start_time as f32 / duration as f32) * timeline_rect.width();
        let x2 = timeline_rect.left()
            + (clip.end_time() as f32 / duration as f32) * timeline_rect.width();
        let y = timeline_rect.top() + row as f32 * row_h + 6.0;
        Rect::from_min_max(
            Pos2::new(x1, y),
            Pos2::new(x2.max(x1 + 46.0), y + row_h - 12.0),
        )
    }

    fn handle_timeline_click(&mut self, pos: Pos2, timeline_rect: Rect, duration: f64, row_h: f32) {
        if pos.x < timeline_rect.left() {
            let row = ((pos.y - timeline_rect.top()) / row_h).floor().max(0.0) as usize;
            if let Some(track) = self.editor.project.tracks.get(row) {
                self.editor.selection.select_track(track.id);
            }
            return;
        }
        let time = ((pos.x - timeline_rect.left()) / timeline_rect.width()).clamp(0.0, 1.0) as f64
            * duration;
        for clip in self.editor.project.clips.clone() {
            if let Some(track_index) = self
                .editor
                .project
                .tracks
                .iter()
                .position(|track| track.id == clip.track_id)
            {
                let rect = self.clip_rect(&clip, timeline_rect, duration, track_index, row_h);
                if rect.contains(pos) {
                    self.editor.selection.select_clip(clip.id);
                    return;
                }
            }
        }
        self.editor.seek(time);
    }

    fn scrub_timeline(&mut self, pos: Pos2, timeline_rect: Rect, duration: f64) {
        if pos.x < timeline_rect.left() {
            return;
        }
        let time = ((pos.x - timeline_rect.left()) / timeline_rect.width()).clamp(0.0, 1.0) as f64
            * duration;
        self.editor.seek(time);
    }

    fn modals(&mut self, ctx: &Context) {
        let startup_open = self.editor.show_startup();
        if startup_open {
            self.startup_modal(ctx);
        }
        if self.editor.overlays.new_project {
            self.new_project_modal(ctx, false);
        }
        if self.editor.overlays.project_settings {
            self.project_settings_modal(ctx);
        }
        if self.editor.overlays.generative_video {
            self.generative_video_modal(ctx);
        }
        if self.editor.overlays.queue {
            self.queue_panel(ctx);
        }
        if self.editor.overlays.providers {
            self.providers_modal(ctx);
        }
    }

    fn startup_modal(&mut self, ctx: &Context) {
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
                kit::modal_header(
                    ui,
                    "NLA AI Video Creator",
                    Some("Create a new project or open an existing one"),
                );
                kit::modal_body(ui, |ui| self.new_project_modal_contents(ui, true));
            });
    }

    fn new_project_modal(&mut self, ctx: &Context, startup: bool) {
        let mut open = true;
        let close_enabled = !startup && self.editor.project_root().is_some();
        let mut close_clicked = false;
        let wizard_size = project_wizard_size(ctx);
        kit::modal_scrim(ctx, "new_project");
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
        if close_clicked || (!open && close_enabled) {
            self.editor.overlays.new_project = false;
        }
    }

    fn new_project_modal_contents(&mut self, ui: &mut Ui, _startup: bool) {
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
                        ui.add_space(8.0);
                        let recent = recent_projects(&self.new_project_parent);
                        let list_height = (ui.available_height() - 48.0).max(120.0);
                        egui::ScrollArea::vertical()
                            .max_height(list_height)
                            .show(ui, |ui| {
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
                                        match self.editor.open_project(folder) {
                                            Ok(_) => self.editor.overlays.new_project = false,
                                            Err(err) => self.editor.status = err,
                                        }
                                    }
                                }
                            });
                        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                            if kit::secondary_button(
                                ui,
                                "Browse for Project...",
                                ui.available_width(),
                            )
                            .clicked()
                            {
                                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                    match self.editor.open_project(folder) {
                                        Ok(_) => self.editor.overlays.new_project = false,
                                        Err(err) => self.editor.status = err,
                                    }
                                }
                            }
                        });
                    });
                },
            );
        });
    }

    fn new_project_create_card(&mut self, ui: &mut Ui) {
        let content_rect = ui.available_rect_before_wrap();
        let footer_gap = 12.0;
        let footer_h = 98.0_f32.min(content_rect.height());
        let footer_top = content_rect.bottom() - footer_h;
        let form_bottom = (footer_top - footer_gap).max(content_rect.top());
        let form_rect = Rect::from_min_max(
            content_rect.left_top(),
            Pos2::new(content_rect.right(), form_bottom),
        );
        let footer_rect = Rect::from_min_max(
            Pos2::new(content_rect.left(), footer_top),
            content_rect.right_bottom(),
        );

        if form_rect.height() > 24.0 {
            let mut form_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(form_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(form_rect.height())
                .show(&mut form_ui, |ui| {
                    kit::field_label(ui, "Create New Project");
                    ui.add_space(8.0);
                    kit::field_label(ui, "Project Name");
                    kit::singleline_text_field(
                        ui,
                        &mut self.new_project_name,
                        ui.available_width(),
                    );
                    ui.add_space(10.0);
                    settings_fields(ui, &mut self.project_settings);
                });
        }

        let mut footer_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(footer_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        kit::field_label(&mut footer_ui, "Save Location");
        footer_ui.add_space(6.0);
        if location_picker_row(&mut footer_ui, &self.new_project_parent).clicked() {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                self.new_project_parent = folder;
            }
        }
        footer_ui.add_space(12.0);
        let create_w = footer_ui.available_width();
        if kit::primary_button(&mut footer_ui, "Create Project", create_w).clicked() {
            match self.editor.create_project(
                &self.new_project_parent,
                self.new_project_name.trim(),
                self.project_settings.clone(),
            ) {
                Ok(_) => {
                    self.editor.overlays.new_project = false;
                }
                Err(err) => self.editor.status = err,
            }
        }
    }

    fn project_settings_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        kit::modal_scrim(ctx, "project_settings");
        egui::Window::new("Project Settings")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([560.0, 520.0])
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "Project Settings",
                    Some("Update resolution, timing, and preview scale."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    kit::card_frame()
                        .show(ui, |ui| settings_fields(ui, &mut self.project_settings));
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
        if close_clicked || !open {
            self.editor.overlays.project_settings = false;
        }
    }

    fn generative_video_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        kit::modal_scrim(ctx, "generative_video");
        egui::Window::new("New Generative Video")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([380.0, 220.0])
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
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.gen_video_fps)
                                .speed(1.0)
                                .prefix("FPS "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.gen_video_frames)
                                .speed(1)
                                .prefix("Frames "),
                        );
                    });
                    ui.add_space(8.0);
                    ui.label(kit::body(format!(
                        "Duration {}",
                        generative_video_duration_label(self.gen_video_fps, self.gen_video_frames)
                    )));
                    ui.add_space(18.0);
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
        if close_clicked || !open {
            self.editor.overlays.generative_video = false;
        }
    }

    fn queue_panel(&mut self, ctx: &Context) {
        let mut close_clicked = false;
        egui::Window::new("Generation Queue")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .frame(kit::modal_frame())
            .default_pos([950.0, 70.0])
            .default_size([320.0, 150.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(ui, "Generation Queue", None, true);
                kit::modal_body(ui, |ui| {
                    if self.editor.generation_queue.is_empty() {
                        kit::empty_state(ui, "Empty", "No generation jobs yet.");
                    } else {
                        for job in self.editor.generation_queue.iter() {
                            ui.label(kit::body(format!("{} - {:?}", job.asset_label, job.status)));
                        }
                    }
                });
            });
        if close_clicked {
            self.editor.overlays.queue = false;
        }
    }

    fn providers_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        kit::modal_scrim(ctx, "providers");
        egui::Window::new("AI Providers (Global)")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .default_size([700.0, 520.0])
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "AI Providers",
                    Some("Global provider definitions and manifests."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    ui.label(kit::caption(
                        crate::core::provider_store::global_providers_root()
                            .display()
                            .to_string(),
                    ));
                    ui.add_space(12.0);
                    ui.columns(2, |columns| {
                        columns[0].vertical(|ui| {
                            kit::card_frame().show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if kit::secondary_button(ui, "New", 84.0).clicked() {
                                        self.editor.status =
                                            "Provider builder will be rebuilt in egui.".to_string();
                                    }
                                    if kit::secondary_button(ui, "Reload", 84.0).clicked() {
                                        self.editor.refresh_providers();
                                    }
                                });
                                ui.add_space(10.0);
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    for path in self.editor.provider_files.iter() {
                                        let summary = provider_file_summary(path);
                                        let selected =
                                            self.selected_provider_file.as_ref() == Some(path);
                                        let response = provider_row(ui, path, &summary, selected);
                                        if response.clicked() {
                                            self.selected_provider_file = Some(path.clone());
                                        }
                                    }
                                });
                            });
                        });
                        columns[1].vertical(|ui| {
                            kit::card_frame().show(ui, |ui| {
                                if let Some(path) = &self.selected_provider_file {
                                    ui.label(kit::value(
                                        path.file_name()
                                            .and_then(|v| v.to_str())
                                            .unwrap_or("provider.json"),
                                    ));
                                    ui.add_space(8.0);
                                    match std::fs::read_to_string(path) {
                                        Ok(text) => {
                                            kit::sunken_frame().show(ui, |ui| {
                                                egui::ScrollArea::vertical()
                                                    .max_height(340.0)
                                                    .show(ui, |ui| {
                                                        ui.monospace(text);
                                                    });
                                            });
                                        }
                                        Err(err) => {
                                            ui.label(
                                                RichText::new(format!(
                                                    "Failed to read provider: {err}"
                                                ))
                                                .color(kit::MARKER),
                                            );
                                        }
                                    }
                                } else {
                                    kit::empty_state(
                                        ui,
                                        "Select a provider",
                                        "Choose a provider from the list to inspect its JSON.",
                                    );
                                }
                            });
                        });
                    });
                });
            });
        if close_clicked || !open {
            self.editor.overlays.providers = false;
        }
    }
}

impl eframe::App for NlaEguiApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_automation();
        self.keep_automation_responsive(&ctx);
        self.tick_playback(&ctx);
        self.update_preview_texture(&ctx);

        self.top_bar(&ctx);
        self.left_panel(&ctx);
        self.right_panel(&ctx);

        egui::TopBottomPanel::bottom("status")
            .exact_height(kit::STATUS_BAR_H)
            .frame(kit::chrome_frame())
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&self.editor.status)
                            .small()
                            .color(kit::TEXT_MUTED),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{:.0} fps   {}",
                                self.editor.project.settings.fps,
                                timecode(self.editor.current_time)
                            ))
                            .small()
                            .color(kit::TEXT_MUTED),
                        );
                    });
                });
            });

        self.timeline_panel(&ctx);
        self.central_preview(&ctx);

        self.modals(&ctx);
    }
}

fn menu_button(
    ui: &mut Ui,
    label: &str,
    add_contents: impl FnOnce(&mut Ui, &mut NlaEguiApp),
    app: &mut NlaEguiApp,
) {
    ui.menu_button(kit::menu_text(label), |ui| add_contents(ui, app));
}

fn asset_row(ui: &mut Ui, asset: &Asset, selected: bool) -> egui::Response {
    let accent = asset_accent(asset);
    kit::draw_accent_row(ui, 42.0, selected, accent, |ui, _rect| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(asset_icon(asset))
                    .color(accent)
                    .size(11.0)
                    .strong(),
            );
            let text_w = (ui.available_width() - 8.0).max(40.0);
            ui.vertical(|ui| {
                ui.add_sized(
                    [text_w, 17.0],
                    egui::Label::new(kit::body(&asset.name)).truncate(),
                );
                ui.add_sized(
                    [text_w, 15.0],
                    egui::Label::new(kit::caption(asset_kind_label(&asset.kind))).truncate(),
                );
            });
        });
    })
}

fn asset_icon(asset: &Asset) -> &'static str {
    match asset.kind {
        AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. } => "VID",
        AssetKind::Image { .. } | AssetKind::GenerativeImage { .. } => "IMG",
        AssetKind::Audio { .. } | AssetKind::GenerativeAudio { .. } => "AUD",
    }
}

fn asset_accent(asset: &Asset) -> Color32 {
    match asset.kind {
        AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. } => kit::VIDEO,
        AssetKind::Image { .. } | AssetKind::GenerativeImage { .. } => kit::IMAGE,
        AssetKind::Audio { .. } | AssetKind::GenerativeAudio { .. } => kit::AUDIO,
    }
}

fn asset_kind_label(kind: &AssetKind) -> &'static str {
    match kind {
        AssetKind::Video { .. } => "Video",
        AssetKind::Image { .. } => "Image",
        AssetKind::Audio { .. } => "Audio",
        AssetKind::GenerativeVideo { .. } => "Generative Video",
        AssetKind::GenerativeImage { .. } => "Generative Image",
        AssetKind::GenerativeAudio { .. } => "Generative Audio",
    }
}

struct ProviderFileSummary {
    name: String,
    subtitle: String,
}

fn provider_row(
    ui: &mut Ui,
    _path: &Path,
    summary: &ProviderFileSummary,
    selected: bool,
) -> egui::Response {
    kit::draw_accent_row(ui, 52.0, selected, kit::AUDIO, |ui, _rect| {
        ui.vertical(|ui| {
            ui.label(kit::value(&summary.name));
            ui.label(kit::caption(&summary.subtitle));
        });
    })
}

fn provider_file_summary(path: &Path) -> ProviderFileSummary {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("provider.json")
        .to_string();
    let Ok(text) = std::fs::read_to_string(path) else {
        return ProviderFileSummary {
            name: file_name,
            subtitle: "Unreadable provider file".to_string(),
        };
    };
    let Ok(entry) = serde_json::from_str::<ProviderEntry>(&text) else {
        return ProviderFileSummary {
            name: file_name,
            subtitle: "Invalid provider JSON".to_string(),
        };
    };
    ProviderFileSummary {
        name: entry.name,
        subtitle: format!("{:?}  {}", entry.output_type, path_label(path)),
    }
}

fn path_label(path: &Path) -> String {
    let text = path.display().to_string();
    let len = text.chars().count();
    if len > 48 {
        format!(
            "...{}",
            text.chars()
                .skip(len.saturating_sub(45))
                .collect::<String>()
        )
    } else {
        text
    }
}

fn inspector_text_field(ui: &mut Ui, label: &str, value: &mut String) -> bool {
    kit::field_label(ui, label);
    kit::singleline_text_field(ui, value, ui.available_width()).changed()
}

const INSPECTOR_NUMERIC_H: f32 = 40.0;
const INSPECTOR_NUMERIC_LABEL_H: f32 = 12.0;
const INSPECTOR_NUMERIC_INPUT_H: f32 = 24.0;
const INSPECTOR_NUMERIC_GAP: f32 = 10.0;

fn inspector_numeric_rect(ui: &mut Ui, width: f32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, INSPECTOR_NUMERIC_H), Sense::hover());
    rect
}

fn inspector_numeric_pair_rects(ui: &mut Ui) -> (Rect, Rect) {
    let available = ui.available_width();
    let col_w = ((available - INSPECTOR_NUMERIC_GAP) * 0.5).max(72.0);
    let total_w = col_w * 2.0 + INSPECTOR_NUMERIC_GAP;
    let (row_rect, _) =
        ui.allocate_exact_size(Vec2::new(total_w, INSPECTOR_NUMERIC_H), Sense::hover());
    let left = Rect::from_min_size(row_rect.min, Vec2::new(col_w, INSPECTOR_NUMERIC_H));
    let right = Rect::from_min_size(
        Pos2::new(
            row_rect.left() + col_w + INSPECTOR_NUMERIC_GAP,
            row_rect.top(),
        ),
        Vec2::new(col_w, INSPECTOR_NUMERIC_H),
    );
    (left, right)
}

fn inspector_numeric_field(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    add_control: impl FnOnce(&mut Ui, f32) -> egui::Response,
) -> bool {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_min_size(rect.size());
    child.spacing_mut().item_spacing.y = 4.0;
    child.add_sized(
        [rect.width(), INSPECTOR_NUMERIC_LABEL_H],
        egui::Label::new(kit::caption(label.to_ascii_uppercase())),
    );
    add_control(&mut child, rect.width()).changed()
}

fn inspector_drag_f32(ui: &mut Ui, label: &str, value: &mut f32, speed: f64, width: f32) -> bool {
    let rect = inspector_numeric_rect(ui, width);
    inspector_drag_f32_in_rect(ui, rect, label, value, speed)
}

fn inspector_drag_f32_in_rect(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    value: &mut f32,
    speed: f64,
) -> bool {
    inspector_numeric_field(ui, rect, label, |ui, width| {
        ui.add_sized(
            [width, INSPECTOR_NUMERIC_INPUT_H],
            egui::DragValue::new(value).speed(speed),
        )
    })
}

fn inspector_drag_f64(ui: &mut Ui, label: &str, value: &mut f64, speed: f64, width: f32) -> bool {
    let rect = inspector_numeric_rect(ui, width);
    inspector_drag_f64_in_rect(ui, rect, label, value, speed)
}

fn inspector_drag_f64_in_rect(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    value: &mut f64,
    speed: f64,
) -> bool {
    inspector_numeric_field(ui, rect, label, |ui, width| {
        ui.add_sized(
            [width, INSPECTOR_NUMERIC_INPUT_H],
            egui::DragValue::new(value).speed(speed),
        )
    })
}

fn inspector_drag_u32_in_rect(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    value: &mut u32,
    speed: f64,
) -> bool {
    inspector_numeric_field(ui, rect, label, |ui, width| {
        ui.add_sized(
            [width, INSPECTOR_NUMERIC_INPUT_H],
            egui::DragValue::new(value).speed(speed),
        )
    })
}

fn inspector_two_drag_f32(
    ui: &mut Ui,
    left: (&str, &mut f32, f64),
    right: (&str, &mut f32, f64),
) -> bool {
    let mut changed = false;
    let (left_rect, right_rect) = inspector_numeric_pair_rects(ui);
    changed |= inspector_drag_f32_in_rect(ui, left_rect, left.0, left.1, left.2);
    changed |= inspector_drag_f32_in_rect(ui, right_rect, right.0, right.1, right.2);
    changed
}

fn inspector_two_drag_f64(
    ui: &mut Ui,
    left: (&str, &mut f64, f64),
    right: (&str, &mut f64, f64),
) -> bool {
    let mut changed = false;
    let (left_rect, right_rect) = inspector_numeric_pair_rects(ui);
    changed |= inspector_drag_f64_in_rect(ui, left_rect, left.0, left.1, left.2);
    changed |= inspector_drag_f64_in_rect(ui, right_rect, right.0, right.1, right.2);
    changed
}

fn inspector_two_drag_u32(
    ui: &mut Ui,
    left: (&str, &mut u32, f64),
    right: (&str, &mut u32, f64),
) -> bool {
    let mut changed = false;
    let (left_rect, right_rect) = inspector_numeric_pair_rects(ui);
    changed |= inspector_drag_u32_in_rect(ui, left_rect, left.0, left.1, left.2);
    changed |= inspector_drag_u32_in_rect(ui, right_rect, right.0, right.1, right.2);
    changed
}

fn transform_editor(ui: &mut Ui, transform: &mut ClipTransform, preview_dirty: &mut bool) {
    kit::field_label(ui, "Transform");
    ui.add_space(6.0);
    *preview_dirty |= inspector_two_drag_f32(
        ui,
        ("Position X", &mut transform.position_x, 1.0),
        ("Position Y", &mut transform.position_y, 1.0),
    );
    *preview_dirty |= inspector_two_drag_f32(
        ui,
        ("Scale X", &mut transform.scale_x, 0.01),
        ("Scale Y", &mut transform.scale_y, 0.01),
    );
    *preview_dirty |= inspector_two_drag_f32(
        ui,
        ("Rotation", &mut transform.rotation_deg, 1.0),
        ("Opacity", &mut transform.opacity, 0.01),
    );
}

fn settings_fields(ui: &mut Ui, settings: &mut ProjectSettings) {
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
        ("Width", &mut settings.width, 8.0),
        ("Height", &mut settings.height, 8.0),
    );
    ui.add_space(8.0);
    kit::field_label(ui, "Preview Downsample");
    ui.add_space(6.0);
    let _ = inspector_two_drag_u32(
        ui,
        ("Width", &mut settings.preview_max_width, 8.0),
        ("Height", &mut settings.preview_max_height, 8.0),
    );
    ui.add_space(8.0);
    kit::field_label(ui, "Timing");
    ui.add_space(6.0);
    let mut minutes = settings.duration_seconds / 60.0;
    if inspector_two_drag_f64(
        ui,
        ("FPS", &mut settings.fps, 1.0),
        ("Minutes", &mut minutes, 0.25),
    ) {
        settings.duration_seconds = (minutes * 60.0).max(1.0);
    }
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
        width: 512,
        height: 512,
        preview_width: 512,
        preview_height: 512,
    },
];

fn recent_projects(parent: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(parent)
        .ok()
        .into_iter()
        .flat_map(|read_dir| read_dir.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.join("project.json").exists())
        .take(8)
        .collect()
}

fn location_picker_row(ui: &mut Ui, path: &Path) -> egui::Response {
    let button_w = 76.0;
    let spacing = 8.0;
    let field_w = (ui.available_width() - button_w - spacing).max(90.0);
    let mut button_response = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = spacing;
        kit::readonly_value_box(ui, path.display().to_string(), Vec2::new(field_w, 30.0));
        button_response = Some(kit::secondary_button(ui, "Browse", button_w));
    });
    button_response.expect("location picker row always creates a browse button")
}

fn timecode(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u32;
    let secs = seconds % 60.0;
    format!("{minutes:02}:{secs:05.2}")
}
