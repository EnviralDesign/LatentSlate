use std::hash::Hash;

use super::*;

const SOURCE_BADGE_H: f32 = 18.0;
const SOURCE_BADGE_W: f32 = 32.0;
const PROVIDER_CHOICE_H: f32 = 28.0;
const BADGE_GAP: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProviderSourceKind {
    ComfyUi,
    LatentSlateEngine,
    OpenAi,
    Xai,
    CustomHttp,
    Other,
}

impl ProviderSourceKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::ComfyUi => "ComfyUI",
            Self::LatentSlateEngine => "LatentSlate Engine",
            Self::OpenAi => "OpenAI",
            Self::Xai => "xAI",
            Self::CustomHttp => "Custom HTTP",
            Self::Other => "Other",
        }
    }

    pub(super) fn badge(self) -> &'static str {
        match self {
            Self::ComfyUi => "CU",
            Self::LatentSlateEngine => "LS",
            Self::OpenAi => "OA",
            Self::Xai => "xAI",
            Self::CustomHttp => "<>",
            Self::Other => "?",
        }
    }

    pub(super) fn sort_key(self) -> u8 {
        match self {
            Self::LatentSlateEngine => 0,
            Self::ComfyUi => 1,
            Self::OpenAi => 2,
            Self::Xai => 3,
            Self::CustomHttp => 4,
            Self::Other => 5,
        }
    }

    fn color(self) -> Color32 {
        match self {
            Self::LatentSlateEngine => kit::PRIMARY_HOVER,
            Self::ComfyUi => kit::IMAGE,
            Self::OpenAi => kit::TEXT,
            Self::Xai => kit::VIDEO,
            Self::CustomHttp => kit::MARKER,
            Self::Other => kit::TEXT_MUTED,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EngineProviderState {
    Live,
    CachedOffline,
    Unavailable,
    NotDiscovered,
    Disabled,
}

impl EngineProviderState {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Live => "LIVE",
            Self::CachedOffline => "CACHED OFFLINE",
            Self::Unavailable => "UNAVAILABLE",
            Self::NotDiscovered => "NOT DISCOVERED",
            Self::Disabled => "DISABLED",
        }
    }

    pub(super) fn color(self) -> Color32 {
        match self {
            Self::Live => kit::PRIMARY_HOVER,
            Self::CachedOffline => kit::MARKER,
            Self::Unavailable => kit::DANGER,
            Self::NotDiscovered | Self::Disabled => kit::TEXT_MUTED,
        }
    }

    pub(super) fn catalog_label(self) -> &'static str {
        match self {
            Self::Live => "Ready",
            Self::CachedOffline => "Offline",
            Self::Unavailable => "Blocked",
            Self::NotDiscovered => "Missing",
            Self::Disabled => "Disabled",
        }
    }
}

pub(super) fn provider_source_kind(connection: &ProviderConnection) -> ProviderSourceKind {
    match connection {
        ProviderConnection::ComfyUi { .. } => ProviderSourceKind::ComfyUi,
        ProviderConnection::LatentSlateEngine { .. } => ProviderSourceKind::LatentSlateEngine,
        ProviderConnection::OpenAiImage { .. } => ProviderSourceKind::OpenAi,
        ProviderConnection::XaiImage { .. } | ProviderConnection::XaiVideo { .. } => {
            ProviderSourceKind::Xai
        }
        ProviderConnection::CustomHttp { .. } => ProviderSourceKind::CustomHttp,
    }
}

pub(super) fn provider_is_available_for_generation(provider: &ProviderEntry) -> bool {
    !matches!(
        provider.connection,
        ProviderConnection::LatentSlateEngine {
            available: false,
            ..
        }
    )
}

pub(super) fn provider_unavailable_reason(provider: &ProviderEntry) -> Option<&str> {
    match &provider.connection {
        ProviderConnection::LatentSlateEngine {
            available: false,
            unavailable_reason,
            ..
        } => unavailable_reason
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty()),
        _ => None,
    }
}

fn is_cached_offline_reason(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    reason.contains("offline") && reason.contains("cached catalog")
}

