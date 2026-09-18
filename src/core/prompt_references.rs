//! Explicit prompt mentions bind to provider input keys, never source files or ordinal positions.
use std::collections::HashMap;
use std::ops::Range;

use crate::state::{InputValue, PromptReference, ProviderEntry, ProviderInputField};

pub fn preview_prompt(
    value: &InputValue,
    provider: &ProviderEntry,
    project: &crate::state::Project,
    config: &crate::state::GenerativeConfig,
    asset_id: Option<uuid::Uuid>,
    clip_id: Option<uuid::Uuid>,
) -> Result<String, Vec<String>> {
    let InputValue::Prompt { text, references } = value else {
        return Ok(authored_text(value).unwrap_or_default().to_string());
    };
    use crate::core::media_binding::{
        lookup_media_binding, resolve_media_binding, MediaResolveContext,
    };
    resolve_prompt(text, references, provider, |field| {
        lookup_media_binding(config, field, project).is_some_and(|spec| {
            resolve_media_binding(
                MediaResolveContext {
                    project,
                    target_asset_id: asset_id,
                    context_clip_id: clip_id,
                    field,
                    provider: Some(provider),
                    config: Some(config),
                },
                &spec,
            )
            .is_ok()
        })
    })
}

pub fn authored_text(value: &InputValue) -> Option<&str> {
    match value {
        InputValue::Prompt { text, .. } => Some(text),
        InputValue::Literal { value } => value.as_str(),
        _ => None,
    }
}

pub fn marker(label: &str) -> String {
    format!("@{{{label}}}")
}

/// Called only for an explicit prompt write, never while loading an old project.
/// Existing registrations win, including when a recipe or source has changed.
pub fn upgrade_written_prompt(
    value: InputValue,
    previous: Option<&InputValue>,
    provider: &ProviderEntry,
) -> InputValue {
    let Some(text) = authored_text(&value).map(str::to_string) else {
        return value;
    };
    let mut references = match &value {
        InputValue::Prompt { references, .. } => references.clone(),
        _ => match previous {
            Some(InputValue::Prompt { references, .. }) => references.clone(),
            _ => HashMap::new(),
        },
    };
    let old_literal = match previous {
        Some(InputValue::Literal { value }) => value.as_str(),
        _ => None,
    };
    let mut offset = 0;
    while let Some(start) = text[offset..].find("@{") {
        let start = offset + start;
        let Some(end) = text[start + 2..].find('}') else {
            break;
        };
        let end = start + 2 + end;
        let label = &text[start + 2..end];
        let token = &text[start..end + 1];
        if !references.contains_key(label) && !old_literal.is_some_and(|old| old.contains(token)) {
            let mut matching = provider.inputs.iter().filter(|field| {
                field.prompt_reference_token.is_some()
                    && (field.label.eq_ignore_ascii_case(label)
                        || field.name.eq_ignore_ascii_case(label))
            });
            if let Some(field) = matching.next().filter(|_| matching.next().is_none()) {
                references.insert(
                    label.to_string(),
                    PromptReference {
                        provider_id: provider.id,
                        input_name: field.name.clone(),
                    },
                );
            }
        }
        offset = end + 1;
    }
    if references.is_empty() {
        value
    } else {
        InputValue::Prompt { text, references }
    }
}

/// Shared discovery for the agent tools, API and desktop harness.
pub fn inspect_references(
    project: &crate::state::Project,
    config: &crate::state::GenerativeConfig,
    provider: &ProviderEntry,
    asset_id: uuid::Uuid,
    clip_id: Option<uuid::Uuid>,
) -> serde_json::Value {
    use crate::core::media_binding::{lookup_media_binding, source_menu_label};
    let source_details = crate::core::media_binding::inspect_config_media_bindings(
        project, asset_id, clip_id, provider, config,
    );
    let inputs: Vec<_> = provider.inputs.iter().filter(|field| field.prompt_reference_token.is_some()).map(|field| {
        let binding = lookup_media_binding(config, field, project);
        let reference = PromptReference { provider_id: provider.id, input_name: field.name.clone() };
        let value = InputValue::Prompt { text: marker(&field.name), references: HashMap::from([(field.name.clone(), reference.clone())]) };
        let resolved = preview_prompt(&value, provider, project, config, Some(asset_id), clip_id);
        serde_json::json!({"input_name":field.name,"label":field.label,"syntax":marker(&field.name),
            "reference":reference,"paired_video_input":field.paired_video_input,
            "source":binding.as_ref().map(|spec|source_menu_label(spec,project)),"binding":binding,"source_details":source_details.get(&field.name),
            "resolved_text":resolved.as_ref().ok(),"errors":resolved.err().unwrap_or_default()})
    }).collect();
    let prompts: HashMap<_,_> = config.inputs.iter().filter(|(_,value)|matches!(value,InputValue::Prompt{..})).map(|(name,value)| {
        let resolved = preview_prompt(value, provider, project, config, Some(asset_id), clip_id);
        (name,serde_json::json!({"authored":value,"resolved_text":resolved.as_ref().ok(),"errors":resolved.err().unwrap_or_default()}))
    }).collect();
    serde_json::json!({"syntax":"@{input_name} or @{Input label}; exact matches bind on explicit writes. Existing bindings retain their identity. Legacy literal prompts are unchanged on load.","inputs":inputs,"prompts":prompts})
}

