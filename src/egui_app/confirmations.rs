use eframe::egui::{self, Context, RichText};
use uuid::Uuid;

use crate::state::ClipImageMode;
use crate::ui_kit as kit;

use super::{
    modal_size, unique_uuid_list, LatentSlateApp, ASSET_DELETE_MODAL_SIZE,
    BRIDGE_KEYFRAME_MODAL_SIZE, TRACK_DELETE_MODAL_SIZE,
};
#[derive(Clone, Debug)]
pub(super) struct AssetDeleteConfirmation {
    pub(super) asset_ids: Vec<Uuid>,
    pub(super) asset_count: usize,
    pub(super) clip_count: usize,
    pub(super) sample_names: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct TrackDeleteConfirmation {
    pub(super) track_ids: Vec<Uuid>,
    pub(super) track_count: usize,
    pub(super) clip_count: usize,
    pub(super) marker_count: usize,
    pub(super) sample_names: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct BridgeKeyframeConfirmation {
    pub(super) clip_ids: Vec<Uuid>,
    pub(super) convert_clip_ids: Vec<Uuid>,
    pub(super) sample_names: Vec<String>,
    pub(super) provider_id: Option<Uuid>,
}

impl LatentSlateApp {
    pub(super) fn request_delete_selected_assets(&mut self) {
        let asset_ids = self.editor.selection.asset_ids.clone();
        self.request_delete_assets(&asset_ids);
    }

    pub(super) fn request_delete_assets(&mut self, asset_ids: &[Uuid]) {
        if let Some(confirmation) = self.asset_delete_confirmation(asset_ids) {
            self.asset_delete_confirmation = Some(confirmation);
        }
    }

    pub(super) fn asset_delete_confirmation(
        &self,
        asset_ids: &[Uuid],
    ) -> Option<AssetDeleteConfirmation> {
        let unique_asset_ids = unique_uuid_list(asset_ids);
        if unique_asset_ids.is_empty() {
            return None;
        }

        let existing_asset_ids: Vec<Uuid> = unique_asset_ids
            .into_iter()
            .filter(|asset_id| self.editor.project.find_asset(*asset_id).is_some())
            .collect();
        if existing_asset_ids.is_empty() {
            return None;
        }

        let clip_count = self
            .editor
            .project
            .clips
            .iter()
            .filter(|clip| existing_asset_ids.contains(&clip.asset_id))
            .count();
        let sample_names = existing_asset_ids
            .iter()
            .filter_map(|asset_id| self.editor.project.find_asset(*asset_id))
            .take(3)
            .map(|asset| asset.name.clone())
            .collect();

        Some(AssetDeleteConfirmation {
            asset_count: existing_asset_ids.len(),
            asset_ids: existing_asset_ids,
            clip_count,
            sample_names,
        })
    }

    pub(super) fn perform_delete_assets(&mut self, asset_ids: &[Uuid]) -> (usize, usize) {
        let unique_asset_ids = unique_uuid_list(asset_ids);
        if !unique_asset_ids.is_empty() {
            self.release_media_handles_for_deleted_assets();
        }
        let result = self.editor.delete_assets(&unique_asset_ids);
        if result.0 > 0 {
            for asset_id in unique_asset_ids {
                self.invalidate_asset_visual_cache(asset_id);
                self.editor.thumbnailer.clear_cache_for_asset(asset_id);
            }
            self.release_media_handles_for_deleted_assets();
            self.editor.preview_dirty = true;
        }
        result
    }

    fn release_media_handles_for_deleted_assets(&mut self) {
        self.invalidate_preview_render_jobs();
        self.preview_layers = None;
        self.editor.previewer.release_media_handles();
        self.asset_lab_video_decoder.release_media_handles();
        if let Some(engine) = &self.audio_engine {
            engine.pause();
            engine.set_items(Vec::new());
        }
        self.editor.is_playing = false;
    }

    pub(super) fn request_delete_selected_tracks(&mut self) {
        let track_ids = self.editor.selection.track_ids.clone();
        self.request_delete_tracks(&track_ids);
    }

    pub(super) fn request_delete_tracks(&mut self, track_ids: &[Uuid]) {
        if let Some(confirmation) = self.track_delete_confirmation(track_ids) {
            self.track_delete_confirmation = Some(confirmation);
        }
    }

    pub(super) fn track_delete_confirmation(
        &self,
        track_ids: &[Uuid],
    ) -> Option<TrackDeleteConfirmation> {
        let unique_track_ids = unique_uuid_list(track_ids);
        if unique_track_ids.is_empty() {
            return None;
        }

        let existing_track_ids: Vec<Uuid> = unique_track_ids
            .into_iter()
            .filter(|track_id| {
                self.editor
                    .project
                    .tracks
                    .iter()
                    .any(|track| track.id == *track_id)
            })
            .collect();
        if existing_track_ids.is_empty() {
            return None;
        }

        let mut clip_count = 0usize;
        let mut marker_count = 0usize;
        for track_id in existing_track_ids.iter().copied() {
            let (clips, markers) = self.editor.project.track_delete_counts(track_id);
            clip_count += clips;
            marker_count += markers;
        }
        let sample_names = existing_track_ids
            .iter()
            .filter_map(|track_id| {
                self.editor
                    .project
                    .tracks
                    .iter()
                    .find(|track| track.id == *track_id)
            })
            .take(4)
            .map(|track| track.name.clone())
            .collect();

        Some(TrackDeleteConfirmation {
            track_count: existing_track_ids.len(),
            track_ids: existing_track_ids,
            clip_count,
            marker_count,
            sample_names,
        })
    }

    pub(super) fn perform_delete_tracks(&mut self, track_ids: &[Uuid]) -> usize {
        let unique_track_ids = unique_uuid_list(track_ids);
        let mut deleted = 0usize;
        for track_id in unique_track_ids {
            if self.editor.project.remove_track(track_id) {
                deleted += 1;
            }
        }
        if deleted > 0 {
            self.editor.selection.clear();
            self.editor.preview_dirty = true;
            self.editor.status = if deleted == 1 {
                "Deleted track".to_string()
            } else {
                format!("Deleted {deleted} tracks")
            };
            self.refresh_audio_playback_items();
        }
        deleted
    }

    pub(super) fn asset_delete_confirmation_modal(&mut self, ctx: &Context) {
        let Some(confirmation) = self.asset_delete_confirmation.clone() else {
            return;
        };

        let mut open = true;
        let mut close_clicked = false;
        let mut cancel_clicked = false;
        let mut delete_clicked = false;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "asset_delete", true);
        let size = modal_size(ctx, ASSET_DELETE_MODAL_SIZE, [380.0, 260.0]);
        egui::Window::new("Delete Assets")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "Delete Assets?",
                    Some("Remove selected project assets and dependent timeline clips."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    kit::body_with_footer(
                        ui,
                        120.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| {
                            let asset_word = if confirmation.asset_count == 1 {
                                "asset"
                            } else {
                                "assets"
                            };
                            let clip_context = match confirmation.clip_count {
                                0 => "No timeline clips reference this selection.".to_string(),
                                1 => "1 timeline clip references this selection.".to_string(),
                                count => {
                                    format!("{count} timeline clips reference this selection.")
                                }
                            };
                            ui.label(
                                RichText::new(format!(
                                    "You are about to delete {} {}.",
                                    confirmation.asset_count, asset_word
                                ))
                                .color(kit::TEXT)
                                .strong(),
                            );
                            ui.add_space(kit::FORM_ROW_GAP);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(clip_context).color(kit::TEXT_MUTED),
                                )
                                .wrap(),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(
                                        "Timeline clip instances will be removed. Source media files on disk are left in place.",
                                    )
                                    .color(kit::TEXT_MUTED),
                                )
                                .wrap(),
                            );
                            if !confirmation.sample_names.is_empty() {
                                ui.add_space(kit::ACTION_GAP);
                                kit::field_label(ui, "Assets");
                                ui.add_space(kit::FORM_ROW_GAP);
                                for name in confirmation.sample_names.iter() {
                                    ui.label(RichText::new(name).color(kit::TEXT));
                                }
                                let remaining =
                                    confirmation.asset_count.saturating_sub(confirmation.sample_names.len());
                                if remaining > 0 {
                                    ui.label(
                                        RichText::new(format!("+ {remaining} more"))
                                            .color(kit::TEXT_MUTED),
                                    );
                                }
                            }
                        },
                        |ui| {
                            kit::equal_width_action_row(
                                ui,
                                2,
                                kit::SECONDARY_BUTTON_H,
                                kit::ACTION_GAP,
                                |ui, index, button_w| match index {
                                    0 => {
                                        cancel_clicked =
                                            kit::secondary_button(ui, "Cancel", button_w)
                                                .clicked();
                                    }
                                    _ => {
                                        delete_clicked =
                                            kit::danger_button(ui, "Delete Assets", button_w)
                                                .clicked();
                                    }
                                },
                            );
                        },
                    );
                });
            });

        if delete_clicked {
            let asset_ids = confirmation.asset_ids.clone();
            self.asset_delete_confirmation = None;
            self.perform_delete_assets(&asset_ids);
        } else if cancel_clicked || close_clicked || outside_clicked || !open {
            self.asset_delete_confirmation = None;
        }
    }

    pub(super) fn track_delete_confirmation_modal(&mut self, ctx: &Context) {
        let Some(confirmation) = self.track_delete_confirmation.clone() else {
            return;
        };

        let mut open = true;
        let mut close_clicked = false;
        let mut cancel_clicked = false;
        let mut delete_clicked = false;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "track_delete", true);
        let size = modal_size(ctx, TRACK_DELETE_MODAL_SIZE, [380.0, 260.0]);
        egui::Window::new("Delete Tracks")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "Delete Tracks?",
                    Some("Remove selected timeline tracks and their contents."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    kit::body_with_footer(
                        ui,
                        110.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| {
                            let track_word = if confirmation.track_count == 1 {
                                "track"
                            } else {
                                "tracks"
                            };
                            ui.label(
                                RichText::new(format!(
                                    "You are about to delete {} {}.",
                                    confirmation.track_count, track_word
                                ))
                                .color(kit::TEXT)
                                .strong(),
                            );
                            ui.add_space(kit::FORM_ROW_GAP);
                            let clip_context = match confirmation.clip_count {
                                0 => "No clips will be removed.".to_string(),
                                1 => "1 clip will be removed.".to_string(),
                                count => format!("{count} clips will be removed."),
                            };
                            let marker_context = match confirmation.marker_count {
                                0 => "No markers will be removed.".to_string(),
                                1 => "1 marker will be removed.".to_string(),
                                count => format!("{count} markers will be removed."),
                            };
                            ui.label(RichText::new(clip_context).color(kit::TEXT_MUTED));
                            ui.label(RichText::new(marker_context).color(kit::TEXT_MUTED));
                            if !confirmation.sample_names.is_empty() {
                                ui.add_space(kit::ACTION_GAP);
                                kit::field_label(ui, "Tracks");
                                ui.add_space(kit::FORM_ROW_GAP);
                                for name in confirmation.sample_names.iter() {
                                    ui.label(RichText::new(name).color(kit::TEXT));
                                }
                                let remaining = confirmation
                                    .track_count
                                    .saturating_sub(confirmation.sample_names.len());
                                if remaining > 0 {
                                    ui.label(
                                        RichText::new(format!("+ {remaining} more"))
                                            .color(kit::TEXT_MUTED),
                                    );
                                }
                            }
                        },
                        |ui| {
                            kit::equal_width_action_row(
                                ui,
                                2,
                                kit::SECONDARY_BUTTON_H,
                                kit::ACTION_GAP,
                                |ui, index, button_w| match index {
                                    0 => {
                                        cancel_clicked =
                                            kit::secondary_button(ui, "Cancel", button_w).clicked();
                                    }
                                    _ => {
                                        delete_clicked =
                                            kit::danger_button(ui, "Delete Tracks", button_w)
                                                .clicked();
                                    }
                                },
                            );
                        },
                    );
                });
            });

        if delete_clicked {
            let track_ids = confirmation.track_ids.clone();
            self.track_delete_confirmation = None;
            self.perform_delete_tracks(&track_ids);
        } else if cancel_clicked || close_clicked || outside_clicked || !open {
            self.track_delete_confirmation = None;
        }
    }

    pub(super) fn bridge_keyframe_confirmation_modal(&mut self, ctx: &Context) {
        let Some(confirmation) = self.bridge_keyframe_confirmation.clone() else {
            return;
        };

        let mut open = true;
        let mut close_clicked = false;
        let mut cancel_clicked = false;
        let mut create_clicked = false;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "bridge_keyframes", true);
        let size = modal_size(ctx, BRIDGE_KEYFRAME_MODAL_SIZE, [400.0, 280.0]);
        egui::Window::new("Convert Keyframes")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "Use Images as Keyframes?",
                    Some("The bridge will anchor image pins and video boundary frames."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    kit::body_with_footer(
                        ui,
                        130.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} referenced image clips will switch to keyframe display mode.",
                                    confirmation.convert_clip_ids.len()
                                ))
                                .color(kit::TEXT)
                                .strong(),
                            );
                            ui.add_space(kit::FORM_ROW_GAP);
                            ui.label(
                                RichText::new(
                                    "Keyframe image clips keep their asset and timing data, but draw as marker-like pins on the timeline. Video references use only the first or last frame at the selected boundary.",
                                )
                                .color(kit::TEXT_MUTED),
                            );
                            ui.label(
                                RichText::new(
                                    "Normal still-image behavior remains available per clip in the Attributes panel.",
                                )
                                .color(kit::TEXT_MUTED),
                            );
                            if !confirmation.sample_names.is_empty() {
                                ui.add_space(kit::ACTION_GAP);
                                kit::field_label(ui, "Referenced Media");
                                ui.add_space(kit::FORM_ROW_GAP);
                                for name in confirmation.sample_names.iter() {
                                    ui.label(RichText::new(name).color(kit::TEXT));
                                }
                            }
                        },
                        |ui| {
                            kit::equal_width_action_row(
                                ui,
                                2,
                                kit::SECONDARY_BUTTON_H,
                                kit::ACTION_GAP,
                                |ui, index, button_w| match index {
                                    0 => {
                                        cancel_clicked =
                                            kit::secondary_button(ui, "Cancel", button_w)
                                                .clicked();
                                    }
                                    _ => {
                                        create_clicked =
                                            kit::primary_button(ui, "Convert + Create", button_w)
                                                .clicked();
                                    }
                                },
                            );
                        },
                    );
                });
            });

        if create_clicked {
            let clip_ids = confirmation.clip_ids.clone();
            for clip_id in confirmation.convert_clip_ids.iter() {
                self.editor
                    .project
                    .set_clip_image_mode(*clip_id, ClipImageMode::Keyframe);
            }
            self.bridge_keyframe_confirmation = None;
            self.create_bridge_video_from_clip_ids(&clip_ids, confirmation.provider_id);
            self.editor.preview_dirty = true;
        } else if cancel_clicked || close_clicked || outside_clicked || !open {
            self.bridge_keyframe_confirmation = None;
        }
    }
}
