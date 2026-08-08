from pathlib import Path
import re


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"Expected one match in {path}, found {count}: {old[:160]!r}")
    file.write_text(text.replace(old, new, 1))


def regex_once(path: str, pattern: str, replacement: str) -> None:
    file = Path(path)
    text = file.read_text()
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"Expected one regex match in {path}, found {count}: {pattern[:160]!r}")
    file.write_text(updated)


Path("src/egui_app/provider_identity.rs").write_text(r'''use std::hash::Hash;

use super::*;

const SOURCE_BADGE_W: f32 = 34.0;
const SOURCE_BADGE_H: f32 = 20.0;
const STATUS_BADGE_H: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    pub(super) fn short_label(self) -> &'static str {
        match self {
            Self::ComfyUi => "CU",
            Self::LatentSlateEngine => "LS",
            Self::OpenAi => "OA",
            Self::Xai => "xAI",
            Self::CustomHttp => "<>",
            Self::Other => "--",
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

    pub(super) fn color(self) -> Color32 {
        match self {
            Self::LatentSlateEngine => kit::PRIMARY,
            Self::ComfyUi => Color32::from_rgb(125, 151, 238),
            Self::OpenAi => Color32::from_rgb(186, 190, 196),
            Self::Xai => Color32::from_rgb(182, 118, 238),
            Self::CustomHttp => kit::AUDIO,
            Self::Other => kit::TEXT_MUTED,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EngineCatalogState {
    Live,
    CachedOffline,
    Unavailable,
    NotDiscovered,
    Disabled,
}

impl EngineCatalogState {
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
            Self::Live => kit::PRIMARY,
            Self::CachedOffline | Self::Unavailable => kit::MARKER,
            Self::NotDiscovered | Self::Disabled => kit::TEXT_MUTED,
        }
    }
}

pub(super) fn provider_source_kind(provider: &ProviderEntry) -> ProviderSourceKind {
    match &provider.connection {
        ProviderConnection::ComfyUi { .. } => ProviderSourceKind::ComfyUi,
        ProviderConnection::LatentSlateEngine { .. } => ProviderSourceKind::LatentSlateEngine,
        ProviderConnection::OpenAiImage { .. } => ProviderSourceKind::OpenAi,
        ProviderConnection::XaiImage { .. } | ProviderConnection::XaiVideo { .. } => {
            ProviderSourceKind::Xai
        }
        ProviderConnection::CustomHttp { .. } => ProviderSourceKind::CustomHttp,
    }
}

pub(super) fn provider_is_available(provider: &ProviderEntry) -> bool {
    match &provider.connection {
        ProviderConnection::LatentSlateEngine { available, .. } => *available,
        _ => true,
    }
}

pub(super) fn provider_unavailable_reason(provider: &ProviderEntry) -> Option<&str> {
    match &provider.connection {
        ProviderConnection::LatentSlateEngine {
            available: false,
            unavailable_reason,
            ..
        } => unavailable_reason.as_deref(),
        _ => None,
    }
}

fn unavailable_reason_is_cached_offline(reason: Option<&str>) -> bool {
    reason.is_some_and(|reason| {
        let reason = reason.to_ascii_lowercase();
        reason.contains("offline") || reason.contains("cached catalog")
    })
}

pub(super) fn provider_status(provider: &ProviderEntry) -> Option<(&'static str, Color32)> {
    match &provider.connection {
        ProviderConnection::LatentSlateEngine {
            available: true, ..
        } => Some(("LIVE", kit::PRIMARY)),
        ProviderConnection::LatentSlateEngine {
            available: false,
            unavailable_reason,
            ..
        } if unavailable_reason_is_cached_offline(unavailable_reason.as_deref()) => {
            Some(("OFFLINE", kit::MARKER))
        }
        ProviderConnection::LatentSlateEngine {
            available: false, ..
        } => Some(("UNAVAILABLE", kit::MARKER)),
        _ => None,
    }
}

pub(super) fn engine_catalog_state(
    providers: &[ProviderEntry],
    enabled: bool,
) -> EngineCatalogState {
    if !enabled {
        return EngineCatalogState::Disabled;
    }
    let engine_tools = providers
        .iter()
        .filter(|provider| provider_source_kind(provider) == ProviderSourceKind::LatentSlateEngine)
        .collect::<Vec<_>>();
    if engine_tools.is_empty() {
        return EngineCatalogState::NotDiscovered;
    }
    if engine_tools.iter().any(|provider| provider_is_available(provider)) {
        return EngineCatalogState::Live;
    }
    if engine_tools
        .iter()
        .all(|provider| unavailable_reason_is_cached_offline(provider_unavailable_reason(provider)))
    {
        EngineCatalogState::CachedOffline
    } else {
        EngineCatalogState::Unavailable
    }
}

fn provider_output_identity_color(output_type: ProviderOutputType) -> Color32 {
    match output_type {
        ProviderOutputType::Image => kit::IMAGE,
        ProviderOutputType::Video => kit::VIDEO,
        ProviderOutputType::Audio => kit::AUDIO,
    }
}

fn paint_text_badge(ui: &Ui, rect: Rect, label: &str, color: Color32) {
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(4),
        color.gamma_multiply(0.13),
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
        label,
        FontId::proportional(10.0),
        color,
    );
}

pub(super) fn paint_provider_source_kind_badge(
    ui: &Ui,
    rect: Rect,
    source: ProviderSourceKind,
) {
    paint_text_badge(ui, rect, source.short_label(), source.color());
}

pub(super) fn paint_provider_source_badge_for_provider(
    ui: &Ui,
    rect: Rect,
    provider: &ProviderEntry,
) {
    paint_provider_source_kind_badge(ui, rect, provider_source_kind(provider));
    if !provider_is_available(provider) {
        ui.painter().circle_filled(
            Pos2::new(rect.right() - 2.5, rect.top() + 2.5),
            3.0,
            kit::MARKER,
        );
    }
}

pub(super) fn provider_source_kind_badge(
    ui: &mut Ui,
    source: ProviderSourceKind,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(SOURCE_BADGE_W, SOURCE_BADGE_H), Sense::hover());
    paint_provider_source_kind_badge(ui, rect, source);
    response.on_hover_text(source.label())
}

pub(super) fn provider_source_badge(
    ui: &mut Ui,
    provider: &ProviderEntry,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(SOURCE_BADGE_W, SOURCE_BADGE_H), Sense::hover());
    paint_provider_source_badge_for_provider(ui, rect, provider);
    let mut hover = provider_source_kind(provider).label().to_string();
    if let Some((status, _)) = provider_status(provider) {
        hover.push_str(&format!(" · {status}"));
    }
    if let Some(reason) = provider_unavailable_reason(provider) {
        hover.push('\n');
        hover.push_str(reason);
    }
    response.on_hover_text(hover)
}

pub(super) fn provider_state_badge(
    ui: &mut Ui,
    label: &str,
    color: Color32,
    width: f32,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, STATUS_BADGE_H), Sense::hover());
    paint_text_badge(ui, rect, label, color);
    response
}

pub(super) fn provider_status_badge(
    ui: &mut Ui,
    provider: &ProviderEntry,
) -> Option<egui::Response> {
    provider_status(provider).map(|(label, color)| {
        let width = if label == "UNAVAILABLE" { 78.0 } else { 56.0 };
        let response = provider_state_badge(ui, label, color, width);
        if let Some(reason) = provider_unavailable_reason(provider) {
            response.on_hover_text(reason)
        } else {
            response
        }
    })
}

fn provider_row_tooltip(provider: &ProviderEntry) -> String {
    let output = match provider.output_type {
        ProviderOutputType::Image => "Image",
        ProviderOutputType::Video => "Video",
        ProviderOutputType::Audio => "Audio",
    };
    let mut text = format!(
        "{} · {} · {}",
        provider_source_kind(provider).label(),
        provider.resolved_workflow_kind().label(),
        output,
    );
    if let Some(description) = provider
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty())
    {
        text.push_str("\n\n");
        text.push_str(description);
    }
    if let Some(reason) = provider_unavailable_reason(provider) {
        text.push_str("\n\nUnavailable: ");
        text.push_str(reason);
    }
    text
}

pub(super) fn provider_identity_row(
    ui: &mut Ui,
    provider: &ProviderEntry,
    selected: bool,
    clickable: bool,
) -> egui::Response {
    let height = 30.0;
    let width = ui.available_width().max(210.0);
    let sense = if clickable { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), sense);
    let response = if clickable {
        crate::core::automation::instrument_response(
            response.on_hover_cursor(egui::CursorIcon::PointingHand),
            "button",
            Some(provider.name.clone()),
            true,
            false,
        )
    } else {
        response
    };

    let fill = if selected {
        kit::FIELD_BG_ACTIVE
    } else if response.hovered() {
        kit::PANEL_RAISED
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(3), fill);

    let content = rect.shrink2(Vec2::new(6.0, 5.0));
    let source_rect = Rect::from_min_size(
        content.left_top(),
        Vec2::new(SOURCE_BADGE_W, SOURCE_BADGE_H),
    );
    paint_provider_source_badge_for_provider(ui, source_rect, provider);

    let kind_label = provider.resolved_workflow_kind().short_label();
    let kind_w = 46.0;
    let kind_rect = Rect::from_min_size(
        Pos2::new(content.right() - kind_w, content.top() + 1.0),
        Vec2::new(kind_w, STATUS_BADGE_H),
    );
    paint_text_badge(
        ui,
        kind_rect,
        kind_label,
        provider_output_identity_color(provider.output_type),
    );

    let status = provider_status(provider);
    let status_w = status.map(|(label, _)| if label == "UNAVAILABLE" { 78.0 } else { 56.0 });
    let status_rect = status_w.map(|status_w| {
        Rect::from_min_size(
            Pos2::new(kind_rect.left() - 5.0 - status_w, kind_rect.top()),
            Vec2::new(status_w, STATUS_BADGE_H),
        )
    });
    if let (Some((label, color)), Some(status_rect)) = (status, status_rect) {
        paint_text_badge(ui, status_rect, label, color);
    }

    let text_left = source_rect.right() + 8.0;
    let text_right = status_rect
        .map(|rect| rect.left() - 7.0)
        .unwrap_or(kind_rect.left() - 7.0);
    let name_color = if provider_is_available(provider) {
        kit::TEXT
    } else {
        kit::TEXT_DIM
    };
    let text_clip = Rect::from_min_max(
        Pos2::new(text_left, rect.top()),
        Pos2::new(text_right.max(text_left), rect.bottom()),
    );
    ui.painter().with_clip_rect(text_clip).text(
        Pos2::new(text_left, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &provider.name,
        FontId::proportional(12.0),
        name_color,
    );

    response.on_hover_text(provider_row_tooltip(provider))
}

pub(super) fn provider_selector_row(
    ui: &mut Ui,
    provider: &ProviderEntry,
    selected: bool,
) -> egui::Response {
    provider_identity_row(ui, provider, selected, true)
}

pub(super) fn provider_readonly_row(
    ui: &mut Ui,
    provider: &ProviderEntry,
) -> egui::Response {
    provider_identity_row(ui, provider, false, false)
}

fn provider_none_selector_row(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let height = 30.0;
    let width = ui.available_width().max(210.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if selected {
        kit::FIELD_BG_ACTIVE
    } else if response.hovered() {
        kit::PANEL_RAISED
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(3), fill);
    let content = rect.shrink2(Vec2::new(6.0, 5.0));
    let source_rect = Rect::from_min_size(
        content.left_top(),
        Vec2::new(SOURCE_BADGE_W, SOURCE_BADGE_H),
    );
    paint_provider_source_kind_badge(ui, source_rect, ProviderSourceKind::Other);
    ui.painter().text(
        Pos2::new(source_rect.right() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        kit::TEXT_MUTED,
    );
    response
}

pub(super) fn provider_labeled_selector(
    ui: &mut Ui,
    label: &str,
    id_salt: impl Hash,
    selected_provider: Option<&ProviderEntry>,
    selected_text: impl Into<String>,
    providers: &[ProviderEntry],
    selected_id: &mut Option<Uuid>,
    none_label: &str,
) -> egui::Response {
    let selected_text = selected_text.into();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = kit::FIELD_LABEL_GAP;
        kit::field_label(ui, label);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = kit::FIELD_COMPOUND_GAP;
            let badge_response = if let Some(provider) = selected_provider {
                provider_source_badge(ui, provider)
            } else {
                provider_source_kind_badge(ui, ProviderSourceKind::Other)
            };
            let combo_response = kit::combo_field(
                ui,
                id_salt,
                selected_text,
                ui.available_width().max(80.0),
                |ui| {
                    if provider_none_selector_row(ui, none_label, selected_id.is_none()).clicked() {
                        *selected_id = None;
                        ui.close();
                    }
                    for provider in providers {
                        if provider_selector_row(ui, provider, *selected_id == Some(provider.id))
                            .clicked()
                        {
                            *selected_id = Some(provider.id);
                            ui.close();
                        }
                    }
                },
            );
            badge_response.union(combo_response)
        })
        .inner
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(connection: ProviderConnection) -> ProviderEntry {
        ProviderEntry::new("Test", ProviderOutputType::Image, connection)
    }

    #[test]
    fn source_kind_follows_connection_type() {
        assert_eq!(
            provider_source_kind(&provider(ProviderConnection::ComfyUi {
                base_url: "http://localhost".to_string(),
                workflow_path: None,
                manifest: None,
            })),
            ProviderSourceKind::ComfyUi,
        );
        assert_eq!(
            provider_source_kind(&provider(ProviderConnection::LatentSlateEngine {
                base_url: "http://localhost".to_string(),
                api_key: None,
                tool_key: "test.tool".to_string(),
                schema_revision: 1,
                schema_hash: "sha256:test".to_string(),
                available: true,
                unavailable_reason: None,
            })),
            ProviderSourceKind::LatentSlateEngine,
        );
    }

    #[test]
    fn engine_catalog_state_distinguishes_live_cached_and_missing() {
        let live = provider(ProviderConnection::LatentSlateEngine {
            base_url: "http://localhost".to_string(),
            api_key: None,
            tool_key: "test.live".to_string(),
            schema_revision: 1,
            schema_hash: "sha256:live".to_string(),
            available: true,
            unavailable_reason: None,
        });
        let cached = provider(ProviderConnection::LatentSlateEngine {
            base_url: "http://localhost".to_string(),
            api_key: None,
            tool_key: "test.cached".to_string(),
            schema_revision: 1,
            schema_hash: "sha256:cached".to_string(),
            available: false,
            unavailable_reason: Some("Engine is offline; loaded from cached catalog".to_string()),
        });

        assert_eq!(engine_catalog_state(&[live], true), EngineCatalogState::Live);
        assert_eq!(
            engine_catalog_state(&[cached], true),
            EngineCatalogState::CachedOffline,
        );
        assert_eq!(engine_catalog_state(&[], true), EngineCatalogState::NotDiscovered);
        assert_eq!(engine_catalog_state(&[], false), EngineCatalogState::Disabled);
    }
}
''')