pub(super) fn provider_engine_state(provider: &ProviderEntry) -> Option<EngineProviderState> {
    match &provider.connection {
        ProviderConnection::LatentSlateEngine {
            available: true, ..
        } => Some(EngineProviderState::Live),
        ProviderConnection::LatentSlateEngine {
            available: false,
            unavailable_reason,
            ..
        } => Some(
            unavailable_reason
                .as_deref()
                .filter(|reason| is_cached_offline_reason(reason))
                .map(|_| EngineProviderState::CachedOffline)
                .unwrap_or(EngineProviderState::Unavailable),
        ),
        _ => None,
    }
}

pub(super) fn engine_provider_state(
    connection_enabled: bool,
    providers: &[ProviderEntry],
) -> EngineProviderState {
    if !connection_enabled {
        return EngineProviderState::Disabled;
    }

    let engine_providers = providers
        .iter()
        .filter(|provider| {
            matches!(
                provider.connection,
                ProviderConnection::LatentSlateEngine { .. }
            )
        })
        .collect::<Vec<_>>();
    if engine_providers.is_empty() {
        return EngineProviderState::NotDiscovered;
    }
    if engine_providers
        .iter()
        .any(|provider| provider_is_available_for_generation(provider))
    {
        return EngineProviderState::Live;
    }
    if engine_providers
        .iter()
        .all(|provider| provider_engine_state(provider) == Some(EngineProviderState::CachedOffline))
    {
        EngineProviderState::CachedOffline
    } else {
        EngineProviderState::Unavailable
    }
}

pub(super) fn provider_identity_tooltip(provider: &ProviderEntry) -> String {
    let source = provider_source_kind(&provider.connection);
    let output = match provider.output_type {
        ProviderOutputType::Image => "Image",
        ProviderOutputType::Video => "Video",
        ProviderOutputType::Audio => "Audio",
    };
    let mut lines = vec![
        provider.name.clone(),
        format!("Source: {}", source.label()),
        format!("Category: {}", provider.resolved_workflow_kind().label()),
        format!("Output: {output}"),
    ];
    if let Some(state) = provider_engine_state(provider) {
        lines.push(format!("State: {}", state.label()));
    }
    if let Some(description) = provider
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty())
    {
        lines.push(String::new());
        lines.push(description.to_string());
    }
    if let Some(reason) = provider_unavailable_reason(provider) {
        let reason_already_in_description = provider
            .description
            .as_deref()
            .is_some_and(|description| description.contains(reason));
        if !reason_already_in_description {
            lines.push(String::new());
            lines.push(format!("Unavailable: {reason}"));
        }
    }
    lines.join("\n")
}

pub(super) fn provider_source_badge_size() -> Vec2 {
    Vec2::new(SOURCE_BADGE_W, SOURCE_BADGE_H)
}

pub(super) fn paint_provider_source_badge(ui: &Ui, rect: Rect, provider: &ProviderEntry) {
    let source = provider_source_kind(&provider.connection);
    let color = provider_engine_state(provider)
        .filter(|state| *state != EngineProviderState::Live)
        .map(EngineProviderState::color)
        .unwrap_or_else(|| source.color());
    paint_source_badge(ui, rect, source, color);
}

fn paint_source_badge(ui: &Ui, rect: Rect, source: ProviderSourceKind, color: Color32) {
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(4),
        color.gamma_multiply(0.14),
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, color.gamma_multiply(0.58)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        source.badge(),
        FontId::proportional(10.0),
        color,
    );
}

pub(super) fn provider_source_badge(ui: &mut Ui, provider: &ProviderEntry) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(provider_source_badge_size(), Sense::hover());
    paint_provider_source_badge(ui, rect, provider);
    response.on_hover_text(provider_identity_tooltip(provider))
}

pub(super) fn engine_state_badge_width(state: EngineProviderState) -> f32 {
    match state {
        EngineProviderState::Live => 44.0,
        EngineProviderState::CachedOffline => 94.0,
        EngineProviderState::Unavailable => 82.0,
        EngineProviderState::NotDiscovered => 98.0,
        EngineProviderState::Disabled => 64.0,
    }
}

pub(super) fn catalog_status_badge_width(state: EngineProviderState) -> f32 {
    match state {
        EngineProviderState::Live => 0.0,
        EngineProviderState::CachedOffline => 56.0,
        EngineProviderState::Unavailable => 60.0,
        EngineProviderState::NotDiscovered => 62.0,
        EngineProviderState::Disabled => 64.0,
    }
}

