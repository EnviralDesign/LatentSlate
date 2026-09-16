//! Responses wire format; the editor and chat UI continue to use the shared ChatEvent contract.
use super::agent_chat::{ChatEvent, ToolCall, ToolFunction};
use super::agent_context::{self, ContextUsage};
use super::agent_openai;
use crate::state::{AgentConnection, AgentProviderEntry, OpenAiAuth};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::mpsc;

pub struct Completion {
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub output: Vec<Value>,
}

pub fn history_owner(provider: &AgentProviderEntry) -> String {
    // Encrypted reasoning belongs to the original account/endpoint and model.
    format!(
        "{}:{:x}",
        provider.id,
        Sha256::digest(serde_json::to_vec(&provider.connection).unwrap_or_default())
    )
}

fn input(
    provider: &AgentProviderEntry,
    messages: &[Value],
) -> Result<(String, Vec<Value>), String> {
    let mut instructions = Vec::new();
    let mut input = Vec::new();
    let owner = history_owner(provider);
    for message in messages {
        let role = message["role"].as_str().unwrap_or("user");
        if role == "system" {
            if let Some(text) = message["content"].as_str() {
                instructions.push(text);
            }
            continue;
        }
        if role == "tool" {
            input.push(json!({"type":"function_call_output", "call_id":message["tool_call_id"], "output":message["content"]}));
            continue;
        }
        if role == "assistant" && message["responses_owner"] == owner {
            if let Some(items) = message["responses_output"].as_array() {
                input.extend(items.iter().cloned());
                continue;
            }
        }
        let mut content = Vec::new();
        let text_type = if role == "assistant" {
            "output_text"
        } else {
            "input_text"
        };
        if let Some(text) = message["content"].as_str().filter(|s| !s.is_empty()) {
            content.push(json!({"type":text_type, "text":text}));
        } else if let Some(parts) = message["content"].as_array() {
            for part in parts {
                match part["type"].as_str() {
                    Some("text") => content.push(json!({"type":text_type,"text":part["text"]})),
                    Some("image_url") if provider.capabilities.image_input => {
                        content.push(json!({"type":"input_image", "image_url":part["image_url"]["url"]}));
                    }
                    Some("image_url") => content.push(json!({"type":"input_text","text":"Earlier image omitted for this model's input capabilities."})),
                    Some("input_video") => return Err("This Responses endpoint does not accept native video. Select Chat Completions for a compatible video agent.".into()),
                    _ => return Err("Unsupported Responses message content.".into()),
                }
            }
        }
        if !content.is_empty() {
            input.push(json!({"role":role, "content":content}));
        }
        if let Some(calls) = message["tool_calls"].as_array() {
            for call in calls {
                input.push(json!({"type":"function_call", "call_id":call["id"], "name":call["function"]["name"], "arguments":call["function"]["arguments"]}));
            }
        }
    }
    // Keep visible history; the latest native compaction item owns the earlier model context.
    if let Some(index) = input.iter().rposition(|item| item["type"] == "compaction") {
        input.drain(..index);
    }
    Ok((instructions.join("\n\n"), input))
}