replace_once(
    "src/egui_app.rs",
    "mod provider_builder;\nmod provider_modal;\n",
    "mod provider_builder;\nmod provider_identity;\nmod provider_modal;\n",
)
replace_once(
    "src/egui_app.rs",
    "use provider_builder::*;\nuse timeline_geometry::*;\n",
    "use provider_builder::*;\nuse provider_identity::*;\nuse timeline_geometry::*;\n",
)

regex_once(
    "src/egui_app/provider_builder.rs",
    r"#\[derive\(Clone, Copy, Debug, PartialEq, Eq\)\]\npub\(super\) enum ProviderSourceKind \{.*?\n\}\n\npub\(super\) fn provider_row\(",
    "pub(super) fn provider_row(",
)

replace_once(
    "src/egui_app/provider_modal.rs",
    '''        kit::card_panel(ui, card_h, |ui| {
            self.add_provider_controls(ui);

            ui.add_space(kit::ACTION_GAP);
''',
    '''        kit::card_panel(ui, card_h, |ui| {
            self.engine_catalog_summary(ui);

            ui.add_space(kit::ACTION_GAP);
            self.add_provider_controls(ui);

            ui.add_space(kit::ACTION_GAP);
''',
)
replace_once(
    "src/egui_app/provider_modal.rs",
    '                ui.label(kit::section_label("Installed"));\n',
    '                ui.label(kit::section_label("Local Providers"));\n',
)
replace_once(
    "src/egui_app/provider_modal.rs",
    '''                        "No providers yet",
                        "Create a provider or reload the local provider folder.",
''',
    '''                        "No local providers yet",
                        "Create a provider or reload the local provider folder. Engine tools are listed above.",
''',
)
replace_once(
    "src/egui_app/provider_modal.rs",
    '''    pub(super) fn add_provider_controls(&mut self, ui: &mut Ui) {
''',
    '''    pub(super) fn engine_catalog_summary(&mut self, ui: &mut Ui) {
        let settings = crate::providers::latentslate_engine::load_connection_settings();
        let engine_tools = self
            .editor
            .provider_entries
            .iter()
            .filter(|provider| {
                provider_source_kind(provider) == ProviderSourceKind::LatentSlateEngine
            })
            .cloned()
            .collect::<Vec<_>>();
        let state = engine_catalog_state(&engine_tools, settings.enabled);
        let available_count = engine_tools
            .iter()
            .filter(|provider| provider_is_available(provider))
            .count();
        let mut refresh_clicked = false;

        kit::sunken_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                provider_source_kind_badge(ui, ProviderSourceKind::LatentSlateEngine);
                ui.label(kit::value("LatentSlate Engine"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if kit::field_button(ui, "Refresh", 68.0).clicked() {
                        refresh_clicked = true;
                    }
                    provider_state_badge(ui, state.label(), state.color(), 104.0);
                });
            });
            ui.add_space(kit::FIELD_LABEL_GAP);
            ui.label(kit::caption(format!("Endpoint: {}", settings.base_url)));
            ui.label(kit::caption(match state {
                EngineCatalogState::Live => format!(
                    "{} tool(s) discovered; {} currently available.",
                    engine_tools.len(),
                    available_count,
                ),
                EngineCatalogState::CachedOffline => format!(
                    "{} cached tool(s) remain visible for project readability while the Engine is offline.",
                    engine_tools.len(),
                ),
                EngineCatalogState::Unavailable => format!(
                    "{} tool(s) were discovered, but the Engine reports them unavailable.",
                    engine_tools.len(),
                ),
                EngineCatalogState::NotDiscovered =>
                    "No Engine catalog has been discovered or cached yet.".to_string(),
                EngineCatalogState::Disabled =>
                    "The LatentSlate Engine connection is disabled in engine.json.".to_string(),
            }));

            if let Some(reason) = engine_tools
                .iter()
                .find_map(provider_unavailable_reason)
                .filter(|_| state != EngineCatalogState::Live)
            {
                ui.add_space(kit::FIELD_LABEL_GAP);
                ui.label(RichText::new(reason).color(kit::MARKER).size(11.0));
            }

            if !engine_tools.is_empty() {
                ui.add_space(kit::FIELD_LABEL_GAP);
                egui::CollapsingHeader::new(format!("Catalog tools ({})", engine_tools.len()))
                    .id_salt("latentslate_engine_catalog_tools")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = kit::FIELD_LABEL_GAP;
                        for provider in &engine_tools {
                            provider_readonly_row(ui, provider);
                        }
                    });
            }
        });

        if refresh_clicked {
            self.editor.refresh_providers();
            self.editor.status = format!("Refreshed LatentSlate Engine catalog at {}", settings.base_url);
        }
    }

    pub(super) fn add_provider_controls(&mut self, ui: &mut Ui) {
''',
)

