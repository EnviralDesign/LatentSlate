//! One-shot Ideogram Magic Prompt expansion through configured agent providers.
//!
//! The pinned system prompt is the Ideogram OSS v1 single-shot captioner. Expansion
//! is a LatentSlate preprocess: Engine still encodes whatever `prompt` it receives.

use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

use super::agent_chat::{self, ChatRequest};
use crate::state::{
    AgentProviderEntry, GenerativeConfig, InputValue, MagicPromptProvenance, ProviderEntry,
};

const PINNED_V1: &str = include_str!("ideogram4_magic_prompt_v1.txt");

pub fn eligible_agents(agents: &[AgentProviderEntry]) -> Vec<&AgentProviderEntry> {
    agents
        .iter()
        .filter(|agent| agent.enabled && agent.capabilities.magic_prompt)
        .collect()
}

pub fn sync_selected_agent(selected: &mut Option<Uuid>, agents: &[AgentProviderEntry]) {
    let eligible = eligible_agents(agents);
    match *selected {
        Some(id) if eligible.iter().any(|agent| agent.id == id) => {}
        Some(_) => *selected = None,
        None if eligible.len() == 1 => *selected = Some(eligible[0].id),
        None => {}
    }
}

pub fn resolve_selected_agent<'a>(
    selected: Option<Uuid>,
    agents: &'a [AgentProviderEntry],
) -> Option<&'a AgentProviderEntry> {
    let eligible = eligible_agents(agents);
    match selected {
        Some(id) => eligible.into_iter().find(|agent| agent.id == id),
        None if eligible.len() == 1 => eligible.into_iter().next(),
        _ => None,
    }
}

pub fn provenance(agent: &AgentProviderEntry) -> MagicPromptProvenance {
    MagicPromptProvenance {
        agent_id: agent.id,
        agent_name: agent.name.clone(),
        model: agent.connection.model().to_string(),
    }
}

pub fn authored_prompt(config: &GenerativeConfig) -> String {
    config
        .inputs
        .get("prompt")
        .and_then(super::prompt_references::authored_text)
        .unwrap_or("")
        .trim()
        .to_string()
}

pub fn aspect_ratio_for_config(provider: &ProviderEntry, config: &GenerativeConfig) -> String {
    let mut values = HashMap::new();
    for (name, value) in &config.inputs {
        if let InputValue::Literal { value } = value {
            values.insert(name.clone(), value.clone());
        }
    }
    let (width, height) =
        super::generation::effective_canvas_dimensions(provider, &values).unwrap_or((1, 1));
    ratio(width.max(1) as u32, height.max(1) as u32)
}

pub fn start_expansion(
    provider: AgentProviderEntry,
    idea: &str,
    aspect_ratio: &str,
) -> ChatRequest {
    agent_chat::complete(
        provider,
        vec![
            json!({"role":"system","content":system_prompt()}),
            json!({"role":"user","content":user_message(aspect_ratio, idea)}),
        ],
    )
}

pub fn caption_from_completion(messages: &[Value]) -> Result<String, String> {
    let text = messages
        .iter()
        .rev()
        .find(|message| message["role"] == "assistant")
        .and_then(|message| message["content"].as_str())
        .ok_or_else(|| "Magic Prompt agent returned no caption.".to_string())?;
    extract_caption(text)
}

pub fn extract_caption(text: &str) -> Result<String, String> {
    let trimmed = strip_fences(text.trim());
    let start = trimmed
        .find('{')
        .ok_or_else(|| "Magic Prompt agent did not return JSON.".to_string())?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| "Magic Prompt agent did not return JSON.".to_string())?;
    if end < start {
        return Err("Magic Prompt agent did not return JSON.".into());
    }
    let json = &trimmed[start..=end];
    let parsed: Value = serde_json::from_str(json)
        .map_err(|_| "Magic Prompt agent returned invalid JSON.".to_string())?;
    validate_caption(&parsed)?;
    Ok(json.to_string())
}

fn validate_caption(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "Magic Prompt caption must be a JSON object.".to_string())?;
    if object
        .get("high_level_description")
        .and_then(Value::as_str)
        .map(|text| text.trim().is_empty())
        .unwrap_or(true)
    {
        return Err("Magic Prompt caption is missing high_level_description.".into());
    }
    let deconstruction = object
        .get("compositional_deconstruction")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            "Magic Prompt caption is missing compositional_deconstruction.".to_string()
        })?;
    if deconstruction
        .get("background")
        .and_then(Value::as_str)
        .is_none()
    {
        return Err("Magic Prompt caption is missing background.".into());
    }
    Ok(())
}

pub fn system_prompt() -> &'static str {
    pinned_section("SYSTEM")
}

fn user_message(aspect_ratio: &str, idea: &str) -> String {
    pinned_section("USER")
        .replace("{{aspect_ratio}}", aspect_ratio)
        .replace("{{original_prompt}}", idea)
}

fn pinned_section(name: &str) -> &'static str {
    let start_tag = match name {
        "SYSTEM" => "[SYSTEM]",
        "USER" => "[USER]",
        _ => return "",
    };
    let Some(start) = PINNED_V1.find(start_tag) else {
        return "";
    };
    let rest = &PINNED_V1[start + start_tag.len()..];
    let end = ["[META]", "[SYSTEM]", "[USER]"]
        .into_iter()
        .filter_map(|tag| rest.find(tag).filter(|&index| index > 0))
        .min()
        .unwrap_or(rest.len());
    rest[..end].trim()
}

