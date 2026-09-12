use super::*;
use crate::core::{
    agent_chat::{ToolCall, ToolResult},
    agent_tools,
    automation::*,
};
use base64::Engine;
use serde_json::{json, Value};

pub(super) struct PendingChatMedia {
    pub receiver: std::sync::mpsc::Receiver<(ToolResult, Option<image::RgbaImage>)>,
    pub reply: tokio::sync::oneshot::Sender<ToolResult>,
    pub row: usize,
}

impl LatentSlateApp {
    pub(super) fn execute_chat_tool(
        &mut self,
        ctx: &Context,
        call: &ToolCall,
    ) -> Result<ToolResult, String> {
        let v: Value = serde_json::from_str(&call.function.arguments)
            .map_err(|_| "Invalid tool arguments.")?;
        self.chat.handles.sync(&self.editor);
        let name = call.function.name.as_str();
        let data = match name {
            "project_context" => self.chat.handles.context(&self.editor),
            "inspect" => self.chat.handles.inspect(&self.editor, &v)?,
            "generation" if v["action"] == "status" => {
                if v.get("asset").is_some() {
                    let id = self.chat.handles.id(&v, "asset", "a")?;
                    json!({"asset":self.chat.handles.inspect(&self.editor, &json!({"handle":v["asset"]}))?,"jobs":self.chat.handles.jobs(&self.editor,Some(id))})
                } else {
                    self.chat.handles.jobs(&self.editor, None)
                }
            }
            "look" => {
                let source = self.chat.handles.source(&v)?;
                let request = if v["mode"] == "frame" {
                    CaptureRequest::Frame {
                        source,
                        time: time(&v),
                        mode: CaptureMode::Normal,
                        format: "png".into(),
                        annotate: true,
                        seek_ui: false,
                        name: None,
                    }
                } else {
                    let frames = if let Some(times) = v.get("times") {
                        let times = times.as_array().ok_or("times must be an array")?;
                        if times.is_empty() || times.len() > 8 {
                            return Err("Choose 1–8 times.".into());
                        }
                        times
                            .iter()
                            .map(|t| {
                                t.as_f64()
                                    .filter(|n| n.is_finite() && *n >= 0.0)
                                    .map(|seconds| CaptureFrameRequest {
                                        label: None,
                                        time: Some(TimeSelector {
                                            seconds: Some(seconds),
                                            ..Default::default()
                                        }),
                                    })
                                    .ok_or("Invalid frame time".to_owned())
                            })
                            .collect::<Result<Vec<_>, _>>()?
                    } else {
                        (0..8)
                            .map(|i| CaptureFrameRequest {
                                label: None,
                                time: Some(TimeSelector {
                                    percent: Some(i as f64 / 8.0),
                                    ..Default::default()
                                }),
                            })
                            .collect()
                    };
                    CaptureRequest::Cutsheet {
                        source,
                        frames,
                        layout: CaptureSheetLayout {
                            columns: 4,
                            thumb_width: 768,
                        },
                        mode: CaptureMode::Normal,
                        format: "png".into(),
                        annotate: true,
                        seek_ui: false,
                        name: None,
                    }
                };
                let response = self.run_agent_capture(ctx, request);
                if !response.ok {
                    return Err(response.message.unwrap_or("Capture failed.".into()));
                }
                let path = response
                    .data
                    .pointer("/capture/path")
                    .and_then(Value::as_str)
                    .ok_or("Capture has no file")?;
                // Encoding is performed by a worker after this editor operation returns.
                return Ok(ToolResult{text:json!({"source":v.get("source").unwrap_or(&json!("timeline")),"kind":v.get("mode").unwrap_or(&json!("cutsheet")),"time":v.get("time"),"times":v.get("times")}).to_string(),media:vec![json!({"host_image_path":path})]});
            }
            "watch_video" => {
                let id = self.chat.handles.id(&v, "asset", "a")?;
                let asset = self
                    .editor
                    .project
                    .assets
                    .iter()
                    .find(|a| a.id == id)
                    .ok_or("Asset no longer exists")?;
                if !matches!(
                    asset.kind,
                    AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. }
                ) {
                    return Err("Asset must be a video.".into());
                }
                let root = self
                    .editor
                    .project
                    .project_path
                    .as_ref()
                    .ok_or("Open a project first.")?;
                let path = if let Some(version) = v.get("version").and_then(Value::as_str) {
                    crate::core::generation::generative_asset_source_path(
                        root,
                        asset,
                        Some(version),
                    )
                } else {
                    crate::core::generation::video_asset_source_path(root, asset)
                }
                .ok_or("Video output is unavailable.")?;
                return Ok(ToolResult{text:json!({"source":v["asset"],"version":v.get("version"),"duration_seconds":asset.duration_seconds,"kind":"native whole video"}).to_string(),media:vec![json!({"host_video_path":path})]});
            }
            "asset_edit" if v["action"] == "extract_still" => {
                let source = self.chat.handles.source(&v)?;
                let response = self.extract_agent_still_to_asset(
                    source,
                    time(&v),
                    agent_tools::optional(&v, "name"),
                );
                if !response.ok {
                    return Err(response.message.unwrap_or("Extract failed.".into()));
                }
                response.data
            }
            _ => {
                let command = self.chat.handles.command(name, &v)?;
                let response = match command {
                    AutomationCommand::StartGeneration {
                        asset_id,
                        context_clip_id,
                        wait,
                    } => self.start_agent_generation(asset_id, context_clip_id, wait),
                    AutomationCommand::SetActiveGenerationVersion { asset_id, version } => {
                        self.set_generative_active_version(asset_id, &version)?;
                        AutomationResponse::ok(json!({"asset_id":asset_id,"version":version}))
                    }
                    command => self.editor.apply_automation_command(&command),
                };
                if !response.ok {
                    return Err(response
                        .message
                        .unwrap_or("Project operation failed.".into()));
                }
                response.data
            }
        };
        self.chat.handles.sync(&self.editor);
        Ok(ToolResult {
            text: self.chat.handles.compact(data).to_string(),
            media: vec![],
        })
    }

    pub(super) fn start_agent_generation(
        &mut self,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        wait: bool,
    ) -> AutomationResponse {
        let before: std::collections::HashSet<_> =
            self.editor.generation_queue.iter().map(|j| j.id).collect();
        self.start_generative_generation(asset_id, context_clip_id);
        let jobs: Vec<_> = self
            .editor
            .generation_queue
            .iter()
            .filter(|j| !before.contains(&j.id))
            .cloned()
            .collect();
        if jobs.is_empty() {
            AutomationResponse::error(self.editor.status.clone())
        } else {
            AutomationResponse::ok(
                json!({"jobs":crate::editor::compact_generation_jobs_json(&jobs),"status":self.editor.status,"wait_requested":wait}),
            )
        }
    }
}

