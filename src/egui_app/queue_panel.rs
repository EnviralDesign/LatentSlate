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
        Stroke::new(1.0, kit::MODAL_STROKE),
        egui::StrokeKind::Inside,
    );

    if attention {
        let time = ui.input(|input| input.time);
        let pulse = ((time * std::f64::consts::TAU / 1.6).sin() as f32 + 1.0) * 0.5;
        let alpha = (42.0 + pulse * 92.0).round() as u8;
        ui.painter().rect_stroke(
            rect.expand(1.0),
            radius,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(244, 127, 45, alpha)),
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
        Stroke::new(1.0, kit::BORDER_SOFT),
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
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let radius = egui::CornerRadius::same(8);
    ui.painter().rect_filled(rect, radius, kit::PANEL);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, kit::BORDER_SOFT),
        egui::StrokeKind::Inside,
    );

    let content = rect.shrink(10.0);
    let (status_label, status_color) = queue_status_style(job.status);
    let output_label = queue_output_label(job.output_type);
    let status_w = match job.status {
        GenerationJobStatus::Succeeded => 56.0,
        GenerationJobStatus::Running => 64.0,
        GenerationJobStatus::Failed => 60.0,
        GenerationJobStatus::Queued => 62.0,
        GenerationJobStatus::Canceled => 74.0,
    };
    let title_rect = Rect::from_min_max(
        content.left_top(),
        Pos2::new(content.right() - status_w - 8.0, content.top() + 18.0),
    );
    let status_rect = Rect::from_min_size(
        Pos2::new(content.right() - status_w, content.top()),
        Vec2::new(status_w, 18.0),
    );
    queue_clipped_label(ui, title_rect, &job.asset_label, kit::TEXT, 12.0, true);
    paint_queue_status_pill(ui, status_rect, status_label, status_color);

    let meta_y = content.top() + 24.0;
    let provider_rect = Rect::from_min_size(
        Pos2::new(content.left(), meta_y),
        Vec2::new((content.width() - 54.0).max(0.0), 14.0),
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
    queue_clipped_label(ui, output_rect, output_label, kit::TEXT_DIM, 10.0, false);

    match job.status {
        GenerationJobStatus::Running => {
            let workflow = job.progress_overall.unwrap_or(0.0).clamp(0.0, 1.0);
            let node = job.progress_node.unwrap_or(0.0).clamp(0.0, 1.0);
            let progress_rect = Rect::from_min_max(
                Pos2::new(content.left(), content.top() + 44.0),
                Pos2::new(content.right() - 60.0, content.bottom()),
            );
            queue_progress_rows(ui, progress_rect, workflow, node);
        }
        GenerationJobStatus::Failed => {
            if let Some(error) = job.error.as_ref() {
                let error_rect = Rect::from_min_size(
                    Pos2::new(content.left(), content.top() + 44.0),
                    Vec2::new(content.width(), 30.0),
                );
                queue_clipped_label(ui, error_rect, error, kit::DANGER, 10.0, false);
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
            Pos2::new(content.right() - 52.0, content.bottom() - 20.0),
            Vec2::new(52.0, 18.0),
        );
        let cancel_response = ui.interact(
            cancel_rect,
            ui.id().with(("generation-job-cancel", job.id)),
            Sense::click(),
        );
        let fill = if cancel_response.hovered() {
            Color32::from_rgba_unmultiplied(145, 32, 42, 92)
        } else {
            Color32::from_rgba_unmultiplied(145, 32, 42, 40)
        };
        ui.painter()
            .rect_filled(cancel_rect, egui::CornerRadius::same(5), fill);
        ui.painter().rect_stroke(
            cancel_rect,
            egui::CornerRadius::same(5),
            Stroke::new(1.0, kit::DANGER.gamma_multiply(0.75)),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            cancel_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Cancel",
            FontId::proportional(9.0),
            kit::TEXT,
        );
        cancel_clicked = cancel_response.clicked();
    }
    cancel_clicked
}

fn queue_progress_rows(ui: &mut Ui, rect: Rect, workflow: f32, node: f32) {
    let row_h = 26.0;
    queue_progress_row(
        ui,
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), row_h)),
        "Workflow",
        workflow,
        kit::PRIMARY,
    );
    queue_progress_row(
        ui,
        Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + row_h),
            Vec2::new(rect.width(), row_h),
        ),
        "Node",
        node,
        kit::MARKER,
    );
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

fn paint_queue_status_pill(ui: &mut Ui, rect: Rect, label: &str, color: Color32) {
    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 22);
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(9), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(9),
        Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label.to_ascii_uppercase(),
        FontId::proportional(9.0),
        color,
    );
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

fn queue_status_style(status: GenerationJobStatus) -> (&'static str, Color32) {
    match status {
        GenerationJobStatus::Queued => ("Queued", kit::TEXT_MUTED),
        GenerationJobStatus::Running => ("Running", kit::MARKER),
        GenerationJobStatus::Succeeded => ("Done", kit::PRIMARY_HOVER),
        GenerationJobStatus::Failed => ("Failed", kit::DANGER),
        GenerationJobStatus::Canceled => ("Canceled", kit::TEXT_DIM),
    }
}

fn queue_output_label(output_type: ProviderOutputType) -> &'static str {
    match output_type {
        ProviderOutputType::Image => "Image",
        ProviderOutputType::Video => "Video",
        ProviderOutputType::Audio => "Audio",
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
        let jobs = self.editor.generation_queue.clone();
        let has_attention = jobs.iter().any(|job| {
            matches!(
                job.status,
                GenerationJobStatus::Queued | GenerationJobStatus::Running
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
        let max_x = (bounds.right() - QUEUE_PANEL_W).max(bounds.left());
        let panel_pos = Pos2::new(
            (anchor.right() - QUEUE_PANEL_W).clamp(bounds.left(), max_x),
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
                    ui.allocate_exact_size(Vec2::new(QUEUE_PANEL_W, panel_h), Sense::hover());
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