pub fn submitted_prompts(
    snapshot: Option<&crate::state::AssetLabSnapshot>,
    submitted: &HashMap<String, InputValue>,
) -> serde_json::Value {
    let values: HashMap<_, _> = snapshot
        .into_iter()
        .flat_map(|snapshot| snapshot.inputs.iter())
        .filter(|(_, value)| authored_text(value).is_some())
        .map(|(name, value)| {
            (
                name,
                serde_json::json!({"authored":value,"submitted":submitted.get(name)}),
            )
        })
        .collect();
    serde_json::json!(values)
}

/// Keep authored prompt text in a submitted input map so continuation does not
/// replace the scene prompt with a serialized caption or resolved mentions.
pub fn overlay_authored_text(
    inputs: &mut HashMap<String, InputValue>,
    authored: &HashMap<String, InputValue>,
) {
    for (name, value) in authored {
        if authored_text(value).is_some() {
            inputs.insert(name.clone(), value.clone());
        }
    }
}

/// Retain registrations when text is deleted so the native text editor's undo/redo
/// and copy/paste within this prompt preserve the original reference identity.
pub fn insert_reference(
    text: &mut String,
    references: &mut HashMap<String, PromptReference>,
    replace: Range<usize>,
    provider: &ProviderEntry,
    input: &ProviderInputField,
) -> usize {
    let reference = PromptReference {
        provider_id: provider.id,
        input_name: input.name.clone(),
    };
    let base = input.label.replace(['{', '}'], "");
    let base = if base.trim().is_empty() {
        input.name.clone()
    } else {
        base
    };
    let label = if references.get(&base) == Some(&reference) {
        base
    } else {
        let mut label = base.clone();
        let mut suffix = 2;
        // Never reinterpret an existing literal occurrence when introducing a mention.
        while references.contains_key(&label) || text.contains(&marker(&label)) {
            label = format!("{base} ({suffix})");
            suffix += 1;
        }
        label
    };
    references.insert(label.clone(), reference);
    let inserted = marker(&label);
    let end = text[..replace.start].chars().count() + inserted.chars().count();
    text.replace_range(replace, &inserted);
    end
}

pub fn active_mentions<'a>(
    text: &'a str,
    references: &'a HashMap<String, PromptReference>,
) -> Vec<(Range<usize>, &'a str, &'a PromptReference)> {
    let mut found = Vec::new();
    let mut offset = 0;
    while let Some(start) = text[offset..].find("@{") {
        let start = offset + start;
        let Some(end) = text[start + 2..].find('}') else {
            break;
        };
        let end = start + 2 + end;
        let label = &text[start + 2..end];
        if let Some(reference) = references.get(label) {
            found.push((start..end + 1, label, reference));
        }
        offset = end + 1;
    }
    found
}

pub fn resolve_reference(
    reference: &PromptReference,
    provider: &ProviderEntry,
    occupied: impl Fn(&ProviderInputField) -> bool,
) -> Result<String, String> {
    if reference.provider_id != provider.id {
        return Err("belongs to another recipe. Reassign it to an input of this recipe.".into());
    }
    let input = provider
        .inputs
        .iter()
        .find(|input| input.name == reference.input_name)
        .ok_or("input is no longer available. Reassign or remove this reference.")?;
    let template = input
        .prompt_reference_token
        .as_deref()
        .filter(|token| !token.is_empty())
        .ok_or(
            "has no supported prompt notation in this recipe. Reassign or remove this reference.",
        )?;
    if !occupied(input) {
        return Err(format!(
            "{} needs a valid source. Choose its source or remove this reference.",
            input.label
        ));
    }
    let index = provider
        .inputs
        .iter()
        .take_while(|other| other.name != input.name)
        .filter(|other| {
            other.prompt_reference_token.as_deref() == Some(template) && occupied(other)
        })
        .count()
        + 1;
    Ok(template.replace("{index}", &index.to_string()))
}

