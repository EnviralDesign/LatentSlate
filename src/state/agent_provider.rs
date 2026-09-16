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
        #[serde(default)]
        protocol: AgentProtocol,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
    },
    OpenAi {
        model: String,
        #[serde(default)]
        auth: OpenAiAuth,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
    },
}

#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentProtocol {
    Responses,
    #[default]
    ChatCompletions,
}

#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiAuth {
    #[default]
    ChatGpt,
    ApiKey,
}

impl AgentConnection {
    pub fn model(&self) -> &str {
        match self {
            Self::OpenAiCompatible { model, .. } | Self::OpenAi { model, .. } => model,
        }
    }

    pub fn model_mut(&mut self) -> &mut String {
        match self {
            Self::OpenAiCompatible { model, .. } | Self::OpenAi { model, .. } => model,
        }
    }

    pub fn supports_native_video(&self) -> bool {
        matches!(
            self,
            Self::OpenAiCompatible {
                protocol: AgentProtocol::ChatCompletions,
                ..
            }
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::OpenAiCompatible {
                protocol: AgentProtocol::Responses,
                ..
            } => "Compatible · Responses",
            Self::OpenAiCompatible { .. } => "Compatible · Chat Completions",
            Self::OpenAi {
                auth: OpenAiAuth::ChatGpt,
                ..
            } => "OpenAI · ChatGPT subscription",
            Self::OpenAi { .. } => "OpenAI · API key",
        }
    }
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
                protocol: AgentProtocol::Responses,
                api_key: None,
            },
            capabilities: AgentCapabilities::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_connections_keep_chat_completions_and_video() {
        let connection: AgentConnection = serde_json::from_value(serde_json::json!({
            "kind":"open_ai_compatible", "base_url":"http://localhost:8080/v1", "model":"fixture"
        }))
        .unwrap();
        assert!(connection.supports_native_video());
        assert!(!AgentProviderEntry::default()
            .connection
            .supports_native_video());
        let connection = AgentConnection::OpenAi {
            model: "fixture".into(),
            auth: OpenAiAuth::ChatGpt,
            api_key: None,
        };
        assert!(!connection.supports_native_video());
    }
}