pub async fn completion(
    client: &reqwest::Client,
    provider: &AgentProviderEntry,
    messages: &[Value],
    tools: &[Value],
    events: &mpsc::Sender<ChatEvent>,
    limit: Option<u64>,
) -> Result<Completion, String> {
    let model = provider.connection.model().trim();
    if model.is_empty() {
        return Err("Configure an agent model first.".into());
    }
    let (instructions, input) = input(provider, messages)?;
    let tools: Vec<_> = tools
        .iter()
        .map(|tool| {
            let mut function = tool["function"].clone();
            function["type"] = json!("function");
            function["strict"] = json!(false);
            function
        })
        .collect();
    let mut body = json!({"model":model,"instructions":instructions,"input":input,"tools":tools,"stream":true,"store":false});
    let native_openai = matches!(provider.connection, AgentConnection::OpenAi { .. });
    if native_openai {
        let threshold =
            agent_context::COMPACT_AT.min(limit.unwrap_or(agent_context::OPENAI_CEILING) * 3 / 4);
        body["context_management"] = json!([{"type":"compaction", "compact_threshold":threshold}]);
    } else if let Some(tokens) =
        agent_context::local_input_tokens(client, provider, &body, true).await
    {
        let full = limit
            .is_some_and(|max| tokens.saturating_add(agent_context::LOCAL_REPLY_RESERVE) >= max);
        let _ = events.send(ChatEvent::Context(ContextUsage {
            tokens: Some(tokens),
            limit,
            full,
            preflight: true,
            ..Default::default()
        }));
        if full {
            return Err(agent_context::FULL_MESSAGE.into());
        }
    }
    if let AgentConnection::OpenAi {
        reasoning_effort: Some(effort),
        ..
    } = &provider.connection
    {
        body["reasoning"] = json!({"effort": effort});
    }
    let mut request = match &provider.connection {
        AgentConnection::OpenAiCompatible {
            base_url, api_key, ..
        } => {
            body["max_output_tokens"] = json!(4096);
            let mut request = client.post(agent_openai::endpoint(base_url, "responses")?);
            if let Some(key) = api_key.as_deref().filter(|s| !s.trim().is_empty()) {
                request = request.bearer_auth(key.trim());
            }
            request
        }
        AgentConnection::OpenAi {
            auth: OpenAiAuth::ApiKey,
            api_key,
            ..
        } => {
            body["include"] = json!(["reasoning.encrypted_content"]);
            client
                .post(format!("{}/responses", agent_openai::API_BASE))
                .bearer_auth(
                    api_key
                        .as_deref()
                        .filter(|s| !s.trim().is_empty())
                        .ok_or("Enter an OpenAI API key first.")?
                        .trim(),
                )
        }
        AgentConnection::OpenAi { .. } => {
            body["include"] = json!(["reasoning.encrypted_content"]);
            let account = agent_openai::account(provider.id).await?;
            client
                .post(format!("{}/responses", agent_openai::CODEX_BASE))
                .bearer_auth(account.access_token)
                .header("ChatGPT-Account-Id", account.account_id)
                .header("originator", "latentslate")
                .header("OpenAI-Beta", "responses=experimental")
        }
    };
    let body = serde_json::to_vec(&body).map_err(|_| "Unable to encode Responses request.")?;
    if body.len() > 48 * 1024 * 1024 {
        return Err("Chat request exceeds the 48 MiB total limit. Request fewer or smaller media inspections.".into());
    }
    request = request
        .header("Accept", "text/event-stream")
        .header("Content-Type", "application/json")
        .body(body);
    let mut response = request.send().await.map_err(|_| {
        "Could not connect to the Responses endpoint. Check the connection and model."
    })?;
    if !response.status().is_success() {
        let status = response.status();
        if let Ok(value) = agent_openai::limited_json(response).await {
            if agent_context::context_error(&value) {
                let _ = events.send(ChatEvent::Context(ContextUsage {
                    limit,
                    full: true,
                    ..Default::default()
                }));
                return Err(agent_context::FULL_MESSAGE.into());
            }
        }
        let hint = match status.as_u16() {
            401 | 403 => "Check the API key, or reconnect your ChatGPT account.",
            429 => "The account's usage or rate limit was reached. No alternative billing route was used.",
            404 => "Check the model and endpoint. For older local servers, select Chat Completions.",
            _ => "Check model access and Responses support on this connection.",
        };
        return Err(format!(
            "Responses request failed (HTTP {}). {hint}",
            status.as_u16()
        ));
    }
    let mut decoder = ResponsesDecoder::default();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Responses stream was interrupted.")?
    {
        for text in decoder.push(&chunk)? {
            let _ = events.send(ChatEvent::Text(text));
        }
        for compacting in std::mem::take(&mut decoder.compaction_events) {
            let _ = events.send(ChatEvent::Compacting(compacting));
        }
        if decoder.output.is_some() {
            break;
        }
    }
    let mut context = ContextUsage::reported(&decoder.usage, limit, !native_openai);
    if decoder.compacted {
        // Usage includes the pre-compaction window; it cannot measure the encrypted replacement.
        context.tokens = None;
        context.compacted = true;
        context.full = false;
    }
    let _ = events.send(ChatEvent::Context(context));
    decoder.finish()
}

#[derive(Default)]
struct ResponsesDecoder {
    pending: Vec<u8>,
    data: Vec<u8>,
    received: usize,
    text: String,
    output: Option<Vec<Value>>,
    completed_items: std::collections::BTreeMap<usize, Value>,
    usage: Value,
    compacted: bool,
    compaction_events: Vec<bool>,
}

