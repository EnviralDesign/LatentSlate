use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, ColorImage, Context, FontId, Layout, Pos2, Rect, RichText, Sense,
    Stroke, TextureHandle, TextureOptions, Ui, Vec2,
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

const PANEL_BG: Color32 = Color32::from_rgb(18, 18, 19);
const SURFACE: Color32 = Color32::from_rgb(24, 24, 26);
const SURFACE_2: Color32 = Color32::from_rgb(30, 30, 33);
const BORDER: Color32 = Color32::from_rgb(43, 43, 48);
const TEXT: Color32 = Color32::from_rgb(244, 244, 245);
const MUTED: Color32 = Color32::from_rgb(145, 145, 155);
const DIM: Color32 = Color32::from_rgb(90, 90, 100);
const GREEN: Color32 = Color32::from_rgb(34, 197, 94);
const BLUE: Color32 = Color32::from_rgb(59, 130, 246);
const ORANGE: Color32 = Color32::from_rgb(249, 115, 22);

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
        configure_style(&cc.egui_ctx);
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
            self.preview_texture = Some(ctx.load_texture("preview-frame", image, TextureOptions::LINEAR));
        }
        self.preview_frame = Some(frame);
        self.editor.preview_dirty = false;
    }

    fn top_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("top_bar")
            .exact_height(32.0)
            .frame(egui::Frame::new().fill(Color32::from_rgb(20, 20, 22)).stroke(Stroke::new(1.0, BORDER)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    menu_button(ui, "File", |ui, this: &mut Self| {
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
                    }, self);

                    menu_button(ui, "Edit", |ui, this: &mut Self| {
                        if ui.button("Add Marker").clicked() {
                            this.editor.add_marker(None);
                            ui.close();
                        }
                        if ui.button("Create Generative Video...").clicked() {
                            this.editor.overlays.generative_video = true;
                            ui.close();
                        }
                    }, self);

                    menu_button(ui, "View", |ui, this: &mut Self| {
                        ui.checkbox(&mut this.editor.layout.preview_stats, "Preview Stats");
                        ui.checkbox(&mut this.editor.layout.left_collapsed, "Collapse Assets");
                        ui.checkbox(&mut this.editor.layout.right_collapsed, "Collapse Attributes");
                        ui.checkbox(&mut this.editor.layout.timeline_collapsed, "Collapse Timeline");
                    }, self);

                    menu_button(ui, "Settings", |ui, this: &mut Self| {
                        if ui.button("AI Providers...").clicked() {
                            this.editor.refresh_providers();
                            this.editor.overlays.providers = true;
                            ui.close();
                        }
                        ui.checkbox(&mut this.editor.layout.hardware_decode, "Hardware Decode");
                    }, self);

                    menu_button(ui, "Help", |ui, this: &mut Self| {
                        ui.label(RichText::new("NLA AI Video Creator").strong());
                        ui.label(RichText::new("egui migration build").small().color(MUTED));
                        if ui.button("Open Harness Docs").clicked() {
                            this.editor.status = "See docs/DESKTOP_TEST_HARNESS.md".to_string();
                            ui.close();
                        }
                    }, self);

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let running = self.editor.generation_queue.iter().any(|job| {
                            job.status == crate::state::GenerationJobStatus::Running
                        });
                        let queue_text = if self.editor.generation_queue.is_empty() {
                            "QUE".to_string()
                        } else {
                            format!("QUE {}", self.editor.generation_queue.len())
                        };
                        if ui.add(egui::Button::new(queue_text).selected(self.editor.overlays.queue || running)).clicked() {
                            self.editor.overlays.queue = !self.editor.overlays.queue;
                        }
                        ui.label(RichText::new(self.editor.project_name()).color(MUTED));
                    });
                });
            });
    }

    fn left_panel(&mut self, ctx: &Context) {
        if self.editor.layout.left_collapsed {
            egui::SidePanel::left("assets_collapsed")
                .exact_width(36.0)
                .frame(panel_frame())
                .show(ctx, |ui| {
                    if ui.button("A").clicked() {
                        self.editor.layout.left_collapsed = false;
                    }
                });
            return;
        }

        egui::SidePanel::left("assets")
            .resizable(true)
            .default_width(self.editor.layout.left_width)
            .width_range(180.0..=420.0)
            .frame(panel_frame())
            .show(ctx, |ui| self.assets_panel(ui));
    }

    fn assets_panel(&mut self, ui: &mut Ui) {
        panel_header(ui, "ASSETS", Some("◀"), || {
            self.editor.layout.left_collapsed = true;
        });
        ui.add_space(6.0);
        if ui.add_sized([ui.available_width(), 34.0], egui::Button::new("📁 Import Files...")).clicked() {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                match self.editor.import_asset(path) {
                    Ok(asset_id) => self.editor.selection.asset_ids = vec![asset_id],
                    Err(err) => self.editor.status = err,
                }
            }
        }

        ui.add_space(12.0);
        ui.label(RichText::new("NEW GENERATIVE").small().color(MUTED));
        ui.horizontal(|ui| {
            if colored_small_button(ui, "▣ Video", GREEN).clicked() {
                self.editor.overlays.generative_video = true;
            }
            if colored_small_button(ui, "▣ Image", GREEN).clicked() {
                if let Err(err) = self.editor.create_generative_image() {
                    self.editor.status = err;
                }
            }
            if colored_small_button(ui, "◆ Audio", BLUE).clicked() {
                if let Err(err) = self.editor.create_generative_audio() {
                    self.editor.status = err;
                }
            }
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
                .frame(panel_frame())
                .show(ctx, |ui| {
                    if ui.button("A").clicked() {
                        self.editor.layout.right_collapsed = false;
                    }
                });
            return;
        }

        egui::SidePanel::right("attributes")
            .resizable(true)
            .default_width(self.editor.layout.right_width)
            .width_range(200.0..=440.0)
            .frame(panel_frame())
            .show(ctx, |ui| self.attributes_panel(ui));
    }

    fn attributes_panel(&mut self, ui: &mut Ui) {
        panel_header(ui, "ATTRIBUTES", Some("▶"), || {
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
            ui.label(RichText::new("Nothing selected").color(DIM));
        }
    }

    fn clip_attributes(&mut self, ui: &mut Ui, clip_id: Uuid) {
        let asset_name = self.editor.project.clips.iter()
            .find(|clip| clip.id == clip_id)
            .and_then(|clip| self.editor.project.find_asset(clip.asset_id))
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| "Unknown asset".to_string());
        ui.label(RichText::new("CLIP").small().color(MUTED));
        ui.label(RichText::new(asset_name).strong());
        ui.add_space(10.0);
        let mut preview_dirty = false;
        if let Some(clip) = self.editor.project.clips.iter_mut().find(|clip| clip.id == clip_id) {
            ui.label(RichText::new("Clip Name").small().color(MUTED));
            let mut label = clip.label.clone().unwrap_or_default();
            if ui.text_edit_singleline(&mut label).changed() {
                clip.label = if label.trim().is_empty() { None } else { Some(label) };
            }
            ui.add_space(10.0);
            transform_editor(ui, &mut clip.transform, &mut preview_dirty);
            ui.add_space(10.0);
            ui.label(RichText::new("Timing").small().color(MUTED));
            ui.horizontal(|ui| {
                preview_dirty |= ui.add(egui::DragValue::new(&mut clip.start_time).speed(0.05).prefix("Start ")).changed();
                preview_dirty |= ui.add(egui::DragValue::new(&mut clip.duration).speed(0.05).prefix("Dur ")).changed();
            });
        }
        if preview_dirty {
            self.editor.preview_dirty = true;
        }
    }

    fn asset_attributes(&mut self, ui: &mut Ui, asset_id: Uuid) {
        if let Some(asset) = self.editor.project.assets.iter_mut().find(|asset| asset.id == asset_id) {
            ui.label(RichText::new("ASSET").small().color(MUTED));
            ui.text_edit_singleline(&mut asset.name);
            ui.add_space(8.0);
            ui.label(RichText::new(asset_kind_label(&asset.kind)).color(MUTED));
            if let Some(duration) = asset.duration_seconds {
                ui.label(format!("Duration: {duration:.2}s"));
            }
            ui.add_space(8.0);
            if ui.button("Add to timeline").clicked() {
                if let Err(err) = self.editor.add_asset_to_timeline(asset_id, None) {
                    self.editor.status = err;
                }
            }
        }
    }

    fn marker_attributes(&mut self, ui: &mut Ui, marker_id: Uuid) {
        if let Some(marker) = self.editor.project.markers.iter_mut().find(|marker| marker.id == marker_id) {
            ui.label(RichText::new("MARKER").small().color(MUTED));
            let mut changed = false;
            changed |= ui.add(egui::DragValue::new(&mut marker.time).speed(0.05).prefix("Time ")).changed();
            let mut label = marker.label.clone().unwrap_or_default();
            if ui.text_edit_singleline(&mut label).changed() {
                marker.label = if label.trim().is_empty() { None } else { Some(label) };
            }
            if changed {
                self.editor.project.markers.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
                self.editor.preview_dirty = true;
            }
        }
    }

    fn track_attributes(&mut self, ui: &mut Ui, track_id: Uuid) {
        if let Some(track) = self.editor.project.tracks.iter_mut().find(|track| track.id == track_id) {
            ui.label(RichText::new("TRACK").small().color(MUTED));
            ui.text_edit_singleline(&mut track.name);
            ui.label(format!("{:?}", track.track_type));
            if track.track_type != TrackType::Marker {
                ui.add(egui::Slider::new(&mut track.volume, 0.0..=2.0).text("Volume"));
            }
        }
    }

    fn central_preview(&mut self, ctx: &Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(10, 10, 11)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("PREVIEW").small().color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let s = &self.editor.project.settings;
                        ui.label(RichText::new(format!("{} x {} @ {:.0}", s.width, s.height, s.fps)).small().color(MUTED));
                    });
                });
                ui.separator();
                let available = ui.available_size();
                let preview_height = available.y.max(160.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(available.x, preview_height), Sense::hover());
                self.paint_preview(ui, rect);
            });
    }

    fn paint_preview(&mut self, ui: &mut Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
        ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
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
                DIM,
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
                ui.painter().text(
                    rect.right_top() + Vec2::new(-180.0, 14.0),
                    egui::Align2::LEFT_TOP,
                    text,
                    FontId::monospace(11.0),
                    MUTED,
                );
            }
        }
    }

    fn timeline_panel(&mut self, ctx: &Context) {
        let height = if self.editor.layout.timeline_collapsed {
            34.0
        } else {
            self.editor.layout.timeline_height
        };
        egui::TopBottomPanel::bottom("timeline")
            .exact_height(height)
            .frame(panel_frame())
            .show(ctx, |ui| {
                if self.editor.layout.timeline_collapsed {
                    ui.horizontal(|ui| {
                        if ui.button("TIMELINE ▴").clicked() {
                            self.editor.layout.timeline_collapsed = false;
                        }
                        ui.label(timecode(self.editor.current_time));
                    });
                    return;
                }
                self.timeline_header(ui);
                self.paint_timeline(ui);
            });
    }

    fn timeline_header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("TIMELINE").small().color(MUTED));
            if ui.small_button("—").clicked() {
                self.editor.layout.timeline_collapsed = true;
            }
            ui.separator();
            if ui.button(if self.editor.is_playing { "⏸" } else { "▶" }).clicked() {
                self.editor.is_playing = !self.editor.is_playing;
            }
            if ui.small_button("◀").clicked() {
                self.editor.seek((self.editor.current_time - 1.0).max(0.0));
            }
            if ui.small_button("▶").clicked() {
                self.editor.seek(self.editor.current_time + 1.0);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(timecode(self.editor.current_time)).monospace().color(MUTED));
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
        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), total_h), Sense::click());
        let painter = ui.painter_at(rect);
        let timeline_rect = Rect::from_min_max(
            Pos2::new(rect.left() + label_width, rect.top() + ruler_h),
            rect.right_bottom(),
        );
        painter.rect_filled(rect, 0.0, Color32::from_rgb(13, 13, 15));
        painter.line_segment(
            [Pos2::new(rect.left() + label_width, rect.top()), Pos2::new(rect.left() + label_width, rect.bottom())],
            Stroke::new(1.0, BORDER),
        );

        for i in 0..=6 {
            let t = duration * i as f64 / 6.0;
            let x = timeline_rect.left() + timeline_rect.width() * i as f32 / 6.0;
            painter.line_segment(
                [Pos2::new(x, rect.top() + ruler_h - 6.0), Pos2::new(x, rect.bottom())],
                Stroke::new(1.0, Color32::from_rgb(35, 35, 40)),
            );
            painter.text(
                Pos2::new(x + 4.0, rect.top() + 4.0),
                egui::Align2::LEFT_TOP,
                timecode(t),
                FontId::monospace(10.0),
                MUTED,
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
                if selected { Color32::from_rgb(30, 44, 38) } else { Color32::from_rgb(18, 18, 20) },
            );
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(rect.left() + label_width, row_rect.top()),
                    row_rect.right_bottom(),
                ),
                0.0,
                if row % 2 == 0 {
                    Color32::from_rgb(15, 15, 17)
                } else {
                    Color32::from_rgb(12, 12, 14)
                },
            );
            painter.line_segment(
                [Pos2::new(rect.left(), row_rect.bottom()), Pos2::new(rect.right(), row_rect.bottom())],
                Stroke::new(1.0, Color32::from_rgb(38, 38, 43)),
            );
            let color = match track.track_type {
                TrackType::Video => GREEN,
                TrackType::Audio => BLUE,
                TrackType::Marker => ORANGE,
            };
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(rect.left() + 10.0, y + 10.0), Vec2::new(3.0, 16.0)),
                1.0,
                color,
            );
            painter.text(
                Pos2::new(rect.left() + 24.0, y + 10.0),
                egui::Align2::LEFT_TOP,
                &track.name,
                FontId::proportional(12.5),
                Color32::from_rgb(230, 230, 236),
            );
        }

        let clips = self.editor.project.clips.clone();
        for clip in clips {
            if let Some(track_index) = self.editor.project.tracks.iter().position(|track| track.id == clip.track_id) {
                let clip_rect = self.clip_rect(&clip, timeline_rect, duration, track_index, row_h);
                let selected = self.editor.selection.clip_ids.contains(&clip.id);
                painter.rect_filled(clip_rect, 3.0, Color32::from_rgb(16, 150, 98));
                painter.rect_stroke(
                    clip_rect,
                    3.0,
                    Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { Color32::from_rgb(0, 255, 170) } else { GREEN }),
                    egui::StrokeKind::Inside,
                );
                let label = self.editor.project.find_asset(clip.asset_id)
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

        if let Some(marker_row) = self.editor.project.tracks.iter().position(|track| track.track_type == TrackType::Marker) {
            for marker in self.editor.project.markers.iter() {
                let x = timeline_rect.left() + (marker.time as f32 / duration as f32) * timeline_rect.width();
                let y = timeline_rect.top() + marker_row as f32 * row_h;
                painter.line_segment(
                    [Pos2::new(x, y + 4.0), Pos2::new(x, y + row_h - 4.0)],
                    Stroke::new(2.0, ORANGE),
                );
                painter.circle_filled(Pos2::new(x, y + 8.0), 4.0, ORANGE);
            }
        }

        let playhead_x = timeline_rect.left()
            + (self.editor.current_time as f32 / duration as f32) * timeline_rect.width();
        painter.line_segment(
            [Pos2::new(playhead_x, rect.top() + ruler_h - 2.0), Pos2::new(playhead_x, rect.bottom())],
            Stroke::new(2.0, Color32::from_rgb(239, 68, 68)),
        );
        painter.circle_filled(Pos2::new(playhead_x, rect.top() + ruler_h - 2.0), 5.0, Color32::from_rgb(239, 68, 68));

        painter.text(
            Pos2::new(rect.left() + 14.0, rect.bottom() - 26.0),
            egui::Align2::LEFT_TOP,
            "+ Video    + Audio",
            FontId::proportional(11.0),
            DIM,
        );

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.handle_timeline_click(pos, timeline_rect, duration, row_h);
            }
        }
    }

    fn clip_rect(&self, clip: &Clip, timeline_rect: Rect, duration: f64, row: usize, row_h: f32) -> Rect {
        let x1 = timeline_rect.left() + (clip.start_time as f32 / duration as f32) * timeline_rect.width();
        let x2 = timeline_rect.left() + (clip.end_time() as f32 / duration as f32) * timeline_rect.width();
        let y = timeline_rect.top() + row as f32 * row_h + 6.0;
        Rect::from_min_max(Pos2::new(x1, y), Pos2::new(x2.max(x1 + 46.0), y + row_h - 12.0))
    }

    fn handle_timeline_click(&mut self, pos: Pos2, timeline_rect: Rect, duration: f64, row_h: f32) {
        if pos.x < timeline_rect.left() {
            let row = ((pos.y - timeline_rect.top()) / row_h).floor().max(0.0) as usize;
            if let Some(track) = self.editor.project.tracks.get(row) {
                self.editor.selection.select_track(track.id);
            }
            return;
        }
        let time = ((pos.x - timeline_rect.left()) / timeline_rect.width()).clamp(0.0, 1.0) as f64 * duration;
        for clip in self.editor.project.clips.clone() {
            if let Some(track_index) = self.editor.project.tracks.iter().position(|track| track.id == clip.track_id) {
                let rect = self.clip_rect(&clip, timeline_rect, duration, track_index, row_h);
                if rect.contains(pos) {
                    self.editor.selection.select_clip(clip.id);
                    return;
                }
            }
        }
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
        egui::Window::new("NLA AI Video Creator")
            .collapsible(false)
            .resizable(false)
            .fixed_size([720.0, 560.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| self.new_project_modal_contents(ui, true));
    }

    fn new_project_modal(&mut self, ctx: &Context, startup: bool) {
        let mut open = true;
        egui::Window::new(if startup { "Create Project" } else { "New Project" })
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([560.0, 430.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| self.new_project_modal_contents(ui, startup));
        if !open {
            self.editor.overlays.new_project = false;
        }
    }

    fn new_project_modal_contents(&mut self, ui: &mut Ui, startup: bool) {
        ui.label(RichText::new(if startup { "Create a new project or open an existing one" } else { "Create a new project" }).color(MUTED));
        ui.add_space(12.0);
        ui.columns(2, |columns| {
            columns[0].vertical(|ui| {
                ui.label(RichText::new("PROJECT NAME").small().color(MUTED));
                ui.text_edit_singleline(&mut self.new_project_name);
                ui.add_space(8.0);
                settings_fields(ui, &mut self.project_settings);
                ui.add_space(8.0);
                ui.label(RichText::new("SAVE LOCATION").small().color(MUTED));
                ui.horizontal(|ui| {
                    ui.label(path_label(&self.new_project_parent));
                    if ui.button("Browse").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.new_project_parent = folder;
                        }
                    }
                });
                ui.add_space(12.0);
                if ui.add_sized([ui.available_width(), 38.0], egui::Button::new("Create Project")).clicked() {
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
            });
            columns[1].vertical(|ui| {
                ui.label(RichText::new("RECENT PROJECTS").strong());
                ui.add_space(8.0);
                let recent = recent_projects(&self.new_project_parent);
                if recent.is_empty() {
                    ui.label(RichText::new("No recent projects in this folder").color(DIM));
                }
                for folder in recent {
                    if ui.button(folder.file_name().and_then(|v| v.to_str()).unwrap_or("Project")).clicked() {
                        if let Err(err) = self.editor.open_project(folder) {
                            self.editor.status = err;
                        }
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    if ui.button("Browse for Project...").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            if let Err(err) = self.editor.open_project(folder) {
                                self.editor.status = err;
                            }
                        }
                    }
                });
            });
        });
    }

    fn project_settings_modal(&mut self, ctx: &Context) {
        let mut open = true;
        egui::Window::new("Project Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([520.0, 420.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                settings_fields(ui, &mut self.project_settings);
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.project_settings = self.editor.project.settings.clone();
                        self.editor.overlays.project_settings = false;
                    }
                    if ui.button("Apply").clicked() {
                        self.editor.project.settings = self.project_settings.clone();
                        self.editor.preview_dirty = true;
                        self.editor.overlays.project_settings = false;
                    }
                });
            });
        if !open {
            self.editor.overlays.project_settings = false;
        }
    }

    fn generative_video_modal(&mut self, ctx: &Context) {
        let mut open = true;
        egui::Window::new("New Generative Video")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([360.0, 190.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new("Define the target duration for this asset.").small().color(MUTED));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.gen_video_fps).speed(1.0).prefix("FPS "));
                    ui.add(egui::DragValue::new(&mut self.gen_video_frames).speed(1).prefix("Frames "));
                });
                ui.add_space(8.0);
                ui.label(format!("Duration {}", generative_video_duration_label(self.gen_video_fps, self.gen_video_frames)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Create").clicked() {
                        if let Err(err) = self.editor.create_generative_video(self.gen_video_fps, self.gen_video_frames) {
                            self.editor.status = err;
                        }
                        self.editor.overlays.generative_video = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.editor.overlays.generative_video = false;
                    }
                });
            });
        if !open {
            self.editor.overlays.generative_video = false;
        }
    }

    fn queue_panel(&mut self, ctx: &Context) {
        egui::Window::new("Generation Queue")
            .default_pos([950.0, 70.0])
            .default_size([320.0, 130.0])
            .show(ctx, |ui| {
                if self.editor.generation_queue.is_empty() {
                    ui.label(RichText::new("No generation jobs yet.").color(DIM));
                } else {
                    for job in self.editor.generation_queue.iter() {
                        ui.label(format!("{} - {:?}", job.asset_label, job.status));
                    }
                }
                if ui.button("Close").clicked() {
                    self.editor.overlays.queue = false;
                }
            });
    }

    fn providers_modal(&mut self, ctx: &Context) {
        let mut open = true;
        egui::Window::new("AI Providers (Global)")
            .open(&mut open)
            .collapsible(false)
            .default_size([700.0, 520.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(crate::core::provider_store::global_providers_root().display().to_string()).small().color(MUTED));
                ui.separator();
                ui.columns(2, |columns| {
                    columns[0].vertical(|ui| {
                        ui.horizontal(|ui| {
                            if ui.button("New").clicked() {
                                self.editor.status = "Provider builder will be rebuilt in egui.".to_string();
                            }
                            if ui.button("Reload").clicked() {
                                self.editor.refresh_providers();
                            }
                        });
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for path in self.editor.provider_files.iter() {
                                let summary = provider_file_summary(path);
                                let selected = self.selected_provider_file.as_ref() == Some(path);
                                let response = ui.add_sized(
                                    [ui.available_width(), 48.0],
                                    egui::Button::new(
                                        RichText::new(format!("{}\n{}", summary.name, summary.subtitle))
                                            .color(TEXT),
                                    )
                                    .selected(selected),
                                );
                                if response.clicked() {
                                    self.selected_provider_file = Some(path.clone());
                                }
                            }
                        });
                    });
                    columns[1].vertical(|ui| {
                        if let Some(path) = &self.selected_provider_file {
                            ui.label(RichText::new(path.file_name().and_then(|v| v.to_str()).unwrap_or("provider.json")).strong());
                            ui.add_space(8.0);
                            match std::fs::read_to_string(path) {
                                Ok(text) => {
                                    egui::ScrollArea::vertical().show(ui, |ui| {
                                        ui.monospace(text);
                                    });
                                }
                                Err(err) => {
                                    ui.label(RichText::new(format!("Failed to read provider: {err}")).color(ORANGE));
                                }
                            }
                        } else {
                            ui.centered_and_justified(|ui| {
                                ui.label(RichText::new("Select a provider from the list").color(DIM));
                            });
                        }
                    });
                });
            });
        if !open {
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
            .exact_height(22.0)
            .frame(egui::Frame::new().fill(Color32::from_rgb(20, 20, 22)).stroke(Stroke::new(1.0, BORDER)))
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.editor.status).small().color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{:.0} fps   {}", self.editor.project.settings.fps, timecode(self.editor.current_time))).small().color(MUTED));
                    });
                });
            });

        self.timeline_panel(&ctx);
        self.central_preview(&ctx);

        self.modals(&ctx);
    }
}

