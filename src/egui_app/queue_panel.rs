use super::*;

use eframe::egui::{
    self, Align, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2,
};
use uuid::Uuid;

use crate::state::{GenerationJob, GenerationJobStatus, ProviderOutputType};
use crate::ui_kit as kit;

use super::{
    QUEUE_EMPTY_BODY_H, QUEUE_JOB_CARD_H, QUEUE_JOB_FAILED_H, QUEUE_JOB_GAP, QUEUE_JOB_RUNNING_H,
};

pub(super) fn queue_list_height(jobs: &[GenerationJob]) -> f32 {
    if jobs.is_empty() {
        return QUEUE_EMPTY_BODY_H;
    }
    jobs.iter().map(queue_job_height).sum::<f32>()
        + QUEUE_JOB_GAP * jobs.len().saturating_sub(1) as f32
}

fn queue_job_height(job: &GenerationJob) -> f32 {
    match job.status {
        GenerationJobStatus::Running => QUEUE_JOB_RUNNING_H,
        GenerationJobStatus::Canceling => QUEUE_JOB_RUNNING_H,
        GenerationJobStatus::Failed => QUEUE_JOB_FAILED_H,
        GenerationJobStatus::Queued
        | GenerationJobStatus::Succeeded
        | GenerationJobStatus::Canceled => QUEUE_JOB_CARD_H,
    }
}

pub(super) fn queue_job_is_terminal(status: GenerationJobStatus) -> bool {
    matches!(
        status,
        GenerationJobStatus::Succeeded
            | GenerationJobStatus::Failed
            | GenerationJobStatus::Canceled
    )
}

pub(super) fn paint_queue_panel_shell(ui: &mut Ui, rect: Rect, attention: bool) {
    let radius = egui::CornerRadius::same(10);
    let shadow_rect = rect.translate(Vec2::new(0.0, 10.0)).expand(10.0);
    ui.painter().rect_filled(
        shadow_rect,
        egui::CornerRadius::same(14),
        Color32::from_rgba_unmultiplied(2, 4, 7, 116),
    );
    ui.painter().rect_filled(rect, radius, kit::PANEL_RAISED);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0_f32, kit::MODAL_STROKE),
        egui::StrokeKind::Inside,
    );

    if attention {
        let time = ui.input(|input| input.time);
        let pulse = ((time * std::f64::consts::TAU / 1.6).sin() as f32 + 1.0) * 0.5;
        let alpha = (42.0 + pulse * 92.0).round() as u8;
        ui.painter().rect_stroke(
            rect.expand(1.0),
            radius,
            Stroke::new(
                1.0_f32,
                Color32::from_rgba_unmultiplied(244, 127, 45, alpha),
            ),
            egui::StrokeKind::Inside,
        );
    }
}

pub(super) fn queue_header(
    ui: &mut Ui,
    rect: Rect,
    job_count: usize,
    has_clearable: bool,
    clear_clicked: &mut bool,
    close_clicked: &mut bool,
) {
    let mut header_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    header_ui.set_min_size(rect.size());
    header_ui.shrink_clip_rect(rect);

    let count_label = if job_count == 0 {
        "Empty".to_string()
    } else {
        job_count.to_string()
    };
    header_ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add_sized(
            [112.0, 16.0],
            egui::Label::new(
                RichText::new("Generation Queue")
                    .color(kit::TEXT)
                    .size(12.0),
            )
            .truncate(),
        );
        ui.add_sized(
            [112.0, 12.0],
            egui::Label::new(
                RichText::new(count_label.to_ascii_uppercase())
                    .color(kit::TEXT_MUTED)
                    .size(10.0),
            )
            .truncate(),
        );
    });
    header_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if kit::popover_button(ui, "Close", 50.0, true).clicked() {
            *close_clicked = true;
        }
        if kit::popover_button(ui, "Clear All", 68.0, has_clearable).clicked() {
            *clear_clicked = true;
        }
    });
}

pub(super) fn queue_body(
    ui: &mut Ui,
    rect: Rect,
    jobs: &[GenerationJob],
    cancel_job_id: &mut Option<Uuid>,
) {
    let mut body_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    body_ui.set_min_size(rect.size());
    body_ui.shrink_clip_rect(rect);
    body_ui.set_width(rect.width());
    body_ui.set_height(rect.height());

    kit::clipped_scroll_body(&mut body_ui, "generation_queue_body", |ui| {
        ui.spacing_mut().item_spacing.y = QUEUE_JOB_GAP;
        if jobs.is_empty() {
            queue_empty_state(ui);
        } else {
            for job in jobs.iter().rev() {
                if queue_job_card(ui, job) {
                    *cancel_job_id = Some(job.id);
                }
            }
        }
    });
}