fn time(v: &Value) -> Option<TimeSelector> {
    v.get("time")
        .and_then(Value::as_f64)
        .map(|seconds| TimeSelector {
            seconds: Some(seconds),
            ..Default::default()
        })
}

pub(super) fn encode_media(mut result: ToolResult) -> ToolResult {
    let outcome = (|| -> Result<(), String> {
        for media in &mut result.media {
            let (path, video) =
                if let Some(p) = media.get("host_video_path").and_then(Value::as_str) {
                    (p, true)
                } else {
                    (
                        media
                            .get("host_image_path")
                            .and_then(Value::as_str)
                            .ok_or("Missing media path")?,
                        false,
                    )
                };
            let limit = 32 * 1024 * 1024;
            let file = std::fs::File::open(path).map_err(|_| "Unable to open media")?;
            if file
                .metadata()
                .map_err(|_| "Unable to inspect media")?
                .len()
                > limit
            {
                return Err("Media exceeds the 32 MiB Chat limit.".into());
            }
            use std::io::Read;
            let mut bytes = vec![];
            file.take(limit + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "Unable to read media")?;
            if bytes.len() as u64 > limit {
                return Err("Media exceeds the 32 MiB Chat limit.".into());
            }
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            *media = if video {
                json!({"type":"input_video","input_video":{"data":encoded}})
            } else {
                json!({"type":"image_url","image_url":{"url":format!("data:image/png;base64,{encoded}")}})
            };
        }
        Ok(())
    })();
    match outcome {
        Ok(()) => result,
        Err(e) => ToolResult::error(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_executes_capture_and_generation_without_agent_api() {
        assert!(!crate::core::automation::is_active());
        let ctx = Context::default();
        let mut app = LatentSlateApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        let root = std::env::temp_dir().join(format!("latentslate-chat-app-{}", Uuid::new_v4()));
        app.editor.project = crate::state::Project::new("Chat app fixture");
        app.editor.project.project_path = Some(root.clone());
        app.editor.project.settings.width = 64;
        app.editor.project.settings.height = 64;
        app.editor.provider_entries =
            vec![crate::core::provider_store::default_openai_image_provider_entry()];
        app.editor.save().unwrap();
        let call = |app: &mut LatentSlateApp, name: &str, args: Value| {
            app.execute_chat_tool(
                &ctx,
                &ToolCall {
                    id: "fixture".into(),
                    kind: "function".into(),
                    function: crate::core::agent_chat::ToolFunction {
                        name: name.into(),
                        arguments: args.to_string(),
                    },
                },
            )
            .unwrap()
        };
        call(
            &mut app,
            "asset_edit",
            json!({"action":"create_generative","output_type":"image","name":"Generated"}),
        );
        call(
            &mut app,
            "generation",
            json!({"action":"configure","asset":"a1","provider":"p1","inputs":{"prompt":"A blue square"}}),
        );
        call(
            &mut app,
            "generation",
            json!({"action":"start","asset":"a1"}),
        );
        assert_eq!(app.editor.generation_queue.len(), 1);
        assert_eq!(
            app.editor.generation_queue[0].asset_id,
            app.editor.project.assets[0].id
        );
        let status = call(&mut app, "generation", json!({"action":"status"}));
        assert!(status.text.contains("j1"));
        let image_path = root.join("fixture.png");
        image::RgbaImage::from_pixel(64, 64, image::Rgba([30, 70, 220, 255]))
            .save(&image_path)
            .unwrap();
        call(
            &mut app,
            "asset_edit",
            json!({"action":"import","path":image_path}),
        );
        app.editor.current_time = 1.25;
        let capture = call(
            &mut app,
            "look",
            json!({"source":"a2","mode":"frame","time":0}),
        );
        assert_eq!(app.editor.current_time, 1.25);
        let encoded = encode_media(capture);
        assert_eq!(encoded.media[0]["type"], "image_url");
        assert!(encoded.media[0]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
        call(&mut app, "save_project", json!({}));
        assert!(!app.editor.project_dirty);
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_video_attachment_preserves_encoded_bytes_and_enforces_size_limit() {
        let path = std::env::temp_dir().join(format!("latentslate-chat-{}.webm", Uuid::new_v4()));
        let bytes = b"encoded video fixture: no image extraction";
        std::fs::write(&path, bytes).unwrap();
        let request = || ToolResult {
            text: json!({"source":"a1","kind":"video"}).to_string(),
            media: vec![json!({"host_video_path":path})],
        };
        let result = encode_media(request());
        assert_eq!(result.media[0]["type"], "input_video");
        let encoded = result.media[0]["input_video"]["data"].as_str().unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
            bytes
        );
        assert!(!result.text.contains(encoded));
        assert!(!result.media[0].to_string().contains("image_url"));
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(32 * 1024 * 1024 + 1)
            .unwrap();
        let rejected = encode_media(request());
        assert!(rejected.media.is_empty());
        assert!(rejected.text.contains("32 MiB"));
        std::fs::remove_file(path).unwrap();
    }
}
