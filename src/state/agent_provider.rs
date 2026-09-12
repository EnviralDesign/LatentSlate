use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An opt-in conversation provider, independent of generation workflows.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentProviderEntry {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub connection: AgentConnection,
    #[serde(default)]
    pub capabilities: AgentCapabilities,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentConnection {
    OpenAiCompatible {
        base_url: String,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
    },
}

#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct AgentCapabilities {
    pub image_input: bool,
    pub video_input: bool,
}

impl Default for AgentProviderEntry {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "OpenAI-compatible Agent".into(),
            enabled: true,
            connection: AgentConnection::OpenAiCompatible {
                base_url: "http://127.0.0.1:8080/v1".into(),
                model: String::new(),
                api_key: None,
            },
            capabilities: AgentCapabilities::default(),
        }
    }
}