fn queue_empty_state(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), QUEUE_EMPTY_BODY_H),
        Sense::hover(),
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        Stroke::new(1.0_f32, kit::BORDER_SOFT),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "No generation jobs yet.",
        FontId::proportional(11.0),
        kit::TEXT_DIM,
    );
}

fn queue_job_card(ui: &mut Ui, job: &GenerationJob) -> bool {
    let height = queue_job_height(job);
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let radius = egui::CornerRadius::same(8);
    ui.painter().rect_filled(rect, radius, kit::PANEL);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0_f32, kit::BORDER_SOFT),
        egui::StrokeKind::Inside,
    );

    let content = rect.shrink(10.0);
    let presentation = queue_operation_presentation(job);
    let output_label = queue_output_label(job.output_type);
    let status_w = kit::operation_status_pill_width(presentation.phase);
    let title_rect = Rect::from_min_max(
        content.left_top(),
        Pos2::new(content.right() - status_w - 8.0, content.top() + 18.0),
    );
    let status_rect = Rect::from_min_size(
        Pos2::new(content.right() - status_w, content.top()),
        Vec2::new(status_w, 18.0),
    );
    queue_clipped_label(ui, title_rect, &job.asset_label, kit::TEXT, 12.0, true);
    kit::paint_operation_status_pill(ui, status_rect, presentation.phase, presentation.severity);

    let meta_y = content.top() + 24.0;
    let provider_rect = Rect::from_min_size(
        Pos2::new(content.left(), meta_y),
        Vec2::new((content.width() - 94.0).max(0.0), 14.0),
    );
    let source_rect = Rect::from_min_size(
        Pos2::new(content.right() - 90.0, meta_y - 2.0),
        provider_source_badge_size(),
    );
    let output_rect = Rect::from_min_size(
        Pos2::new(content.right() - 52.0, meta_y),
        Vec2::new(52.0, 14.0),
    );
    queue_clipped_label(
        ui,
        provider_rect,
        &job.provider.name,
        kit::TEXT_MUTED,
        10.0,
        false,
    );
    paint_provider_source_badge(ui, source_rect, &job.provider);
    queue_clipped_label(ui, output_rect, output_label, kit::TEXT_DIM, 10.0, false);

    match job.status {
        GenerationJobStatus::Running => {
            let progress_rect = Rect::from_min_max(
                Pos2::new(content.left(), content.top() + 44.0),
                Pos2::new(content.right() - 60.0, content.bottom()),
            );
            queue_progress_rows(ui, progress_rect, job);
        }
        GenerationJobStatus::Canceling => {
            let cancel_rect = Rect::from_min_size(
                Pos2::new(content.left(), content.top() + 44.0),
                Vec2::new(content.width(), 30.0),
            );
            queue_clipped_label(ui, cancel_rect, "Canceling…", kit::TEXT_DIM, 10.0, false);
        }
        GenerationJobStatus::Failed => {
            let outcome_rect = Rect::from_min_size(
                Pos2::new(content.left(), content.top() + 44.0),
                Vec2::new(content.width(), 16.0),
            );
            queue_clipped_label(
                ui,
                outcome_rect,
                &presentation.title,
                kit::DANGER,
                10.5,
                true,
            );
            if let Some(detail) = presentation.detail.as_deref() {
                let detail_rect = Rect::from_min_size(
                    Pos2::new(content.left(), content.top() + 62.0),
                    Vec2::new((content.width() - 72.0).max(0.0), 16.0),
                );
                queue_clipped_label(ui, detail_rect, detail, kit::TEXT_MUTED, 10.0, false);
            }
            if let Some(technical_detail) = presentation.technical_detail.as_deref() {
                let trigger_rect = Rect::from_min_size(
                    Pos2::new(content.right() - 64.0, content.bottom() - 24.0),
                    Vec2::new(64.0, 24.0),
                );
                let mut trigger_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(trigger_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                trigger_ui.shrink_clip_rect(trigger_rect);
                let trigger = kit::popover_button(&mut trigger_ui, "Details", 64.0, true);
                let _ = kit::technical_details_popup(
                    &trigger,
                    ("generation_job", job.id),
                    technical_detail,
                );
            }
        }
        GenerationJobStatus::Queued
        | GenerationJobStatus::Succeeded
        | GenerationJobStatus::Canceled => {}
    }

    let mut cancel_clicked = false;
    if matches!(
        job.status,
        GenerationJobStatus::Queued | GenerationJobStatus::Running
    ) {
        let cancel_rect = Rect::from_min_size(
            Pos2::new(
                content.right() - 52.0,
                content.bottom() - kit::POPOVER_BUTTON_H,
            ),
            Vec2::new(52.0, kit::POPOVER_BUTTON_H),
        );
        let mut cancel_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(cancel_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        cancel_ui.shrink_clip_rect(cancel_rect);
        let cancel_response = kit::popover_button(&mut cancel_ui, "Cancel", 52.0, true);
        cancel_clicked = cancel_response.clicked();
    }
    let _ = response.on_hover_text(provider_identity_tooltip(&job.provider));
    cancel_clicked
}

fn queue_progress_rows(ui: &mut Ui, rect: Rect, job: &GenerationJob) {
    let row_h = 26.0;
    let mut row = 0usize;
    if let Some(overall) = job.progress_overall.as_ref() {
        queue_progress_row(
            ui,
            Rect::from_min_size(
                Pos2::new(rect.left(), rect.top() + row as f32 * row_h),
                Vec2::new(rect.width(), row_h),
            ),
            &overall.label,
            overall.progress,
            kit::PRIMARY,
        );
        row += 1;
    }
    if let Some(stage) = job.progress_stage.as_ref() {
        let stage_rect = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + row as f32 * row_h),
            Vec2::new(rect.width(), row_h),
        );
        if let Some(progress) = stage.progress {
            let label = stage
                .detail
                .as_deref()
                .map(|detail| format!("{} · {detail}", stage.label))
                .unwrap_or_else(|| stage.label.clone());
            queue_progress_row(ui, stage_rect, &label, progress, kit::MARKER);
        } else {
            queue_running_detail(ui, stage_rect, &stage.label, stage.detail.as_deref());
        }
        row += 1;
    }
    if row == 0 {
        queue_running_detail(ui, rect, "Running…", None);
    }
}