replace_once(
    "src/egui_app/provider_builder.rs",
    '''    let text_width = (workflow_rect.left() - rect.left() - 8.0).max(24.0);

    paint_truncated_row_text_top(
        ui,
        Pos2::new(rect.left(), rect.top() + 2.0),
''',
    '''    let source_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.center().y - 9.0),
        Vec2::new(34.0, 18.0),
    );
    paint_provider_source_kind_badge(ui, source_rect, summary.source);
    let text_left = source_rect.right() + 8.0;
    let text_width = (workflow_rect.left() - text_left - 8.0).max(24.0);

    paint_truncated_row_text_top(
        ui,
        Pos2::new(text_left, rect.top() + 2.0),
''',
)
replace_once(
    "src/egui_app/provider_builder.rs",
    '''        Pos2::new(rect.left(), rect.bottom() - 2.0),
''',
    '''        Pos2::new(text_left, rect.bottom() - 2.0),
''',
)

regex_once(
    "src/egui_app/attributes_panel.rs",
    r'''            kit::field_label\(ui, "Provider"\);\n            kit::combo_field\(.*?\n            \);\n\n            if show_missing_provider''',
    '''            provider_labeled_selector(
                ui,
                "Provider",
                ("gen_provider", asset_id),
                selected_provider.as_ref(),
                provider_label,
                &compatible_providers,
                &mut next_provider_id,
                "None selected",
            );

            if show_missing_provider''',
)
replace_once(
    "src/egui_app/attributes_panel.rs",
    '''            } else if compatible_providers.is_empty() {
''',
    '''            } else if selected_provider
                .as_ref()
                .is_some_and(|provider| !provider_is_available(provider))
            {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(
                    RichText::new(
                        selected_provider
                            .as_ref()
                            .and_then(provider_unavailable_reason)
                            .unwrap_or("Selected provider is unavailable."),
                    )
                    .color(kit::MARKER)
                    .size(11.0),
                );
            } else if compatible_providers.is_empty() {
''',
)
regex_once(
    "src/egui_app/attributes_panel.rs",
    r'''\nfn provider_choice_menu_row\(ui: &mut Ui, provider: &ProviderEntry\) -> egui::Response \{.*\Z''',
    '''
fn provider_choice_menu_row(ui: &mut Ui, provider: &ProviderEntry) -> egui::Response {
    provider_identity_row(ui, provider, false, true)
}
''',
)

