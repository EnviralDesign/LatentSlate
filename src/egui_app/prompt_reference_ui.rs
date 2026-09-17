use super::*;
use crate::core::media_binding::{
    lookup_media_binding, resolve_generation_context, resolve_media_binding, source_menu_label,
    MediaResolveContext,
};
use crate::core::prompt_references as mentions;
use crate::state::PromptReference;

#[derive(Clone, Default)]
struct Picker {
    open: bool,
    query: String,
    replace: std::ops::Range<usize>,
    selected: usize,
    cursor: usize,
    reassign: Option<String>,
    dismissed: Option<(String, usize)>,
}

fn byte_index(text: &str, character: usize) -> usize {
    text.char_indices()
        .nth(character)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn fuzzy_matches(haystack: &str, query: &str) -> bool {
    let mut chars = haystack.chars();
    query
        .chars()
        .all(|needle| chars.by_ref().any(|candidate| candidate == needle))
}

fn query_at(text: &str, cursor: usize) -> Option<(std::ops::Range<usize>, String)> {
    let end = byte_index(text, cursor);
    let start = text[..end].rfind('@')?;
    if start > 0
        && text[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }
    let query = &text[start + 1..end];
    if query.contains(['{', '}', '\n', '@']) || query.len() > 80 {
        return None;
    }
    Some((start..end, query.to_lowercase()))
}

impl LatentSlateApp {
    pub(super) fn prompt_reference_field(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        clip_id: Option<Uuid>,
        provider: &ProviderEntry,
        config: &GenerativeConfig,
        input: &ProviderInputField,
        label: &str,
    ) -> Option<InputValue> {
        ui.push_id(("prompt_mentions", asset_id, &input.name), |ui| {
            let original = config.inputs.get(&input.name).cloned().unwrap_or_else(|| InputValue::Literal {
                value: input.default.clone().unwrap_or_else(|| serde_json::json!("")),
            });
            let mut text = mentions::authored_text(&original).unwrap_or_default().to_string();
            let mut references = match &original {
                InputValue::Prompt { references, .. } => references.clone(),
                _ => HashMap::new(),
            };
            let id = ui.make_persistent_id("picker");
            let mut picker = ui.data(|data| data.get_temp::<Picker>(id).unwrap_or_default());
            if !ui.is_enabled() || !ui.memory(|memory| memory.allows_interaction(ui.layer_id())) {
                picker.open = false;
            }
            let mut accept = false;
            if picker.open {
                ui.input_mut(|i| {
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) { picker.selected += 1; }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) { picker.selected = picker.selected.saturating_sub(1); }
                    accept = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                        picker.open = false;
                        picker.dismissed = Some((text.clone(), picker.cursor));
                        picker.reassign = None;
                    }
                });
            }
            let context = resolve_generation_context(&self.editor.project, asset_id, clip_id, self.generation_context_by_asset.get(&asset_id).copied()).ok().flatten();
            let available: HashSet<_> = provider.inputs.iter().filter(|field| {
                lookup_media_binding(config, field, &self.editor.project).is_some_and(|spec| {
                    resolve_media_binding(MediaResolveContext {
                        project: &self.editor.project, target_asset_id: Some(asset_id),
                        context_clip_id: context, field, provider: Some(provider), config: Some(config),
                    }, &spec).is_ok()
                })
            }).map(|field| field.name.clone()).collect();
            let occupied = |field: &ProviderInputField| available.contains(&field.name);
            let active = mentions::active_mentions(&text, &references);
            let highlights: Vec<_> = active.iter().map(|(range, _, reference)| {
                (range.clone(), if mentions::resolve_reference(reference, provider, &occupied).is_ok() { kit::IMAGE } else { kit::DANGER })
            }).collect();
            let hover = active.iter().map(|(_, name, reference)| {
                format!("{} → {}", mentions::marker(name), mentions::resolve_reference(reference, provider, &occupied).unwrap_or_else(|error| error))
            }).collect::<Vec<_>>().join("\n");
            provider_input_field_label(ui, label, input);
            let (response, cursor) = kit::multiline_text_field_highlighted(ui, &mut text, ui.available_width(), kit::MultilineTextFieldOptions::rows(3), &highlights);
            if response.has_focus() {
                if let Some(cursor) = cursor { picker.cursor = cursor.primary.index; }
            }
            let mut changed = response.changed();
            if response.changed() && !response.has_focus() { picker.cursor = text.chars().count(); }
            if response.has_focus() || response.changed() {
                if let Some((range, query)) = query_at(&text, picker.cursor) {
                    if picker.dismissed.as_ref() != Some(&(text.clone(), picker.cursor)) {
                        if picker.query != query { picker.selected = 0; }
                        picker.open = true;
                        picker.query = query;
                        picker.replace = range;
                        picker.reassign = None;
                    }
                } else if picker.reassign.is_none() && response.changed() {
                    picker.open = false;
                }
            }
            if !hover.is_empty() { response.clone().on_hover_text(hover); }
            let supports = provider.inputs.iter().any(|field| field.prompt_reference_token.is_some());
            if supports || !references.is_empty() {
                if kit::secondary_button(ui, "@ Input", 76.0).on_hover_text("Reference a generation input. Type @ to search by input or source; use ↑/↓ and Enter to choose. References follow the input when its source changes.").clicked() {
                    picker.open = true;
                    picker.query.clear();
                    let index = byte_index(&text, picker.cursor);
                    picker.replace = index..index;
                    picker.reassign = None;
                }
                let preview = mentions::preview_prompt(&InputValue::Prompt { text: text.clone(), references: references.clone() }, provider, &self.editor.project, config, Some(asset_id), context);
                if let Err(errors) = &preview {
                    for error in errors { ui.label(kit::caption(error).color(kit::DANGER)); }
                }
                if !mentions::active_mentions(&text, &references).is_empty() {
                    egui::CollapsingHeader::new("Prompt references").id_salt("references").show(ui, |ui| {
                        let mut labels: Vec<_> = mentions::active_mentions(&text, &references).into_iter().map(|(_, label, _)| label.to_string()).collect();
                        labels.sort(); labels.dedup();
                        for label in labels {
                            let reference = &references[&label];
                            let target = mentions::resolve_reference(reference, provider, &occupied).unwrap_or_else(|_| "Unresolved".into());
                            kit::bounded_horizontal_row(ui, kit::SECONDARY_BUTTON_H, |ui, width| {
                                ui.add_sized([width - 76.0, kit::SECONDARY_BUTTON_H], egui::Label::new(format!("{} → {target}", mentions::marker(&label))).truncate());
                                if kit::secondary_button(ui, "Reassign", 68.0).clicked() {
                                    picker.open = true; picker.query.clear(); picker.selected = 0; picker.reassign = Some(label.clone());
                                }
                            });
                        }
                        if let Ok(preview) = preview { ui.label(kit::caption("Prompt to send")); ui.add(egui::Label::new(preview).selectable(true).wrap()); }
                    });
                }
            }
            let mut candidates: Vec<_> = provider.inputs.iter().filter(|field| field.prompt_reference_token.is_some()).filter(|field| {
                let source = lookup_media_binding(config, field, &self.editor.project).map(|spec| source_menu_label(&spec, &self.editor.project)).unwrap_or_default();
                fuzzy_matches(&format!("{} {source}", field.label).to_lowercase(), &picker.query)
            }).collect();
            candidates.sort_by_key(|field| !occupied(field));
            picker.selected = picker.selected.min(candidates.len().saturating_sub(1));
            let mut chosen = None;
            let mut open = picker.open;
            egui::Popup::menu(&response).id(id.with("popup")).open_bool(&mut open)
                .width(response.rect.width().max(260.0)).frame(kit::modal_frame().inner_margin(egui::Margin::same(8)))
                .show(|ui| {
                    ui.label(kit::caption(if picker.reassign.is_some() { "Reassign reference" } else { "Reference an input" }));
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        if candidates.is_empty() { ui.label(kit::caption("No supported inputs match.")); }
                        for (index, field) in candidates.iter().enumerate() {
                            let spec = lookup_media_binding(config, field, &self.editor.project);
                            let reference = PromptReference { provider_id: provider.id, input_name: field.name.clone() };
                            let token = mentions::resolve_reference(&reference, provider, &occupied);
                            let (preview, summary) = self.source_choice_preview(ui, asset_id, provider, field, context, config, spec.as_ref());
                            let source = spec.as_ref().map(|spec| source_menu_label(spec, &self.editor.project)).unwrap_or_else(|| "Choose a source first".into());
                            let subtitle = match &token { Ok(token) => format!("{source} · {token}"), Err(_) => source };
                            let row = ui.add_enabled_ui(spec.is_some(), |ui| {
                                kit::source_row(ui, (id, &field.name), &field.label, &subtitle, preview, None, index == picker.selected, ui.available_width())
                            }).inner.on_hover_text(summary);
                            if row.clicked() || (accept && index == picker.selected && spec.is_some()) { chosen = Some((*field).clone()); }
                        }
                    });
                });
            picker.open = open;
            if let Some(field) = chosen {
                if let Some(label) = picker.reassign.take() {
                    let ranges: Vec<_> = mentions::active_mentions(&text, &references).into_iter()
                        .filter(|(_, alias, _)| *alias == label).map(|(range, _, _)| range).collect();
                    for range in ranges.into_iter().rev() {
                        picker.cursor = mentions::insert_reference(&mut text, &mut references, range, provider, &field);
                    }
                } else {
                    picker.cursor = mentions::insert_reference(&mut text, &mut references, picker.replace.clone(), provider, &field);
                }
                changed = true;
                picker.open = false;
                picker.dismissed = Some((text.clone(), picker.cursor));
                response.request_focus();
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(picker.cursor))));
                    state.store(ui.ctx(), response.id);
                }
            }
            ui.data_mut(|data| data.insert_temp(id, picker));
            changed.then(|| mentions::upgrade_written_prompt(
                if references.is_empty() { InputValue::Literal { value: text.into() } } else { InputValue::Prompt { text, references } }, Some(&original), provider))
        }).inner
    }
}