#[cfg(test)]
fn queue_determinate_lane_count(job: &GenerationJob) -> usize {
    usize::from(job.progress_overall.is_some())
        + usize::from(
            job.progress_stage
                .as_ref()
                .is_some_and(|stage| stage.progress.is_some()),
        )
}

fn queue_running_detail(ui: &mut Ui, rect: Rect, label: &str, detail: Option<&str>) {
    queue_clipped_label(ui, rect, label, kit::TEXT_MUTED, 10.0, false);
    if let Some(detail) = detail {
        let detail_rect = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + 14.0),
            Vec2::new(rect.width(), 12.0),
        );
        queue_clipped_label(ui, detail_rect, detail, kit::TEXT_DIM, 8.5, false);
    }
}

fn queue_progress_row(ui: &mut Ui, rect: Rect, label: &str, progress: f32, color: Color32) {
    let pct = (progress.clamp(0.0, 1.0) * 100.0).round() as u32;
    ui.painter().text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        label,
        FontId::proportional(9.0),
        kit::TEXT_DIM,
    );
    ui.painter().text(
        rect.right_top(),
        egui::Align2::RIGHT_TOP,
        format!("{pct}%"),
        FontId::proportional(9.0),
        kit::TEXT_DIM,
    );

    let track_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.top() + 15.0),
        Vec2::new(rect.width(), 6.0),
    );
    ui.painter()
        .rect_filled(track_rect, egui::CornerRadius::same(3), kit::PANEL_SUNKEN);
    let fill_rect = Rect::from_min_size(
        track_rect.min,
        Vec2::new(
            track_rect.width() * progress.clamp(0.0, 1.0),
            track_rect.height(),
        ),
    );
    ui.painter()
        .rect_filled(fill_rect, egui::CornerRadius::same(3), color);
}

fn queue_clipped_label(
    ui: &mut Ui,
    rect: Rect,
    text: &str,
    color: Color32,
    size: f32,
    strong: bool,
) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_min_size(rect.size());
    child.shrink_clip_rect(rect);
    let mut text = RichText::new(text).color(color).size(size);
    if strong {
        text = text.strong();
    }
    child.add_sized(rect.size(), egui::Label::new(text).truncate());
}