pub(super) fn paint_engine_state_badge(ui: &Ui, rect: Rect, state: EngineProviderState) {
    let color = state.color();
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(4),
        color.gamma_multiply(0.12),
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, color.gamma_multiply(0.54)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        state.label(),
        FontId::proportional(9.5),
        color,
    );
}

pub(super) fn engine_state_badge(ui: &mut Ui, state: EngineProviderState) -> egui::Response {
    let size = Vec2::new(engine_state_badge_width(state), SOURCE_BADGE_H);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    paint_engine_state_badge(ui, rect, state);
    response
}

pub(super) fn provider_combo_field<R>(
    ui: &mut Ui,
    id_salt: impl Hash,
    selected_provider: Option<&ProviderEntry>,
    placeholder: &str,
    width: f32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::Response {
    let automation_label = selected_provider
        .map(|provider| provider.name.clone())
        .unwrap_or_else(|| placeholder.to_string());
    let response = if let Some(provider) = selected_provider {
        kit::combo_field_with_leading(
            ui,
            id_salt,
            automation_label,
            width,
            SOURCE_BADGE_W,
            |ui, rect| paint_provider_source_badge(ui, rect, provider),
            add_contents,
        )
    } else {
        kit::combo_field(ui, id_salt, automation_label, width, add_contents)
    };
    if let Some(provider) = selected_provider {
        response.on_hover_text(provider_identity_tooltip(provider))
    } else {
        response
    }
}

pub(super) fn labeled_provider_combo_field<R>(
    ui: &mut Ui,
    label: &str,
    id_salt: impl Hash,
    selected_provider: Option<&ProviderEntry>,
    placeholder: &str,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = kit::FIELD_LABEL_GAP;
        kit::field_label(ui, label);
        provider_combo_field(
            ui,
            id_salt,
            selected_provider,
            placeholder,
            ui.available_width(),
            add_contents,
        )
    })
    .inner
}

pub(super) fn provider_selectable_value<T>(
    ui: &mut Ui,
    value: &mut T,
    selected_value: T,
    provider: &ProviderEntry,
) -> egui::Response
where
    T: PartialEq + Clone,
{
    let selected = *value == selected_value;
    let enabled = provider_is_available_for_generation(provider);
    let response = provider_choice_row(ui, provider, selected, enabled, "selectable", false);
    if response.clicked() && enabled {
        *value = selected_value;
        ui.close();
    }
    response
}

pub(super) fn provider_choice_button_row(ui: &mut Ui, provider: &ProviderEntry) -> egui::Response {
    provider_choice_row(
        ui,
        provider,
        false,
        provider_is_available_for_generation(provider),
        "button",
        true,
    )
}

fn provider_choice_row(
    ui: &mut Ui,
    provider: &ProviderEntry,
    selected: bool,
    enabled: bool,
    automation_kind: &'static str,
    show_workflow: bool,
) -> egui::Response {
    let width = ui.available_width().max(210.0);
    let (rect, raw_response) = ui.allocate_exact_size(
        Vec2::new(width, PROVIDER_CHOICE_H),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let response = crate::core::automation::instrument_response(
        raw_response.on_hover_cursor(if enabled {
            egui::CursorIcon::PointingHand
        } else {
            egui::CursorIcon::NotAllowed
        }),
        automation_kind,
        Some(provider.name.clone()),
        enabled,
        false,
    );

    let fill = if selected {
        kit::FIELD_BG_ACTIVE
    } else if response.hovered() && enabled {
        kit::PANEL_RAISED
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(3), fill);

    let content = rect.shrink2(Vec2::new(8.0, 5.0));
    let source_size = provider_source_badge_size();
    let source_rect = Rect::from_min_size(
        Pos2::new(
            content.right() - source_size.x,
            content.center().y - source_size.y * 0.5,
        ),
        source_size,
    );
    paint_provider_source_badge(ui, source_rect, provider);

    let mut right = source_rect.left() - BADGE_GAP;
    if let Some(state) = provider_engine_state(provider) {
        let state_w = engine_state_badge_width(state);
        let state_rect = Rect::from_min_size(
            Pos2::new(right - state_w, content.center().y - SOURCE_BADGE_H * 0.5),
            Vec2::new(state_w, SOURCE_BADGE_H),
        );
        paint_engine_state_badge(ui, state_rect, state);
        right = state_rect.left() - BADGE_GAP;
    }
    if show_workflow {
        let workflow = provider.resolved_workflow_kind().short_label();
        let workflow_w = (workflow.chars().count() as f32 * 6.0 + 14.0).clamp(38.0, 54.0);
        let workflow_rect = Rect::from_min_size(
            Pos2::new(
                right - workflow_w,
                content.center().y - SOURCE_BADGE_H * 0.5,
            ),
            Vec2::new(workflow_w, SOURCE_BADGE_H),
        );
        paint_compact_badge(
            ui,
            workflow_rect,
            workflow,
            provider_output_color(provider.output_type),
        );
        right = workflow_rect.left() - BADGE_GAP;
    }

    let name_color = if enabled { kit::TEXT } else { kit::TEXT_DIM };
    let name_width = (right - content.left()).max(24.0);
    paint_truncated_row_text_top(
        ui,
        Pos2::new(content.left(), content.center().y - 6.5),
        kit::value(&provider.name),
        12.0,
        name_width,
        name_color,
    );

    response.on_hover_text(provider_identity_tooltip(provider))
}

fn paint_compact_badge(ui: &Ui, rect: Rect, label: &str, color: Color32) {
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(4),
        color.gamma_multiply(0.12),
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, color.gamma_multiply(0.48)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(9.5),
        color,
    );
}