fn configure_style(ctx: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = SURFACE;
    visuals.panel_fill = PANEL_BG;
    visuals.extreme_bg_color = Color32::from_rgb(7, 7, 8);
    visuals.widgets.inactive.bg_fill = SURFACE_2;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 45, 50);
    visuals.widgets.active.bg_fill = Color32::from_rgb(55, 70, 90);
    visuals.selection.bg_fill = Color32::from_rgb(35, 95, 70);
    ctx.set_visuals(visuals);
}

fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL_BG)
        .stroke(Stroke::new(1.0, BORDER))
        .inner_margin(egui::Margin::same(8))
}

fn panel_header(ui: &mut Ui, label: &str, toggle: Option<&str>, mut on_toggle: impl FnMut()) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).small().color(MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(toggle) = toggle {
                if ui.small_button(toggle).clicked() {
                    on_toggle();
                }
            }
        });
    });
}

fn menu_button(
    ui: &mut Ui,
    label: &str,
    add_contents: impl FnOnce(&mut Ui, &mut NlaEguiApp),
    app: &mut NlaEguiApp,
) {
    ui.menu_button(label, |ui| add_contents(ui, app));
}

fn colored_small_button(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(color)).small())
}

fn asset_row(ui: &mut Ui, asset: &Asset, selected: bool) -> egui::Response {
    let text = format!("{}  {}", asset_icon(asset), asset.name);
    ui.add_sized(
        [ui.available_width(), 34.0],
        egui::Button::new(RichText::new(text).color(TEXT)).selected(selected),
    )
}