fn queue_operation_presentation(job: &GenerationJob) -> kit::OperationPresentation {
    use kit::{OperationPhase as Phase, OperationPresentation, OperationSeverity as Severity};

    match job.status {
        GenerationJobStatus::Queued => {
            OperationPresentation::new(Phase::Queued, Severity::Neutral, "Waiting to generate.")
        }
        GenerationJobStatus::Running => OperationPresentation::new(
            Phase::Running,
            Severity::Informational,
            "Generation in progress.",
        ),
        GenerationJobStatus::Canceling => OperationPresentation::new(
            Phase::Canceling,
            Severity::Informational,
            "Cancel request sent.",
        ),
        GenerationJobStatus::Succeeded => {
            OperationPresentation::new(Phase::Succeeded, Severity::Success, "Generation complete.")
        }
        GenerationJobStatus::Failed => {
            let mut presentation =
                OperationPresentation::new(Phase::Failed, Severity::Error, "Generation failed.")
                    .detail(format!("{} was not generated.", job.asset_label));
            if let Some(error) = job
                .error
                .as_deref()
                .filter(|error| !error.trim().is_empty())
            {
                presentation = presentation.technical_detail(error);
            }
            presentation
        }
        GenerationJobStatus::Canceled => {
            OperationPresentation::new(Phase::Canceled, Severity::Neutral, "Generation canceled.")
        }
    }
}

fn queue_output_label(output_type: ProviderOutputType) -> &'static str {
    match output_type {
        ProviderOutputType::Image => "Image",
        ProviderOutputType::Video => "Video",
        ProviderOutputType::Audio => "Audio",
    }
}