pub fn resolve_prompt(
    text: &str,
    references: &HashMap<String, PromptReference>,
    provider: &ProviderEntry,
    occupied: impl Fn(&ProviderInputField) -> bool,
) -> Result<String, Vec<String>> {
    let mut result = String::new();
    let mut offset = 0;
    let mut errors = Vec::new();
    for (range, label, reference) in active_mentions(text, references) {
        result.push_str(&text[offset..range.start]);
        match resolve_reference(reference, provider, &occupied) {
            Ok(token) => result.push_str(&token),
            Err(error) => errors.push(format!("{} {error}", marker(label))),
        }
        offset = range.end;
    }
    result.push_str(&text[offset..]);
    errors.sort();
    errors.dedup();
    if errors.is_empty() {
        Ok(result)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::*;
    use serde_json::json;

    fn field(name: &str, token: &str) -> ProviderInputField {
        serde_json::from_value(json!({"name":name,"label":name,"input_type":{"type":"image"},"prompt_reference_token":token})).unwrap()
    }
    fn provider() -> ProviderEntry {
        let mut provider = crate::core::provider_store::default_openai_image_provider_entry();
        provider.inputs = vec![
            field("image1", "<Picture {index}>"),
            field("image3", "<Picture {index}>"),
            field("paired", "<Audio {index}>"),
            field("audio", "<Audio {index}>"),
        ];
        provider.inputs[2].paired_video_input = Some("video".into());
        provider
    }

    #[test]
    fn prompt_references_bind_exact_writes_keep_identity_and_use_final_occupied_order() {
        let provider = provider();
        let value = upgrade_written_prompt(
            InputValue::Literal {
                value: json!("Use @{image3}; hear @{audio} and @{paired}."),
            },
            None,
            &provider,
        );
        let InputValue::Prompt { text, references } = &value else {
            panic!("explicit references")
        };
        assert_eq!(
            resolve_prompt(text, references, &provider, |_| true).unwrap(),
            "Use <Picture 2>; hear <Audio 2> and <Audio 1>."
        );
        assert_eq!(
            resolve_prompt(text, references, &provider, |field| field.name != "image1").unwrap(),
            "Use <Picture 1>; hear <Audio 2> and <Audio 1>."
        );
        assert!(
            resolve_prompt(text, references, &provider, |field| field.name != "paired")
                .unwrap_err()[0]
                .contains("paired needs a valid source")
        );
        let just_audio = "Hear @{audio}";
        assert_eq!(
            resolve_prompt(just_audio, references, &provider, |field| field.name
                != "paired")
            .unwrap(),
            "Hear <Audio 1>"
        );
        let mut reordered = provider.clone();
        reordered.inputs.swap(0, 1);
        assert_eq!(
            resolve_prompt(text, references, &reordered, |_| true).unwrap(),
            "Use <Picture 1>; hear <Audio 2> and <Audio 1>."
        );
        reordered.id = uuid::Uuid::new_v4();
        let edited = upgrade_written_prompt(
            InputValue::Literal { value: json!(text) },
            Some(&value),
            &reordered,
        );
        assert_eq!(edited, value);
        assert!(resolve_prompt(text, references, &reordered, |_| true)
            .unwrap_err()
            .iter()
            .all(|error| error.contains("another recipe")));
        let encoded = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<InputValue>(&encoded).unwrap(), value);
    }

    #[test]
    fn prompt_references_preserve_literal_text_and_fixed_provider_notation() {
        let mut provider = provider();
        provider.inputs = vec![field("image1", "Picture 1"), field("image3", "Picture 3")];
        let old = InputValue::Literal {
            value: json!("Literal @{image3} and <Picture 2>"),
        };
        assert_eq!(
            upgrade_written_prompt(old.clone(), Some(&old), &provider),
            old
        );
        let value = upgrade_written_prompt(
            InputValue::Literal {
                value: json!("Use @{image3} with literal <Picture 2>"),
            },
            None,
            &provider,
        );
        let InputValue::Prompt { text, references } = value else {
            panic!("prompt")
        };
        assert_eq!(
            resolve_prompt(&text, &references, &provider, |f| f.name == "image3").unwrap(),
            "Use Picture 3 with literal <Picture 2>"
        );
        provider.inputs[1].prompt_reference_token = Some("image 2".into());
        assert_eq!(
            resolve_prompt(&text, &references, &provider, |f| f.name == "image3").unwrap(),
            "Use image 2 with literal <Picture 2>"
        );
        let mut text = "Literal @{image3}. @".to_string();
        let mut refs = HashMap::new();
        let start = text.len() - 1;
        insert_reference(
            &mut text,
            &mut refs,
            start..start + 1,
            &provider,
            &provider.inputs[1],
        );
        assert_eq!(text, "Literal @{image3}. @{image3 (2)}");
        assert_eq!(
            resolve_prompt(&text, &refs, &provider, |_| true).unwrap(),
            "Literal @{image3}. image 2"
        );
    }
}