fn asset_icon(asset: &Asset) -> &'static str {
    match asset.kind {
        AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. } => "🎬",
        AssetKind::Image { .. } | AssetKind::GenerativeImage { .. } => "🖼",
        AssetKind::Audio { .. } | AssetKind::GenerativeAudio { .. } => "◆",
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

fn transform_editor(ui: &mut Ui, transform: &mut ClipTransform, preview_dirty: &mut bool) {
    ui.label(RichText::new("TRANSFORM").small().color(MUTED));
    egui::Grid::new("transform_grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
        *preview_dirty |= ui.add(egui::DragValue::new(&mut transform.position_x).speed(1.0).prefix("X ")).changed();
        *preview_dirty |= ui.add(egui::DragValue::new(&mut transform.position_y).speed(1.0).prefix("Y ")).changed();
        ui.end_row();
        *preview_dirty |= ui.add(egui::DragValue::new(&mut transform.scale_x).speed(0.01).prefix("Scale X ")).changed();
        *preview_dirty |= ui.add(egui::DragValue::new(&mut transform.scale_y).speed(0.01).prefix("Scale Y ")).changed();
        ui.end_row();
        *preview_dirty |= ui.add(egui::DragValue::new(&mut transform.rotation_deg).speed(1.0).prefix("Rot ")).changed();
        *preview_dirty |= ui.add(egui::Slider::new(&mut transform.opacity, 0.0..=1.0).text("Opacity")).changed();
        ui.end_row();
    });
}

fn settings_fields(ui: &mut Ui, settings: &mut ProjectSettings) {
    ui.label(RichText::new("RESOLUTION").small().color(MUTED));
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut settings.width).speed(8).prefix("W "));
        ui.add(egui::DragValue::new(&mut settings.height).speed(8).prefix("H "));
    });
    ui.label(RichText::new("PREVIEW DOWNSAMPLE").small().color(MUTED));
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut settings.preview_max_width).speed(8).prefix("W "));
        ui.add(egui::DragValue::new(&mut settings.preview_max_height).speed(8).prefix("H "));
    });
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut settings.fps).speed(1.0).prefix("FPS "));
        let mut minutes = settings.duration_seconds / 60.0;
        if ui.add(egui::DragValue::new(&mut minutes).speed(0.25).prefix("Minutes ")).changed() {
            settings.duration_seconds = (minutes * 60.0).max(1.0);
        }
    });
}

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

fn path_label(path: &Path) -> String {
    let text = path.display().to_string();
    if text.len() > 48 {
        format!("...{}", &text[text.len().saturating_sub(45)..])
    } else {
        text
    }
}

fn timecode(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u32;
    let secs = seconds % 60.0;
    format!("{minutes:02}:{secs:05.2}")
}