fn strip_fences(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let rest = rest
        .strip_prefix("json")
        .or_else(|| rest.strip_prefix("JSON"))
        .unwrap_or(rest);
    rest.trim().trim_end_matches('`').trim()
}

fn ratio(width: u32, height: u32) -> String {
    let divisor = gcd(width, height);
    format!("{}:{}", width / divisor, height / divisor)
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AgentCapabilities, AgentConnection, AgentProtocol};

    fn agent(name: &str, enabled: bool, magic: bool) -> AgentProviderEntry {
        AgentProviderEntry {
            id: Uuid::new_v4(),
            name: name.into(),
            enabled,
            connection: AgentConnection::OpenAiCompatible {
                base_url: "http://127.0.0.1:8080/v1".into(),
                model: "fixture".into(),
                protocol: AgentProtocol::Responses,
                api_key: None,
            },
            capabilities: AgentCapabilities {
                image_input: false,
                video_input: false,
                magic_prompt: magic,
            },
        }
    }

    #[test]
    fn pinned_system_prompt_is_the_oss_captioner() {
        let prompt = system_prompt();
        assert!(prompt.contains("high_level_description"));
        assert!(prompt.contains("compositional_deconstruction"));
        assert!(!prompt.contains("[SYSTEM]"));
        assert!(!prompt.contains("[META]"));
        assert!(user_message("16:9", "a red mug").contains("16:9"));
        assert!(user_message("16:9", "a red mug").contains("a red mug"));
    }

    #[test]
    fn extract_caption_accepts_fenced_and_raw_json() {
        let caption = r#"{"high_level_description":"A red mug.","compositional_deconstruction":{"background":"a wooden table","elements":[]}}"#;
        assert_eq!(extract_caption(caption).unwrap(), caption);
        assert_eq!(
            extract_caption(&format!("Sure.\n```json\n{caption}\n```\n")).unwrap(),
            caption
        );
    }

    #[test]
    fn extract_caption_rejects_incomplete_objects() {
        assert!(extract_caption("a red mug").is_err());
        assert!(extract_caption(r#"{"high_level_description":"A red mug."}"#).is_err());
        assert!(extract_caption(
            r#"{"high_level_description":"","compositional_deconstruction":{"background":"x","elements":[]}}"#
        )
        .is_err());
    }

    #[test]
    fn stale_or_ambiguous_selection_does_not_fallback() {
        let local = agent("Local", true, true);
        let cloud = agent("Cloud", true, true);
        let disabled = agent("Off", false, true);
        let chat_only = agent("Chat", true, false);
        let agents = vec![local.clone(), cloud.clone(), disabled, chat_only];

        assert!(resolve_selected_agent(None, &agents).is_none());
        assert_eq!(
            resolve_selected_agent(Some(local.id), &agents)
                .unwrap()
                .id,
            local.id
        );
        assert!(resolve_selected_agent(Some(Uuid::new_v4()), &agents).is_none());

        let mut selected = Some(local.id);
        sync_selected_agent(&mut selected, &agents);
        assert_eq!(selected, Some(local.id));
        selected = Some(Uuid::new_v4());
        sync_selected_agent(&mut selected, &agents);
        assert_eq!(selected, None);

        let only = vec![local.clone()];
        selected = None;
        sync_selected_agent(&mut selected, &only);
        assert_eq!(selected, Some(local.id));
        assert_eq!(resolve_selected_agent(None, &only).unwrap().id, local.id);
    }

    #[test]
    fn aspect_ratio_reduces_canvas_dimensions() {
        assert_eq!(ratio(1024, 1024), "1:1");
        assert_eq!(ratio(1376, 768), "43:24");
        assert_eq!(ratio(1920, 1080), "16:9");
    }

    #[test]
    #[ignore]
    fn live_terra_agent_emits_ideogram_caption() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/release/LatentSlateData/providers/agents");
        let agents = crate::core::agent_provider_store::load_from(&root);
        let terra = agents
            .into_iter()
            .find(|agent| agent.name.eq_ignore_ascii_case("Terra API"))
            .expect("Terra API agent in release LatentSlateData");
        let request = start_expansion(terra, "a red mug on a wooden table", "1:1");
        let started = std::time::Instant::now();
        loop {
            match request.events.recv_timeout(std::time::Duration::from_millis(250)) {
                Ok(crate::core::agent_chat::ChatEvent::Finished { messages, error }) => {
                    if let Some(error) = error {
                        panic!("{error}");
                    }
                    let caption = caption_from_completion(&messages).expect("caption");
                    let value: Value = serde_json::from_str(&caption).unwrap();
                    let description = value["high_level_description"].as_str().unwrap();
                    assert!(!description.is_empty());
                    assert!(value
                        .pointer("/compositional_deconstruction/background")
                        .and_then(Value::as_str)
                        .is_some());
                    eprintln!(
                        "Terra Magic Prompt ok in {:.1}s; high_level_description {} chars",
                        started.elapsed().as_secs_f32(),
                        description.chars().count()
                    );
                    return;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    if started.elapsed() < std::time::Duration::from_secs(180) => {}
                Err(_) => panic!("Magic Prompt request closed"),
            }
        }
    }
}