#[cfg(test)]
mod operation_mapping_tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn job(status: GenerationJobStatus, error: Option<&str>) -> GenerationJob {
        GenerationJob {
            id: Uuid::new_v4(),
            created_at: Utc::now(),
            status,
            progress_overall: None,
            progress_stage: None,
            attempts: 0,
            next_attempt_at: None,
            provider: crate::state::ProviderEntry::new(
                "Provider",
                ProviderOutputType::Image,
                crate::state::ProviderConnection::ComfyUi {
                    base_url: "http://127.0.0.1:8188".to_string(),
                    workflow_path: None,
                    manifest: None,
                },
            ),
            output_type: ProviderOutputType::Image,
            asset_id: Uuid::new_v4(),
            clip_id: None,
            asset_label: "Stargazers V07".to_string(),
            folder_path: PathBuf::new(),
            inputs: HashMap::new(),
            inputs_snapshot: HashMap::new(),
            media_bindings_snapshot: HashMap::new(),
            resolved_media_inputs: HashMap::new(),
            seed_advance: None,
            version: None,
            lab_node_id: None,
            activate_on_success: true,
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn queue_statuses_map_phase_and_severity_independently() {
        use kit::{OperationPhase as Phase, OperationSeverity as Severity};
        let cases = [
            (
                GenerationJobStatus::Queued,
                Phase::Queued,
                Severity::Neutral,
            ),
            (
                GenerationJobStatus::Running,
                Phase::Running,
                Severity::Informational,
            ),
            (
                GenerationJobStatus::Canceling,
                Phase::Canceling,
                Severity::Informational,
            ),
            (
                GenerationJobStatus::Succeeded,
                Phase::Succeeded,
                Severity::Success,
            ),
            (GenerationJobStatus::Failed, Phase::Failed, Severity::Error),
            (
                GenerationJobStatus::Canceled,
                Phase::Canceled,
                Severity::Neutral,
            ),
        ];
        for (status, phase, severity) in cases {
            let presentation = queue_operation_presentation(&job(status, None));
            assert_eq!(presentation.phase, phase);
            assert_eq!(presentation.severity, severity);
        }
    }

    #[test]
    fn running_jobs_render_only_progress_that_is_known() {
        let mut running = job(GenerationJobStatus::Running, None);
        assert_eq!(queue_determinate_lane_count(&running), 0);
        running.progress_overall = Some(crate::state::GenerationProgressLane {
            label: "Overall".to_string(),
            progress: 0.4,
        });
        assert_eq!(queue_determinate_lane_count(&running), 1);
        running.progress_stage = Some(crate::state::GenerationProgressStage {
            label: "Sampling".to_string(),
            progress: Some(0.5),
            detail: Some("Step 2 of 4".to_string()),
        });
        assert_eq!(queue_determinate_lane_count(&running), 2);
    }

    #[test]
    fn failed_job_keeps_raw_error_secondary_and_has_no_speculative_action() {
        let presentation = queue_operation_presentation(&job(
            GenerationJobStatus::Failed,
            Some("HTTP 500: no output file"),
        ));
        assert_eq!(presentation.title, "Generation failed.");
        assert!(presentation
            .detail
            .as_deref()
            .unwrap()
            .contains("Stargazers V07"));
        assert_eq!(
            presentation.technical_detail.as_deref(),
            Some("HTTP 500: no output file")
        );
        assert!(presentation.primary_action.is_none());
        assert!(presentation.secondary_action.is_none());
    }
}
impl LatentSlateApp {
    pub(super) fn queue_panel(&mut self, ctx: &Context) {
        let mut close_clicked = false;
        let mut clear_clicked = false;
        let mut cancel_job_id = None;
        let app_rect = ctx.content_rect();
        let fallback_anchor = Rect::from_min_size(
            Pos2::new(app_rect.right() - 72.0, app_rect.top() + 4.0),
            Vec2::new(62.0, kit::TOP_BAR_BUTTON_H),
        );
        let anchor = self.queue_button_rect.unwrap_or(fallback_anchor);
        let bounds = app_rect.shrink(QUEUE_PANEL_MARGIN);
        let panel_w = QUEUE_PANEL_W.min(bounds.width()).max(240.0);
        let jobs = self.editor.generation_queue.clone();
        let has_attention = jobs.iter().any(|job| {
            matches!(
                job.status,
                GenerationJobStatus::Queued
                    | GenerationJobStatus::Running
                    | GenerationJobStatus::Canceling
            )
        });
        let has_clearable = jobs.iter().any(|job| queue_job_is_terminal(job.status));
        let desired_body_h = queue_list_height(&jobs);
        let desired_h =
            QUEUE_PANEL_PAD * 2.0 + QUEUE_PANEL_HEADER_H + QUEUE_PANEL_GAP + desired_body_h;
        let max_h_by_window = (app_rect.height() - QUEUE_PANEL_MAX_APP_GAP).max(QUEUE_PANEL_MIN_H);
        let panel_top =
            (anchor.bottom() + QUEUE_PANEL_GAP).clamp(bounds.top(), bounds.bottom() - 24.0);
        let max_h_below = (bounds.bottom() - panel_top).max(QUEUE_PANEL_MIN_H);
        let panel_h = desired_h.clamp(
            QUEUE_PANEL_MIN_H,
            max_h_by_window.min(max_h_below).max(QUEUE_PANEL_MIN_H),
        );
        let max_x = (bounds.right() - panel_w).max(bounds.left());
        let panel_pos = Pos2::new(
            (anchor.right() - panel_w).clamp(bounds.left(), max_x),
            panel_top,
        );

        if kit::modal_scrim(ctx, "queue").clicked() {
            close_clicked = true;
        }

        egui::Area::new(egui::Id::new("generation_queue_popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(panel_pos)
            .show(ctx, |ui| {
                let (panel_rect, _) =
                    ui.allocate_exact_size(Vec2::new(panel_w, panel_h), Sense::hover());
                paint_queue_panel_shell(ui, panel_rect, has_attention);

                let content_rect = panel_rect.shrink(QUEUE_PANEL_PAD);
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(content_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                child.set_min_size(content_rect.size());
                child.shrink_clip_rect(content_rect);
                child.set_width(content_rect.width());

                let header_rect = Rect::from_min_size(
                    content_rect.min,
                    Vec2::new(content_rect.width(), QUEUE_PANEL_HEADER_H),
                );
                let body_rect = Rect::from_min_max(
                    Pos2::new(content_rect.left(), header_rect.bottom() + QUEUE_PANEL_GAP),
                    content_rect.right_bottom(),
                );

                queue_header(
                    &mut child,
                    header_rect,
                    jobs.len(),
                    has_clearable,
                    &mut clear_clicked,
                    &mut close_clicked,
                );
                queue_body(&mut child, body_rect, &jobs, &mut cancel_job_id);
            });

        if let Some(job_id) = cancel_job_id {
            let _ = self.cancel_generation_job(job_id);
        }
        if clear_clicked {
            let before = self.editor.generation_queue.len();
            self.editor
                .generation_queue
                .retain(|job| !queue_job_is_terminal(job.status));
            let cleared = before.saturating_sub(self.editor.generation_queue.len());
            self.editor.status = if cleared == 1 {
                "Cleared 1 completed generation job.".to_string()
            } else {
                format!("Cleared {cleared} completed generation jobs.")
            };
        }
        if close_clicked {
            self.editor.overlays.queue = false;
        }
    }
}
