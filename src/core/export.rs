use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use image::{imageops::FilterType, RgbaImage};
use uuid::Uuid;

use crate::constants::PREVIEW_CACHE_BUDGET_BYTES;
use crate::core::audio::decode::{decode_audio_to_f32, AudioDecodeConfig};
use crate::core::audio::waveform::resolve_audio_or_video_source;
use crate::core::preview::{PreviewDecodeMode, PreviewRenderer, PreviewRgbaFrame};
use crate::state::{Project, TrackType};

const EXPORT_AUDIO_SAMPLE_RATE: u32 = 48_000;
const EXPORT_AUDIO_CHANNELS: u16 = 2;
const EXPORT_VIDEO_PROGRESS_MAX: f32 = 0.82;
const EXPORT_AUDIO_PROGRESS_START: f32 = 0.82;
const EXPORT_AUDIO_PROGRESS_MAX: f32 = 0.9;
const EXPORT_ENCODE_PROGRESS: f32 = 0.94;
const EXPORT_PREVIEW_MAX_W: u32 = 260;
const EXPORT_PREVIEW_MAX_H: u32 = 150;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoExportQuality {
    Draft,
    Standard,
    High,
}

impl VideoExportQuality {
    pub fn label(self) -> &'static str {
        match self {
            VideoExportQuality::Draft => "Draft",
            VideoExportQuality::Standard => "Standard",
            VideoExportQuality::High => "High",
        }
    }

    fn crf(self) -> &'static str {
        match self {
            VideoExportQuality::Draft => "28",
            VideoExportQuality::Standard => "23",
            VideoExportQuality::High => "18",
        }
    }

    fn preset(self) -> &'static str {
        match self {
            VideoExportQuality::Draft => "veryfast",
            VideoExportQuality::Standard => "medium",
            VideoExportQuality::High => "slow",
        }
    }
}

#[derive(Clone, Debug)]
pub struct VideoExportSettings {
    pub output_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub include_audio: bool,
    pub quality: VideoExportQuality,
}

#[derive(Clone, Debug)]
pub struct VideoExportJob {
    pub project: Project,
    pub settings: VideoExportSettings,
}

