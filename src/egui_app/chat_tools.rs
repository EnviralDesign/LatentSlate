use super::*;
use crate::core::export::{
    TimestampOverlayPosition, TimestampOverlaySettings, VideoExportCodec, VideoExportEvent,
    VideoExportFrameFormat, VideoExportJob, VideoExportQuality, VideoExportSettings,
};
use crate::core::{
    agent_chat::{ToolCall, ToolResult},
    agent_tools,
    automation::*,
};
use base64::Engine;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub(super) struct ChatToolResult {
    pub result: ToolResult,
    pub video: Option<ChatVideo>,
}

impl From<ToolResult> for ChatToolResult {
    fn from(result: ToolResult) -> Self {
        Self {
            result,
            video: None,
        }
    }
}

pub(super) struct ChatVideo {
    pub project: Project,
    pub asset_path: Option<PathBuf>,
    pub start: f64,
    pub end: f64,
    pub output: PathBuf,
}

pub(super) struct PendingChatMedia {
    pub receiver: std::sync::mpsc::Receiver<(ToolResult, Option<image::RgbaImage>)>,
    pub reply: Option<tokio::sync::oneshot::Sender<ToolResult>>,
    pub row: usize,
    pub cancel: Arc<AtomicBool>,
}

impl Drop for PendingChatMedia {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl LatentSlateApp {
    pub(super) fn execute_chat_tool(
        &mut self,
        ctx: &Context,
        call: &ToolCall,
    ) -> Result<ChatToolResult, String> {
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
                if v.get("version").is_some() && !matches!(source, CaptureSource::Asset { .. }) {
                    return Err("version applies only to assets.".into());
                }
                let image_asset = match &source {
                    CaptureSource::Asset { asset_id, .. } => {
                        self.editor.project.find_asset(*asset_id).is_some_and(|a| {
                            matches!(
                                a.kind,
                                AssetKind::Image { .. } | AssetKind::GenerativeImage { .. }
                            )
                        })
                    }
                    _ => false,
                };
                let mode = v["mode"].as_str().unwrap_or(
                    if v.get("time").is_some() || v.get("frame").is_some() {
                        "frame"
                    } else if v.get("times").is_some() {
                        "cutsheet"
                    } else if image_asset {
                        "asset"
                    } else {
                        "cutsheet"
                    },
                );
                if mode == "asset" {
                    if !image_asset {
                        return Err("mode asset requires an image asset source.".into());
                    }
                    if v.get("time").is_some()
                        || v.get("frame").is_some()
                        || v.get("times").is_some()
                    {
                        return Err("Original image inspection does not accept time selectors; use mode frame.".into());
                    }
                    let CaptureSource::Asset { asset_id, version } = &source else {
                        unreachable!()
                    };
                    let path = self.chat_asset_path(*asset_id, version.as_deref())?;
                    return Ok(ToolResult {
                        text:
                            json!({"source":v["source"],"version":version,"kind":"original image"})
                                .to_string(),
                        media: vec![json!({"host_image_path":path})],
                    }
                    .into());
                }
                if !matches!(mode, "frame" | "cutsheet") {
                    return Err("Unknown look mode.".into());
                }
                let selector = time(&v)?;
                if (mode == "frame" && v.get("times").is_some())
                    || (mode == "cutsheet" && selector.is_some())
                {
                    return Err(
                        "Frame mode accepts time/frame; cutsheet mode accepts times.".into(),
                    );
                }
                let request = if mode == "frame" {
                    CaptureRequest::Frame {
                        source,
                        time: selector,
                        mode: CaptureMode::Normal,
                        format: "png".into(),
                        annotate: false,
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
                let capture = &response.data["capture"];
                let frames = capture["frames"].as_array().map(|frames| {
                    frames
                        .iter()
                        .map(|f| json!({"time":f["time"],"local_time":f["local_time"]}))
                        .collect::<Vec<_>>()
                });
                return Ok(ToolResult{text:self.chat.handles.compact(json!({"source":v.get("source").unwrap_or(&json!("timeline")),"kind":mode,"time":capture["time"],"local_time":capture["local_time"],"frames":frames})).to_string(),media:vec![json!({"host_image_path":path})]}.into());
            }
            "watch_video" => return self.prepare_chat_video(&v),
            "asset_edit" if v["action"] == "extract_still" => {
                let source = self.chat.handles.source(&v)?;
                let response = self.extract_agent_still_to_asset(
                    source,
                    time(&v)?,
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
        }
        .into())
    }

    fn chat_asset_path(&self, id: Uuid, version: Option<&str>) -> Result<PathBuf, String> {
        let project = &self.editor.project;
        let asset = project.find_asset(id).ok_or("Asset no longer exists.")?;
        let root = project
            .project_path
            .as_ref()
            .ok_or("Open a project first.")?;
        if let Some(version) = version {
            if !project
                .generative_config(id)
                .is_some_and(|c| c.versions.iter().any(|r| r.version == version))
            {
                return Err("Generation version not found.".into());
            }
            crate::core::generation::generative_asset_source_path(root, asset, Some(version))
        } else {
            crate::core::generation::active_asset_source_path(root, asset)
        }
        .ok_or("Asset output is unavailable.".into())
    }

    fn prepare_chat_video(&self, v: &Value) -> Result<ChatToolResult, String> {
        if v.get("source").is_some() && v.get("asset").is_some() {
            return Err("Choose source or asset, not both.".into());
        }
        let mut args = v.clone();
        if let Some(asset) = v.get("asset") {
            args["source"] = asset.clone();
        }
        let source = self.chat.handles.source(&args)?;
        let (project, asset_path, duration, scope) = match &source {
            CaptureSource::Asset { asset_id, version } => {
                let asset = self
                    .editor
                    .project
                    .find_asset(*asset_id)
                    .ok_or("Asset not found.")?;
                if !matches!(
                    asset.kind,
                    AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. }
                ) {
                    return Err("Choose a video asset.".into());
                }
                (
                    self.editor.project.clone(),
                    Some(self.chat_asset_path(*asset_id, version.as_deref())?),
                    asset.duration_seconds,
                    "asset",
                )
            }
            CaptureSource::Timeline | CaptureSource::Track { .. } => {
                if v.get("version").is_some() {
                    return Err("version applies only to assets.".into());
                }
                let (project, ..) = self.capture_project_and_time(&source, None)?;
                let scope = if matches!(source, CaptureSource::Track { .. }) {
                    "isolated track"
                } else {
                    "visible composite"
                };
                (project, None, Some(self.editor.project.duration()), scope)
            }
            CaptureSource::Clip { .. } => {
                return Err("Video source must be an asset, timeline, or video track.".into())
            }
        };
        let start = v
            .get("start")
            .map(|value| {
                value
                    .as_f64()
                    .filter(|t| t.is_finite() && *t >= 0.0)
                    .ok_or("Invalid start seconds.")
            })
            .transpose()?;
        let end = v
            .get("end")
            .map(|value| {
                value
                    .as_f64()
                    .filter(|t| t.is_finite() && *t > 0.0)
                    .ok_or("Invalid end seconds.")
            })
            .transpose()?;
        let (start, end) = match (start, end) {
            (Some(start), Some(end)) => (start, end),
            (None, None) if asset_path.is_some() => (
                0.0,
                duration.ok_or("Asset duration unknown; specify start and end.")?,
            ),
            _ => return Err("Specify both start and end seconds.".into()),
        };
        if !end.is_finite() || end <= start || end - start > 12.0 {
            return Err("Choose a nonempty video range of at most 12 seconds. Longer assets require start/end.".into());
        }
        if duration.is_some_and(|duration| end > duration + 0.001) {
            return Err("Video range exceeds the source duration.".into());
        }
        let output =
            crate::core::paths::app_tmp_root().join(format!("chat-video-{}.mp4", Uuid::new_v4()));
        let result = ToolResult {
            text: json!({"source":args.get("source").unwrap_or(&json!("timeline")),"version":v.get("version"),"scope":scope,"start_seconds":start,"end_seconds_exclusive":end,"duration_seconds":end-start,"media_zero_seconds":start,"time_domain":if asset_path.is_some() {"asset"} else {"timeline"},"kind":"silent native video proxy","maximum_dimensions":[320,320],"sampling":"Controlled by backend preset; use look for exact frames."}).to_string(),
            media: vec![json!({"host_video_path":output})],
        };
        Ok(ChatToolResult {
            result,
            video: Some(ChatVideo {
                project,
                asset_path,
                start,
                end,
                output,
            }),
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

fn time(v: &Value) -> Result<Option<TimeSelector>, String> {
    if v.get("time").is_some() && v.get("frame").is_some() {
        return Err("Choose time or frame, not both.".into());
    }
    if let Some(value) = v.get("frame") {
        let frame = value
            .as_i64()
            .filter(|f| *f >= 0)
            .ok_or("frame must be a nonnegative integer")?;
        return Ok(Some(TimeSelector {
            frame: Some(frame),
            ..Default::default()
        }));
    }
    v.get("time")
        .map(|value| {
            let seconds = value
                .as_f64()
                .filter(|t| t.is_finite() && *t >= 0.0)
                .ok_or("time must be finite nonnegative seconds")?;
            Ok(TimeSelector {
                seconds: Some(seconds),
                ..Default::default()
            })
        })
        .transpose()
}
pub(super) fn prepare_video(video: &ChatVideo, cancel: Arc<AtomicBool>) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Stopped.".into());
    }
    std::fs::create_dir_all(video.output.parent().ok_or("Missing proxy directory")?)
        .map_err(|e| e.to_string())?;
    if let Some(path) = &video.asset_path {
        let metadata =
            crate::core::media::probe_video_metadata(path).ok_or("Unable to probe video asset.")?;
        if !metadata
            .duration_seconds
            .is_some_and(|duration| video.end <= duration + 0.001)
        {
            return Err(
                "Video range exceeds the actual media duration or duration is unavailable.".into(),
            );
        }
        let mut command = std::process::Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-ss"])
            .arg(video.start.to_string()).arg("-i").arg(path)
            .arg("-t").arg((video.end - video.start).to_string())
            .args(["-map", "0:v:0", "-vf", "scale=320:320:force_original_aspect_ratio=decrease:force_divisible_by=2,setsar=1,setpts=PTS-STARTPTS", "-an", "-c:v", "libx264", "-crf", "18", "-preset", "fast", "-pix_fmt", "yuv420p"])
            .arg(&video.output).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Unable to start video proxy encoder: {e}"))?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Stopped.".into());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err("Video proxy encoding failed.".into())
                    }
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(40)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(e.to_string());
                }
            }
        }
    }
    let width = video.project.settings.width.max(2);
    let height = video.project.settings.height.max(2);
    let scale = 320.0 / width.max(height) as f64;
    let even = |size: u32| ((size as f64 * scale).round() as u32 / 2 * 2).max(2);
    let fps = video.project.settings.fps;
    if !fps.is_finite() || fps <= 0.0 || fps > 60.0 {
        return Err("Timeline video inspection supports project frame rates up to 60 FPS.".into());
    }
    let job = VideoExportJob {
        project: video.project.clone(),
        preserve_project_canvas: true,
        settings: VideoExportSettings {
            output_path: video.output.clone(),
            codec: VideoExportCodec::H264,
            width: even(width),
            height: even(height),
            fps,
            start_seconds: video.start,
            duration_seconds: video.end - video.start,
            include_audio: false,
            quality: VideoExportQuality::High,
            frame_format: VideoExportFrameFormat::Bmp,
            timestamp_overlay: TimestampOverlaySettings {
                enabled: false,
                position: TimestampOverlayPosition::BottomCenter,
            },
        },
    };
    let mut result = Err("Video preparation did not finish.".into());
    crate::core::export::export_video(job, cancel, |event| match event {
        VideoExportEvent::Finished(_) => result = Ok(()),
        VideoExportEvent::Failed(error) => result = Err(error),
        VideoExportEvent::Cancelled => result = Err("Stopped.".into()),
        _ => {}
    });
    result
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
            let mime = if video {
                "video/mp4"
            } else {
                match image::guess_format(&bytes).map_err(|_| "Unrecognized image format")? {
                    image::ImageFormat::Png => "image/png",
                    image::ImageFormat::Jpeg => "image/jpeg",
                    _ => {
                        let image = image::load_from_memory(&bytes)
                            .map_err(|_| "Unable to decode image")?;
                        bytes.clear();
                        image
                            .write_to(
                                &mut std::io::Cursor::new(&mut bytes),
                                image::ImageFormat::Png,
                            )
                            .map_err(|_| "Unable to encode image")?;
                        "image/png"
                    }
                }
            };
            if bytes.len() as u64 > limit {
                return Err("Prepared image exceeds the 32 MiB Chat limit.".into());
            }
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            *media = if video {
                json!({"type":"input_video","input_video":{"data":encoded}})
            } else {
                json!({"type":"image_url","image_url":{"url":format!("data:{mime};base64,{encoded}")}})
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
            .result
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