regex_once(
    "src/egui_app/asset_lab.rs",
    r'''                    let provider_label = selected_provider.*?                    \);\n                    if provider_choice != display_node.provider_id''',
    '''                    let provider_label = selected_provider
                        .as_ref()
                        .map(|provider| provider.name.clone())
                        .unwrap_or_else(|| "Select provider".to_string());
                    let mut provider_choice = display_node.provider_id;
                    provider_labeled_selector(
                        ui,
                        "Provider",
                        ("asset_lab_node_provider", node.id),
                        selected_provider.as_ref(),
                        provider_label,
                        &compatible_providers,
                        &mut provider_choice,
                        "None",
                    );
                    if let Some(provider) = selected_provider
                        .as_ref()
                        .filter(|provider| !provider_is_available(provider))
                    {
                        ui.add_space(kit::FIELD_LABEL_GAP);
                        ui.label(
                            RichText::new(
                                provider_unavailable_reason(provider)
                                    .unwrap_or("Selected provider is unavailable."),
                            )
                            .color(kit::MARKER)
                            .size(11.0),
                        );
                    }
                    if provider_choice != display_node.provider_id''',
)
replace_once(
    "src/egui_app/asset_lab.rs",
    '''            let node_label = node
                .provider_id
                .map(|id| asset_lab_provider_name(&self.editor.provider_entries, id))
                .unwrap_or_else(|| "Staged step".to_string());
''',
    '''            let node_provider = node.provider_id.and_then(|id| {
                self.editor
                    .provider_entries
                    .iter()
                    .find(|provider| provider.id == id)
            });
            let node_label = node_provider
                .map(|provider| provider.name.clone())
                .unwrap_or_else(|| "Staged step".to_string());
''',
)
replace_once(
    "src/egui_app/asset_lab.rs",
    '''                painter.text(
                    Pos2::new(text_left, text_top + 20.0),
                    egui::Align2::LEFT_TOP,
                    node_label,
                    FontId::proportional(11.0),
                    kit::TEXT_MUTED,
                );
''',
    '''                let provider_text_left = if let Some(provider) = node_provider {
                    let badge_rect = Rect::from_min_size(
                        Pos2::new(text_left, text_top + 19.0),
                        Vec2::new(30.0, 16.0),
                    );
                    paint_provider_source_badge_for_provider(ui, badge_rect, provider);
                    badge_rect.right() + 6.0
                } else {
                    text_left
                };
                painter.text(
                    Pos2::new(provider_text_left, text_top + 20.0),
                    egui::Align2::LEFT_TOP,
                    node_label,
                    FontId::proportional(11.0),
                    kit::TEXT_MUTED,
                );
''',
)