#[derive(Clone, Debug)]
pub struct VideoExportPreview {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum VideoExportEvent {
    Progress {
        stage: &'static str,
        message: String,
        progress: f32,
        frame_index: Option<usize>,
        total_frames: Option<usize>,
        preview: Option<VideoExportPreview>,
    },
    Finished(VideoExportSummary),
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct VideoExportSummary {
    pub output_path: PathBuf,
    pub frame_count: usize,
    pub duration_seconds: f64,
    pub audio_included: bool,
    pub warnings: Vec<String>,
}

pub fn export_video(
    mut job: VideoExportJob,
    cancel: Arc<AtomicBool>,
    mut emit: impl FnMut(VideoExportEvent),
) {
    match export_video_inner(&mut job, &cancel, &mut emit) {
        Ok(summary) => emit(VideoExportEvent::Finished(summary)),
        Err(ExportFailure::Cancelled) => emit(VideoExportEvent::Cancelled),
        Err(ExportFailure::Error(err)) => emit(VideoExportEvent::Failed(err)),
    }
}

enum ExportFailure {
    Cancelled,
    Error(String),
}

type ExportResult<T> = Result<T, ExportFailure>;

impl From<String> for ExportFailure {
    fn from(value: String) -> Self {
        ExportFailure::Error(value)
    }
}

fn export_video_inner(
    job: &mut VideoExportJob,
    cancel: &Arc<AtomicBool>,
    emit: &mut impl FnMut(VideoExportEvent),
) -> ExportResult<VideoExportSummary> {
    validate_export_settings(&job.settings)?;
    let project_root =
        job.project.project_path.clone().ok_or_else(|| {
            ExportFailure::Error("Project must be saved before export.".to_string())
        })?;
    if !project_root.exists() {
        return Err(ExportFailure::Error(format!(
            "Project folder does not exist: {}",
            project_root.display()
        )));
    }

    if let Some(parent) = job.settings.output_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ExportFailure::Error(format!(
                "Failed to create export directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let temp_root = project_root
        .join(".cache")
        .join("exports")
        .join(format!("export-{}", Uuid::new_v4()));
    let frame_dir = temp_root.join("frames");
    fs::create_dir_all(&frame_dir).map_err(|err| {
        ExportFailure::Error(format!(
            "Failed to create export frame cache {}: {err}",
            frame_dir.display()
        ))
    })?;

    let result = (|| {
        let frame_count = export_frame_count(job.settings.duration_seconds, job.settings.fps);
        render_video_frames(job, &project_root, &frame_dir, frame_count, cancel, emit)?;

        let mut warnings = Vec::new();
        let audio_path = temp_root.join("mix.wav");
        let audio_included = if job.settings.include_audio {
            render_audio_mix(job, &project_root, &audio_path, cancel, emit, &mut warnings)?
        } else {
            false
        };

        check_cancel(cancel)?;
        emit(VideoExportEvent::Progress {
            stage: "encode",
            message: "Encoding MP4".to_string(),
            progress: EXPORT_ENCODE_PROGRESS,
            frame_index: None,
            total_frames: None,
            preview: None,
        });
        encode_mp4(
            job,
            &frame_dir,
            audio_included.then_some(audio_path.as_path()),
            cancel,
        )?;

        Ok(VideoExportSummary {
            output_path: job.settings.output_path.clone(),
            frame_count,
            duration_seconds: job.settings.duration_seconds,
            audio_included,
            warnings,
        })
    })();

    match &result {
        Ok(_) | Err(ExportFailure::Cancelled) => {
            let _ = fs::remove_dir_all(&temp_root);
        }
        Err(ExportFailure::Error(_)) => {}
    }

    result
}

fn validate_export_settings(settings: &VideoExportSettings) -> ExportResult<()> {
    if settings.width < 2 || settings.height < 2 {
        return Err(ExportFailure::Error(
            "Export width and height must be at least 2 pixels.".to_string(),
        ));
    }
    if settings.width % 2 != 0 || settings.height % 2 != 0 {
        return Err(ExportFailure::Error(
            "Export width and height must be even for MP4/H.264.".to_string(),
        ));
    }
    if !(settings.fps.is_finite() && settings.fps > 0.0 && settings.fps <= 240.0) {
        return Err(ExportFailure::Error(
            "Export FPS must be between 1 and 240.".to_string(),
        ));
    }
    if !(settings.start_seconds.is_finite() && settings.start_seconds >= 0.0) {
        return Err(ExportFailure::Error(
            "Export start time must be zero or greater.".to_string(),
        ));
    }
    if !(settings.duration_seconds.is_finite() && settings.duration_seconds > 0.0) {
        return Err(ExportFailure::Error(
            "Export duration must be greater than zero.".to_string(),
        ));
    }
    if settings.output_path.as_os_str().is_empty() {
        return Err(ExportFailure::Error(
            "Choose an output file before exporting.".to_string(),
        ));
    }
    Ok(())
}

fn render_video_frames(
    job: &mut VideoExportJob,
    project_root: &Path,
    frame_dir: &Path,
    frame_count: usize,
    cancel: &Arc<AtomicBool>,
    emit: &mut impl FnMut(VideoExportEvent),
) -> ExportResult<()> {
    let mut export_project = job.project.clone();
    export_project.settings.width = job.settings.width;
    export_project.settings.height = job.settings.height;
    export_project.settings.preview_max_width = job.settings.width;
    export_project.settings.preview_max_height = job.settings.height;
    export_project.project_path = Some(project_root.to_path_buf());

    let renderer = PreviewRenderer::new_with_limits(
        project_root.to_path_buf(),
        PREVIEW_CACHE_BUDGET_BYTES,
        job.settings.width,
        job.settings.height,
    );
    let preview_every = (job.settings.fps.round() as usize / 2).max(1);

    for frame_index in 0..frame_count {
        check_cancel(cancel)?;
        let time = job.settings.start_seconds + frame_index as f64 / job.settings.fps;
        let output =
            renderer.render_frame_rgba(&export_project, time, PreviewDecodeMode::Seek, false);
        let frame = output
            .frame
            .unwrap_or_else(|| black_frame(job.settings.width, job.settings.height));
        let frame_path = frame_dir.join(format!("frame_{frame_index:06}.png"));
        image::save_buffer_with_format(
            &frame_path,
            &frame.bytes,
            frame.width,
            frame.height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(|err| {
            ExportFailure::Error(format!(
                "Failed to write frame {}: {err}",
                frame_path.display()
            ))
        })?;

        let frame_number = frame_index + 1;
        let should_preview =
            frame_index == 0 || frame_number == frame_count || frame_index % preview_every == 0;
        emit(VideoExportEvent::Progress {
            stage: "frames",
            message: format!("Rendering frame {frame_number} of {frame_count}"),
            progress: (frame_number as f32 / frame_count.max(1) as f32) * EXPORT_VIDEO_PROGRESS_MAX,
            frame_index: Some(frame_number),
            total_frames: Some(frame_count),
            preview: should_preview.then(|| preview_from_frame(&frame)),
        });
    }

    Ok(())
}

fn render_audio_mix(
    job: &VideoExportJob,
    project_root: &Path,
    audio_path: &Path,
    cancel: &Arc<AtomicBool>,
    emit: &mut impl FnMut(VideoExportEvent),
    warnings: &mut Vec<String>,
) -> ExportResult<bool> {
    check_cancel(cancel)?;
    emit(VideoExportEvent::Progress {
        stage: "audio",
        message: "Preparing audio mix".to_string(),
        progress: EXPORT_AUDIO_PROGRESS_START,
        frame_index: None,
        total_frames: None,
        preview: None,
    });

    let sample_rate = EXPORT_AUDIO_SAMPLE_RATE as f64;
    let channels = EXPORT_AUDIO_CHANNELS as usize;
    let total_frames = (job.settings.duration_seconds * sample_rate)
        .ceil()
        .max(1.0) as usize;
    let mut mix = vec![0.0_f32; total_frames * channels];

    let track_types: HashMap<_, _> = job
        .project
        .tracks
        .iter()
        .map(|track| (track.id, track.track_type))
        .collect();
    let track_volumes: HashMap<_, _> = job
        .project
        .tracks
        .iter()
        .map(|track| (track.id, track.volume))
        .collect();
    let mut decoded_cache: HashMap<Uuid, Option<Arc<Vec<f32>>>> = HashMap::new();
    let mut mixed_any = false;

    let export_start = job.settings.start_seconds;
    let export_end = job.settings.start_seconds + job.settings.duration_seconds;
    let clips_len = job.project.clips.len().max(1);

    for (clip_index, clip) in job.project.clips.iter().enumerate() {
        check_cancel(cancel)?;
        let Some(track_type) = track_types.get(&clip.track_id).copied() else {
            continue;
        };
        if !matches!(track_type, TrackType::Audio | TrackType::Video) {
            continue;
        }
        let Some(asset) = job.project.find_asset(clip.asset_id) else {
            continue;
        };
        if !asset.is_audio() && !asset.is_video() {
            continue;
        }

        let overlap_start = clip.start_time.max(export_start);
        let overlap_end = clip.end_time().min(export_end);
        if overlap_end <= overlap_start {
            continue;
        }
        let gain = track_volumes.get(&clip.track_id).copied().unwrap_or(1.0) * clip.volume;
        if gain <= 0.0 {
            continue;
        }

        let samples = match decoded_cache.get(&asset.id).cloned() {
            Some(samples) => samples,
            None => {
                let source = resolve_audio_or_video_source(project_root, asset);
                let decoded = match source {
                    Some(source) => decode_audio_to_f32(
                        &source,
                        AudioDecodeConfig {
                            target_rate: EXPORT_AUDIO_SAMPLE_RATE,
                            target_channels: EXPORT_AUDIO_CHANNELS,
                        },
                    )
                    .map(|decoded| Arc::new(decoded.samples)),
                    None => Err("No audio source found".to_string()),
                };
                let entry = match decoded {
                    Ok(samples) => Some(samples),
                    Err(err) => {
                        warnings.push(format!("Skipped audio for {}: {err}", asset.name));
                        None
                    }
                };
                decoded_cache.insert(asset.id, entry.clone());
                entry
            }
        };
        let Some(samples) = samples else {
            continue;
        };

        let source_frame = ((clip.trim_in_seconds + overlap_start - clip.start_time) * sample_rate)
            .round()
            .max(0.0) as usize;
        let dest_frame = ((overlap_start - export_start) * sample_rate)
            .round()
            .max(0.0) as usize;
        let requested_frames = ((overlap_end - overlap_start) * sample_rate)
            .round()
            .max(0.0) as usize;
        let available_source_frames = samples.len() / channels;
        if source_frame >= available_source_frames || dest_frame >= total_frames {
            continue;
        }
        let copy_frames = requested_frames
            .min(available_source_frames.saturating_sub(source_frame))
            .min(total_frames.saturating_sub(dest_frame));
        if copy_frames == 0 {
            continue;
        }

        for frame in 0..copy_frames {
            let src = (source_frame + frame) * channels;
            let dst = (dest_frame + frame) * channels;
            for channel in 0..channels {
                mix[dst + channel] += samples[src + channel] * gain;
            }
        }
        mixed_any = true;
        let clip_progress = (clip_index + 1) as f32 / clips_len as f32;
        emit(VideoExportEvent::Progress {
            stage: "audio",
            message: format!("Mixing audio {}", asset.name),
            progress: EXPORT_AUDIO_PROGRESS_START
                + clip_progress * (EXPORT_AUDIO_PROGRESS_MAX - EXPORT_AUDIO_PROGRESS_START),
            frame_index: None,
            total_frames: None,
            preview: None,
        });
    }

    if !mixed_any {
        return Ok(false);
    }

    write_pcm16_wav(
        audio_path,
        &mix,
        EXPORT_AUDIO_SAMPLE_RATE,
        EXPORT_AUDIO_CHANNELS,
    )?;
    Ok(true)
}

fn encode_mp4(
    job: &VideoExportJob,
    frame_dir: &Path,
    audio_path: Option<&Path>,
    cancel: &Arc<AtomicBool>,
) -> ExportResult<()> {
    let frame_pattern = frame_dir.join("frame_%06d.png");
    let mut command = Command::new("ffmpeg");
    command
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-framerate")
        .arg(format_fps(job.settings.fps))
        .arg("-i")
        .arg(&frame_pattern);
    if let Some(audio_path) = audio_path {
        command.arg("-i").arg(audio_path);
    }
    command
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg(job.settings.quality.preset())
        .arg("-crf")
        .arg(job.settings.quality.crf())
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-movflags")
        .arg("+faststart");
    if audio_path.is_some() {
        command
            .arg("-c:a")
            .arg("aac")
            .arg("-b:a")
            .arg("192k")
            .arg("-shortest");
    }
    command.arg(&job.settings.output_path);
    command.stdout(Stdio::null()).stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|err| {
        ExportFailure::Error(format!(
            "Failed to start ffmpeg. Ensure ffmpeg.exe is on PATH or installed: {err}"
        ))
    })?;
    let stderr_text = Arc::new(Mutex::new(String::new()));
    if let Some(mut stderr) = child.stderr.take() {
        let stderr_text = Arc::clone(&stderr_text);
        thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            if let Ok(mut slot) = stderr_text.lock() {
                *slot = text;
            }
        });
    }

    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ExportFailure::Cancelled);
        }
        if let Some(status) = child.try_wait().map_err(|err| {
            ExportFailure::Error(format!("Failed while waiting for ffmpeg: {err}"))
        })? {
            if status.success() {
                return Ok(());
            }
            let stderr = stderr_text
                .lock()
                .ok()
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| format!("ffmpeg exited with status {status}"));
            return Err(ExportFailure::Error(format!(
                "FFmpeg export failed: {stderr}"
            )));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn write_pcm16_wav(
    path: &Path,
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
) -> ExportResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ExportFailure::Error(format!("Failed to create audio mix directory: {err}"))
        })?;
    }
    let mut file = fs::File::create(path).map_err(|err| {
        ExportFailure::Error(format!(
            "Failed to create audio mix {}: {err}",
            path.display()
        ))
    })?;
    let bytes_per_sample = 2_u16;
    let block_align = channels * bytes_per_sample;
    let byte_rate = sample_rate * block_align as u32;
    let data_len = samples
        .len()
        .checked_mul(bytes_per_sample as usize)
        .and_then(|len| u32::try_from(len).ok())
        .ok_or_else(|| {
            ExportFailure::Error("Audio mix is too large for WAV output.".to_string())
        })?;
    let riff_len = 36_u32.checked_add(data_len).ok_or_else(|| {
        ExportFailure::Error("Audio mix is too large for WAV output.".to_string())
    })?;

    file.write_all(b"RIFF").map_err(io_error)?;
    file.write_all(&riff_len.to_le_bytes()).map_err(io_error)?;
    file.write_all(b"WAVE").map_err(io_error)?;
    file.write_all(b"fmt ").map_err(io_error)?;
    file.write_all(&16_u32.to_le_bytes()).map_err(io_error)?;
    file.write_all(&1_u16.to_le_bytes()).map_err(io_error)?;
    file.write_all(&channels.to_le_bytes()).map_err(io_error)?;
    file.write_all(&sample_rate.to_le_bytes())
        .map_err(io_error)?;
    file.write_all(&byte_rate.to_le_bytes()).map_err(io_error)?;
    file.write_all(&block_align.to_le_bytes())
        .map_err(io_error)?;
    file.write_all(&16_u16.to_le_bytes()).map_err(io_error)?;
    file.write_all(b"data").map_err(io_error)?;
    file.write_all(&data_len.to_le_bytes()).map_err(io_error)?;
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        file.write_all(&value.to_le_bytes()).map_err(io_error)?;
    }
    Ok(())
}