#[cfg(test)]
mod provider_identity_tests {
    use super::*;

    fn engine_provider(available: bool, reason: Option<&str>) -> ProviderEntry {
        ProviderEntry {
            id: Uuid::new_v4(),
            name: "Text to Video".to_string(),
            description: Some("Engine tool".to_string()),
            output_type: ProviderOutputType::Video,
            workflow_kind: ProviderWorkflowKind::TextToVideo,
            timeline_bridge: None,
            inputs: Vec::new(),
            connection: ProviderConnection::LatentSlateEngine {
                base_url: "http://127.0.0.1:8765".to_string(),
                api_key: None,
                tool_key: "video.text_to_video".to_string(),
                schema_revision: 1,
                schema_hash: "hash".to_string(),
                available,
                unavailable_reason: reason.map(str::to_string),
            },
        }
    }

    #[test]
    fn provider_identity_comes_from_connection_not_name() {
        let provider = ProviderEntry::new(
            "LatentSlate Engine-looking name",
            ProviderOutputType::Video,
            ProviderConnection::ComfyUi {
                base_url: "http://127.0.0.1:8188".to_string(),
                workflow_path: None,
                manifest: None,
            },
        );
        assert_eq!(
            provider_source_kind(&provider.connection),
            ProviderSourceKind::ComfyUi
        );
        assert_eq!(provider_source_kind(&provider.connection).badge(), "CU");
    }

    #[test]
    fn provider_identity_engine_state_covers_all_connection_states() {
        let live = engine_provider(true, None);
        assert_eq!(
            engine_provider_state(true, &[live]),
            EngineProviderState::Live
        );

        let cached = engine_provider(
            false,
            Some("LatentSlate Engine is offline; this tool was loaded from the cached catalog."),
        );
        assert_eq!(
            engine_provider_state(true, &[cached]),
            EngineProviderState::CachedOffline
        );

        let unavailable = engine_provider(false, Some("Required model bundle is not installed."));
        assert_eq!(
            engine_provider_state(true, &[unavailable]),
            EngineProviderState::Unavailable
        );
        assert_eq!(
            engine_provider_state(true, &[]),
            EngineProviderState::NotDiscovered
        );
        assert_eq!(
            engine_provider_state(false, &[]),
            EngineProviderState::Disabled
        );
    }

    #[test]
    fn provider_identity_unavailable_engine_tools_cannot_generate() {
        let cached = engine_provider(
            false,
            Some("LatentSlate Engine is offline; this tool was loaded from the cached catalog."),
        );
        assert!(!provider_is_available_for_generation(&cached));
        assert!(provider_identity_tooltip(&cached).contains("CACHED OFFLINE"));
        assert!(provider_identity_tooltip(&cached).contains("cached catalog"));
    }
}