impl ResponsesDecoder {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, String> {
        self.received += bytes.len();
        if self.received > 8 * 1024 * 1024 {
            return Err("Responses stream exceeded the size limit.".into());
        }
        self.pending.extend_from_slice(bytes);
        let mut emitted = Vec::new();
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let mut line: Vec<_> = self.pending.drain(..=end).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                if !self.data.is_empty() {
                    let data = std::mem::take(&mut self.data);
                    self.event(&data, &mut emitted)?;
                }
            } else if let Some(data) = line.strip_prefix(b"data:") {
                if !self.data.is_empty() {
                    self.data.push(b'\n');
                }
                self.data
                    .extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
            }
        }
        Ok(emitted)
    }

    fn event(&mut self, bytes: &[u8], emitted: &mut Vec<String>) -> Result<(), String> {
        if bytes == b"[DONE]" {
            return Ok(());
        }
        let event: Value =
            serde_json::from_slice(bytes).map_err(|_| "Invalid Responses streaming JSON.")?;
        match event["type"].as_str() {
            Some("response.output_item.added") if event["item"]["type"] == "compaction" => {
                self.compaction_events.push(true);
            }
            Some("response.output_item.done") => {
                let item = event.get("item").filter(|item| item.is_object())
                    .ok_or("Responses completed item is missing.")?;
                let index = event["output_index"].as_u64()
                    .map(|index| index as usize).unwrap_or(self.completed_items.len());
                self.completed_items.insert(index, item.clone());
                if item["type"] == "compaction" {
                    self.compacted = true;
                    self.compaction_events.push(false);
                }
            }
            Some("response.output_text.delta" | "response.refusal.delta") => {
                let delta = event["delta"].as_str().ok_or("Responses text delta is missing.")?;
                self.text.push_str(delta);
                emitted.push(delta.into());
            }
            Some("response.completed") => {
                self.usage = event["response"]["usage"].clone();
                if event["response"]["status"].as_str().is_some_and(|s| s != "completed") {
                    return Err("Agent response stopped before completion.".into());
                }
                let output = event["response"]["output"].as_array().filter(|items| !items.is_empty())
                    .cloned().unwrap_or_else(|| std::mem::take(&mut self.completed_items).into_values().collect());
                self.compacted |= output.iter().any(|item| item["type"] == "compaction");
                let final_text: String = output.iter().filter(|item| item["type"] == "message")
                    .filter_map(|item| item["content"].as_array()).flatten()
                    .filter_map(|part| part["text"].as_str().or(part["refusal"].as_str())).collect();
                if let Some(suffix) = final_text.strip_prefix(&self.text).filter(|s| !s.is_empty()) {
                    emitted.push(suffix.into());
                }
                self.text = final_text;
                self.output = Some(output);
            }
            Some("response.failed" | "response.incomplete" | "error") => return Err("Agent response failed or stopped before completion. Check account limits and try a smaller request.".into()),
            _ => {},
        }
        Ok(())
    }

    fn finish(self) -> Result<Completion, String> {
        // Use final output when supplied, or completed streamed items from Codex.
        // Never execute a partially streamed function or drop the reasoning items needed by the next request.
        let output = self
            .output
            .ok_or("Responses stream ended before completion.")?;
        let mut calls = Vec::new();
        let mut ids = std::collections::HashSet::new();
        for item in &output {
            if item["type"] == "function_call" {
                let id = item["call_id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or("Missing Responses tool call ID.")?;
                let name = item["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or("Missing Responses tool name.")?;
                if !ids.insert(id) || calls.len() >= 16 {
                    return Err("Duplicate or excessive Responses tool calls.".into());
                }
                calls.push(ToolCall {
                    id: id.into(),
                    kind: "function".into(),
                    function: ToolFunction {
                        name: name.into(),
                        arguments: item["arguments"]
                            .as_str()
                            .ok_or("Missing Responses tool arguments.")?
                            .into(),
                    },
                });
            }
        }
        if self.text.trim().is_empty() && calls.is_empty() {
            return Err("Agent completed without a reply or tool call. Please try again.".into());
        }
        Ok(Completion {
            text: self.text,
            calls,
            output,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_completed_items_survive_empty_final_output() {
        let mut decoder = ResponsesDecoder::default();
        let mut emitted = Vec::new();
        for event in [
            json!({"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","encrypted_content":"opaque","summary":[]}}),
            json!({"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","call_id":"call_1","name":"project_context","arguments":"{}"}}),
            json!({"type":"response.completed","response":{"status":"completed","output":[]}}),
        ] {
            decoder
                .event(&serde_json::to_vec(&event).unwrap(), &mut emitted)
                .unwrap();
        }
        let completion = decoder.finish().unwrap();
        assert_eq!(completion.calls[0].function.name, "project_context");
        assert_eq!(completion.output[0]["encrypted_content"], "opaque");
        let mut decoder = ResponsesDecoder::default();
        decoder
            .event(
                br#"{"type":"response.completed","response":{"status":"completed","output":[]}}"#,
                &mut emitted,
            )
            .unwrap();
        assert!(decoder.finish().is_err());
    }

    #[test]
    fn fragmented_llama_stream_without_indices_preserves_calls_and_reasoning() {
        let output = json!([
            {"type":"reasoning","id":"rs_1","summary":[],"encrypted_content":"opaque"},
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"Café"}]},
            {"type":"function_call","id":"fc_1","call_id":"call_1","name":"project_context","arguments":"{}"}
        ]);
        let events = [
            json!({"type":"response.output_text.delta","delta":"Café"}),
            json!({"type":"response.output_item.added","item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"project_context","arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","item_id":"fc_1","delta":"{}"}),
            json!({"type":"response.completed","response":{"status":"completed","output":output}}),
        ];
        let stream: String = events
            .iter()
            .map(|e| format!("data: {e}\r\n\r\n"))
            .collect();
        let mut decoder = ResponsesDecoder::default();
        let mut text = String::new();
        for byte in stream.as_bytes() {
            text.extend(decoder.push(&[*byte]).unwrap());
        }
        let complete = decoder.finish().unwrap();
        assert_eq!(text, "Café");
        assert_eq!(complete.calls[0].id, "call_1");
        assert_eq!(complete.calls[0].function.arguments, "{}");
        assert_eq!(complete.output, output.as_array().unwrap().clone());

        let provider = AgentProviderEntry::default();
        let history = vec![
            json!({"role":"assistant","content":complete.text,"tool_calls":complete.calls,
                "responses_owner":history_owner(&provider),"responses_output":complete.output}),
            json!({"role":"tool","tool_call_id":"call_1","content":"{\"ok\":true}"}),
            json!({"role":"user","content":[{"type":"text","text":"Image result"},{"type":"image_url","image_url":{"url":"data:image/png;base64,AA=="}}]}),
        ];
        let mut provider = provider;
        provider.capabilities.image_input = true;
        let (_, replay) = input(&provider, &history).unwrap();
        assert_eq!(replay[0]["encrypted_content"], "opaque");
        assert_eq!(replay[3]["type"], "function_call_output");
        assert_eq!(replay[4]["content"][1]["type"], "input_image");
        *provider.connection.model_mut() = "different-model".into();
        let (_, replay) = input(&provider, &history).unwrap();
        assert!(!replay.iter().any(|item| item["type"] == "reasoning"));
        assert_eq!(replay[1]["call_id"], "call_1");
    }

    #[test]
    fn compaction_replaces_wire_history_without_erasing_transcript() {
        let provider = AgentProviderEntry::default();
        let compact = json!({"type":"compaction","encrypted_content":"opaque"});
        let history = vec![
            json!({"role":"system","content":"Persistent instructions"}),
            json!({"role":"user","content":"Earlier request"}),
            json!({"role":"assistant","content":"Earlier answer","responses_owner":history_owner(&provider),
                "responses_output":[compact.clone()]}),
            json!({"role":"user","content":"Next request"}),
        ];
        let (instructions, replay) = input(&provider, &history).unwrap();
        assert_eq!(instructions, "Persistent instructions");
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0], compact);
        assert_eq!(history[1]["content"], "Earlier request");
        let mut decoder = ResponsesDecoder::default();
        let mut emitted = vec![];
        decoder
            .event(
                &serde_json::to_vec(
                    &json!({"type":"response.output_item.added","item":{"type":"compaction"}}),
                )
                .unwrap(),
                &mut emitted,
            )
            .unwrap();
        decoder
            .event(
                &serde_json::to_vec(
                    &json!({"type":"response.output_item.done","item":compact,"output_index":0}),
                )
                .unwrap(),
                &mut emitted,
            )
            .unwrap();
        assert_eq!(decoder.compaction_events, vec![true, false]);
        assert!(decoder.compacted);
    }

    #[test]
    fn incomplete_stream_never_executes_a_partial_tool() {
        let mut decoder = ResponsesDecoder::default();
        decoder.push(b"data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"save_project\",\"arguments\":\"{}\"}}\n\n").unwrap();
        assert!(decoder.finish().is_err());
        let mut decoder = ResponsesDecoder::default();
        assert!(decoder
            .push(b"data: {\"type\":\"response.incomplete\"}\n\n")
            .is_err());
    }
}