fn io_error(err: std::io::Error) -> ExportFailure {
    ExportFailure::Error(err.to_string())
}

fn export_frame_count(duration_seconds: f64, fps: f64) -> usize {
    (duration_seconds * fps).ceil().max(1.0) as usize
}

fn check_cancel(cancel: &Arc<AtomicBool>) -> ExportResult<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(ExportFailure::Cancelled)
    } else {
        Ok(())
    }
}

fn black_frame(width: u32, height: u32) -> PreviewRgbaFrame {
    let mut bytes = vec![0_u8; width as usize * height as usize * 4];
    for alpha in bytes.iter_mut().skip(3).step_by(4) {
        *alpha = 255;
    }
    PreviewRgbaFrame {
        width,
        height,
        bytes,
    }
}

fn preview_from_frame(frame: &PreviewRgbaFrame) -> VideoExportPreview {
    let Some(image) = RgbaImage::from_raw(frame.width, frame.height, frame.bytes.clone()) else {
        return VideoExportPreview {
            width: frame.width,
            height: frame.height,
            rgba: frame.bytes.clone(),
        };
    };
    let scale = (EXPORT_PREVIEW_MAX_W as f32 / frame.width.max(1) as f32)
        .min(EXPORT_PREVIEW_MAX_H as f32 / frame.height.max(1) as f32)
        .min(1.0);
    let width = (frame.width as f32 * scale).round().max(1.0) as u32;
    let height = (frame.height as f32 * scale).round().max(1.0) as u32;
    let image = image::imageops::resize(&image, width, height, FilterType::Triangle);
    VideoExportPreview {
        width,
        height,
        rgba: image.into_raw(),
    }
}

fn format_fps(fps: f64) -> String {
    if (fps - fps.round()).abs() < 0.0001 {
        format!("{:.0}", fps)
    } else {
        format!("{fps:.3}")
    }
}
