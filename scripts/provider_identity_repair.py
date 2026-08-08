from pathlib import Path
import re


def replace_once(path: Path, old: str, new: str, label: str) -> None:
    text = path.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"Expected one {label}, found {count}")
    path.write_text(text.replace(old, new, 1))


builder = Path("src/egui_app/provider_builder.rs")
text = builder.read_text()
text, count = re.subn(
    r"\nimpl ProviderSourceKind \{.*?\n\}\n\npub\(super\) fn provider_row\(",
    "\npub(super) fn provider_row(",
    text,
    count=1,
    flags=re.S,
)
if count > 1:
    raise SystemExit(f"Found more than one legacy ProviderSourceKind impl: {count}")
builder.write_text(text)

identity = Path("src/egui_app/provider_identity.rs")
replace_once(
    identity,
    "pub(super) fn provider_labeled_selector(",
    "pub(super) fn provider_labeled_selector<Id: Hash + 'static>(",
    "provider selector signature",
)
replace_once(
    identity,
    "    id_salt: impl Hash,",
    "    id_salt: Id,",
    "provider selector ID type",
)
replace_once(
    identity,
    "    provider_identity_row(ui, provider, selected, true)",
    "    provider_identity_row(ui, provider, selected, provider_is_available(provider))",
    "provider selector availability gate",
)
replace_once(
    identity,
    "            response.on_hover_cursor(egui::CursorIcon::PointingHand),",
    "            response.clone().on_hover_cursor(egui::CursorIcon::PointingHand),",
    "provider identity response clone",
)

attributes = Path("src/egui_app/attributes_panel.rs")
replace_once(
    attributes,
    "    provider_identity_row(ui, provider, false, true)",
    "    provider_identity_row(ui, provider, false, provider_is_available(provider))",
    "context provider availability gate",
)

asset_lab = Path("src/egui_app/asset_lab.rs")
replace_once(
    asset_lab,
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
    '''            let node_label = node
                .provider_id
                .map(|id| asset_lab_provider_name(&self.editor.provider_entries, id))
                .unwrap_or_else(|| "Staged step".to_string());
''',
    "Asset Lab node provider lookup",
)
replace_once(
    asset_lab,
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
    '''                painter.text(
                    Pos2::new(text_left, text_top + 20.0),
                    egui::Align2::LEFT_TOP,
                    node_label,
                    FontId::proportional(11.0),
                    kit::TEXT_MUTED,
                );
''',
    "Asset Lab graph-node badge block",
)
