//! Context occupancy is per request, never accumulated billing usage.
use super::agent_openai;
use crate::state::{AgentConnection, AgentProviderEntry};
use serde_json::Value;

pub const OPENAI_CEILING: u64 = 272_000;
pub const COMPACT_AT: u64 = 200_000;
pub const LOCAL_REPLY_RESERVE: u64 = 4096;
pub const FULL_MESSAGE: &str = "Context full. Start a new chat.";

#[derive(Clone, Copy, Default)]
pub struct ContextUsage {
    pub tokens: Option<u64>,
    pub limit: Option<u64>,
    pub full: bool,
    pub compacted: bool,
    pub preflight: bool,
}

impl ContextUsage {
    pub fn reported(usage: &Value, limit: Option<u64>, local: bool) -> Self {
        let input = usage["input_tokens"]
            .as_u64()
            .or(usage["prompt_tokens"].as_u64());
        let output = usage["output_tokens"]
            .as_u64()
            .or(usage["completion_tokens"].as_u64());
        let tokens = input.zip(output).map(|(i, o)| i.saturating_add(o));
        Self {
            tokens,
            limit,
            full: local
                && tokens
                    .zip(limit)
                    .is_some_and(|(n, max)| n.saturating_add(LOCAL_REPLY_RESERVE) >= max),
            ..Default::default()
        }
    }
}

pub fn context_error(value: &Value) -> bool {
    let error = value.get("error").unwrap_or(value);
    ["type", "code"].iter().any(|key| {
        matches!(
            error[*key].as_str(),
            Some(
                "exceed_context_size_error" | "context_length_exceeded" | "context_window_exceeded"
            )
        )
    })
}

pub async fn limit(provider: &AgentProviderEntry) -> Option<u64> {
    let reported = agent_openai::models(provider)
        .await
        .ok()
        .and_then(|models| {
            models
                .into_iter()
                .find(|m| m.id == provider.connection.model())
        })
        .and_then(|m| m.context_window);
    if matches!(provider.connection, AgentConnection::OpenAi { .. }) {
        Some(reported.unwrap_or(OPENAI_CEILING).min(OPENAI_CEILING))
    } else {
        reported
    }
}

/// Router presets expose configured context without loading a cold model.
/// Do not confuse a model's trained context length with its server allocation.
pub fn configured_window(entry: &Value) -> Option<u64> {
    if let Some(n) = entry["context_window"].as_u64().filter(|n| *n > 0) {
        return Some(n);
    }
    let args = entry["status"]["args"].as_array()?;
    let arg = |names: &[&str]| -> Option<u64> {
        args.windows(2).find_map(|pair| {
            names
                .contains(&pair[0].as_str()?)
                .then(|| pair[1].as_str()?.parse().ok())
                .flatten()
        })
    };
    let context = arg(&["--ctx-size", "-c"])?;
    let parallel = arg(&["--parallel", "-np"]).unwrap_or(1);
    (context > 0 && parallel > 0).then(|| context / parallel)
}

pub async fn local_input_tokens(
    client: &reqwest::Client,
    provider: &AgentProviderEntry,
    body: &Value,
    responses: bool,
) -> Option<u64> {
    let AgentConnection::OpenAiCompatible {
        base_url, api_key, ..
    } = &provider.connection
    else {
        return None;
    };
    let path = if responses {
        "responses/input_tokens"
    } else {
        "chat/completions/input_tokens"
    };
    let mut url = agent_openai::endpoint(base_url, path).ok()?;
    url.query_pairs_mut().append_pair("autoload", "false");
    let mut request = client.post(url).json(body);
    if let Some(key) = api_key.as_deref().filter(|key| !key.trim().is_empty()) {
        request = request.bearer_auth(key.trim());
    }
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    agent_openai::limited_json(response).await.ok()?["input_tokens"].as_u64()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn occupancy_is_not_cumulative_and_reserves_reply_space() {
        let first = ContextUsage::reported(
            &json!({"prompt_tokens":5000,"completion_tokens":100}),
            Some(10000),
            true,
        );
        let second = ContextUsage::reported(
            &json!({"input_tokens":100,"output_tokens":10}),
            Some(10000),
            true,
        );
        assert_eq!(first.tokens, Some(5100));
        assert_eq!(second.tokens, Some(110));
        assert!(!first.full);
        assert!(
            ContextUsage::reported(
                &json!({"prompt_tokens":6000,"completion_tokens":1}),
                Some(10000),
                true
            )
            .full
        );
        assert!(ContextUsage::reported(&Value::Null, Some(10000), true)
            .tokens
            .is_none());
        assert_eq!(
            configured_window(&json!({"status":{"args":["--ctx-size","98304","--parallel","1"]}})),
            Some(98304)
        );
    }
}