replace_once(
    "src/egui_app/project_modals.rs",
    '''        "{} of {} installed providers visible to this project and the Agent API.",
''',
    '''        "{} of {} provider tools visible to this project and the Agent API.",
''',
)
replace_once(
    "src/egui_app/project_modals.rs",
    '''        ui.label(kit::caption("No providers installed yet."));
''',
    '''        ui.label(kit::caption("No provider tools are currently discovered."));
''',
)
regex_once(
    "src/egui_app/project_modals.rs",
    r'''fn provider_scope_row\(ui: &mut Ui, provider: &ProviderEntry, enabled: &mut bool\) \{.*?\n\}\n\nfn provider_output_label''',
    '''fn provider_scope_row(ui: &mut Ui, provider: &ProviderEntry, enabled: &mut bool) {
    ui.horizontal(|ui| {
        let label = format!("Enable {}", provider.name);
        let _ = automation_checkbox(ui, enabled, "");
        provider_source_badge(ui, provider);

        let output_w = 58.0;
        let status_w = provider_status(provider)
            .map(|(status, _)| if status == "UNAVAILABLE" { 78.0 } else { 56.0 })
            .unwrap_or(0.0);
        let status_gap = if status_w > 0.0 {
            kit::FIELD_COMPOUND_GAP
        } else {
            0.0
        };
        let reserved = output_w + status_w + status_gap + kit::FIELD_COMPOUND_GAP * 2.0;
        let name_w = (ui.available_width() - reserved).max(52.0);
        ui.add_sized(
            [name_w, kit::FIELD_H],
            egui::Label::new(kit::body(provider.name.clone())).truncate(),
        )
        .on_hover_text(label);
        let _ = provider_status_badge(ui, provider);
        kit::media_pill_sized(
            ui,
            provider_output_label(provider),
            kit::TEXT_MUTED,
            output_w,
        );
    });
}

fn provider_output_label''',
)

replace_once(
    "docs/PROVIDERS.md",
    '''Engine tools are read-only in LatentSlate. Their schemas are owned by the
Engine and refreshed from its catalog rather than edited as local provider JSON.
''',
    '''Engine tools are read-only in LatentSlate. Their schemas are owned by the
Engine and refreshed from its catalog rather than edited as local provider JSON.

Provider selectors and project scope rows show compact source badges (`LS`, `CU`,
`OA`, `xAI`, and `<>`) so identically named workflow categories remain distinguishable.
The AI Providers screen also reports the Engine endpoint and whether its catalog is
live, cached while offline, unavailable, disabled, or not yet discovered. Cached
Engine tools remain visible for project readability but are marked offline and cannot
run until the Engine reconnects and the catalog is refreshed.
''',
)
