//! Shared timing and validation helpers for purpose-built timeline bridge clips.

use uuid::Uuid;

use crate::state::{
    input_value_as_f64, Clip, ClipBridgeLink, GenerativeConfig, InputRole, InputValue, Project,
    ProviderEntry, ProviderInputType, ProviderWorkflowKind,
    DEFAULT_TIMELINE_BRIDGE_MAX_VISIBLE_FRAMES,
};

#[derive(Debug, Clone)]
pub struct TimelineBridgeFields {
    pub left_video: String,
    pub right_video: String,
    pub fps: String,
    pub left_replace_frames: String,
    pub right_replace_frames: String,
    pub edge_blend_frames: String,
}

#[derive(Debug, Clone)]
pub struct TimelineBridgeParameters {
    pub processing_fps: f64,
    pub left_replace_frames: u32,
    pub right_replace_frames: u32,
    pub edge_blend_frames: u32,
    pub max_visible_frames: Option<u32>,
}

impl TimelineBridgeParameters {
    pub fn visible_frames(&self) -> u32 {
        self.left_replace_frames + self.right_replace_frames
    }

    pub fn left_seconds(&self) -> f64 {
        self.left_replace_frames as f64 / self.processing_fps.max(1.0)
    }

    pub fn right_seconds(&self) -> f64 {
        self.right_replace_frames as f64 / self.processing_fps.max(1.0)
    }
}

#[derive(Debug, Clone)]
pub struct TimelineBridgeResolution {
    pub left_clip_id: Option<Uuid>,
    pub right_clip_id: Option<Uuid>,
    pub start_time: f64,
    pub duration: f64,
    pub parameters: Option<TimelineBridgeParameters>,
    pub errors: Vec<String>,
}

impl TimelineBridgeResolution {
    pub fn valid(&self) -> bool {
        self.errors.is_empty()
            && self.parameters.is_some()
            && self.left_clip_id.is_some()
            && self.right_clip_id.is_some()
    }

    pub fn tooltip(&self) -> Option<String> {
        (!self.errors.is_empty()).then(|| self.errors.join("\n"))
    }
}

pub fn provider_is_timeline_bridge(provider: &ProviderEntry) -> bool {
    provider.resolved_workflow_kind() == ProviderWorkflowKind::VideoToBridge
}

pub fn timeline_bridge_fields(
    provider: &ProviderEntry,
) -> Result<TimelineBridgeFields, Vec<String>> {
    let mut errors = Vec::new();
    let left_video = role_input_name(provider, InputRole::LeftVideo, ProviderInputType::Video);
    let right_video = role_input_name(provider, InputRole::RightVideo, ProviderInputType::Video);
    let fps = numeric_role_input_name(provider, InputRole::Fps);
    let left_replace_frames = numeric_role_input_name(provider, InputRole::LeftReplaceFrames);
    let right_replace_frames = numeric_role_input_name(provider, InputRole::RightReplaceFrames);
    let edge_blend_frames = numeric_role_input_name(provider, InputRole::EdgeBlendFrames);

    if left_video.is_none() {
        errors.push("Provider needs a video input with Role: Left Video.".to_string());
    }
    if right_video.is_none() {
        errors.push("Provider needs a video input with Role: Right Video.".to_string());
    }
    if fps.is_none() {
        errors.push("Provider needs a numeric input with Role: FPS.".to_string());
    }
    if left_replace_frames.is_none() {
        errors.push("Provider needs a numeric input with Role: Left Frames.".to_string());
    }
    if right_replace_frames.is_none() {
        errors.push("Provider needs a numeric input with Role: Right Frames.".to_string());
    }
    if edge_blend_frames.is_none() {
        errors.push("Provider needs a numeric input with Role: Edge Blend.".to_string());
    }

    if errors.is_empty() {
        Ok(TimelineBridgeFields {
            left_video: left_video.unwrap(),
            right_video: right_video.unwrap(),
            fps: fps.unwrap(),
            left_replace_frames: left_replace_frames.unwrap(),
            right_replace_frames: right_replace_frames.unwrap(),
            edge_blend_frames: edge_blend_frames.unwrap(),
        })
    } else {
        Err(errors)
    }
}

pub fn timeline_bridge_parameters(
    provider: &ProviderEntry,
    config: &GenerativeConfig,
) -> Result<TimelineBridgeParameters, Vec<String>> {
    let mut errors = Vec::new();
    let fields = match timeline_bridge_fields(provider) {
        Ok(fields) => fields,
        Err(field_errors) => {
            errors.extend(field_errors);
            return Err(errors);
        }
    };

    let fps = input_number(provider, config, &fields.fps).unwrap_or(0.0);
    if !fps.is_finite() || fps <= 0.0 {
        errors.push("Bridge FPS must be greater than 0.".to_string());
    }
    let left = input_number(provider, config, &fields.left_replace_frames)
        .unwrap_or(0.0)
        .round()
        .max(0.0) as u32;
    let right = input_number(provider, config, &fields.right_replace_frames)
        .unwrap_or(0.0)
        .round()
        .max(0.0) as u32;
    let edge = input_number(provider, config, &fields.edge_blend_frames)
        .unwrap_or(0.0)
        .round()
        .max(0.0) as u32;
    if left == 0 && right == 0 {
        errors
            .push("Bridge must replace at least one frame on the left or right side.".to_string());
    }

    let max_visible_frames = provider
        .timeline_bridge
        .as_ref()
        .and_then(|settings| settings.max_visible_frames)
        .or(Some(DEFAULT_TIMELINE_BRIDGE_MAX_VISIBLE_FRAMES));

    if errors.is_empty() {
        Ok(TimelineBridgeParameters {
            processing_fps: fps,
            left_replace_frames: left,
            right_replace_frames: right,
            edge_blend_frames: edge,
            max_visible_frames,
        })
    } else {
        Err(errors)
    }
}

pub fn resolve_timeline_bridge_clip(
    project: &Project,
    provider: Option<&ProviderEntry>,
    config: Option<&GenerativeConfig>,
    bridge_clip: &Clip,
) -> TimelineBridgeResolution {
    let fallback = TimelineBridgeResolution {
        left_clip_id: bridge_clip
            .bridge
            .as_ref()
            .and_then(|link| link.left_clip_id),
        right_clip_id: bridge_clip
            .bridge
            .as_ref()
            .and_then(|link| link.right_clip_id),
        start_time: bridge_clip.start_time,
        duration: bridge_clip.duration,
        parameters: None,
        errors: Vec::new(),
    };

    let Some(provider) = provider.filter(|provider| provider_is_timeline_bridge(provider)) else {
        return fallback;
    };
    let Some(config) = config else {
        return TimelineBridgeResolution {
            errors: vec!["Bridge generative config is unavailable.".to_string()],
            ..fallback
        };
    };

    let mut errors = Vec::new();
    let params = match timeline_bridge_parameters(provider, config) {
        Ok(params) => Some(params),
        Err(parameter_errors) => {
            errors.extend(parameter_errors);
            None
        }
    };

    let link = bridge_clip
        .bridge
        .clone()
        .unwrap_or_else(|| ClipBridgeLink::new(None, None));
    let left_clip = link
        .left_clip_id
        .and_then(|id| project.clips.iter().find(|clip| clip.id == id));
    let right_clip = link
        .right_clip_id
        .and_then(|id| project.clips.iter().find(|clip| clip.id == id));

    let Some(left_clip) = left_clip else {
        errors.push("Bridge needs a left source clip.".to_string());
        return TimelineBridgeResolution { errors, ..fallback };
    };
    let Some(right_clip) = right_clip else {
        errors.push("Bridge needs a right source clip.".to_string());
        return TimelineBridgeResolution { errors, ..fallback };
    };

    let mut start_time = bridge_clip.start_time;
    let mut duration = bridge_clip.duration;
    if let Some(params) = params.as_ref() {
        start_time = (left_clip.end_time() - params.left_seconds()).max(0.0);
        let end_time = right_clip.start_time + params.right_seconds();
        duration = (end_time - start_time).max(0.1);

        let left_asset = project.find_asset(left_clip.asset_id);
        let right_asset = project.find_asset(right_clip.asset_id);
        let left_asset_valid = left_asset.is_some_and(|asset| asset.is_video());
        let right_asset_valid = right_asset.is_some_and(|asset| asset.is_video());
        if !left_asset_valid {
            errors.push("Left source must be a video clip.".to_string());
        }
        if !right_asset_valid {
            errors.push("Right source must be a video clip.".to_string());
        }

        if left_clip.id == right_clip.id
            || left_clip.id == bridge_clip.id
            || right_clip.id == bridge_clip.id
        {
            errors.push("Bridge sources must be two different source clips.".to_string());
        }
        if right_clip.start_time < left_clip.start_time {
            errors.push("Right source must start after the left source.".to_string());
        }

        let seam_delta = right_clip.start_time - left_clip.end_time();
        let frame_tolerance = 0.5 / params.processing_fps.max(1.0);
        if seam_delta.abs() > frame_tolerance {
            if seam_delta > 0.0 {
                errors.push(format!(
                    "Source clips have a {} gap; bridge providers require adjacent sources.",
                    format_seconds(seam_delta)
                ));
            } else {
                errors.push(format!(
                    "Source clips overlap by {}; bridge providers require adjacent sources.",
                    format_seconds(seam_delta.abs())
                ));
            }
        }

        let left_available_frames = available_bridge_side_frames(
            left_clip,
            left_asset.and_then(|asset| asset.duration_seconds),
            true,
            &params,
        );
        let right_available_frames = available_bridge_side_frames(
            right_clip,
            right_asset.and_then(|asset| asset.duration_seconds),
            false,
            &params,
        );
        if params.left_replace_frames > left_available_frames {
            errors.push(format!(
                "Bridge needs {} left frames, but the left source has {} available.",
                params.left_replace_frames, left_available_frames
            ));
        }
        if params.right_replace_frames > right_available_frames {
            errors.push(format!(
                "Bridge needs {} right frames, but the right source has {} available.",
                params.right_replace_frames, right_available_frames
            ));
        }
        if let Some(max_visible) = params.max_visible_frames {
            if params.visible_frames() > max_visible {
                errors.push(format!(
                    "Bridge span is {} frames; this provider supports up to {}.",
                    params.visible_frames(),
                    max_visible
                ));
            }
        }
        let min_side = params.left_replace_frames.min(params.right_replace_frames);
        if params.edge_blend_frames > min_side {
            errors.push(format!(
                "Edge blend is {} frames; it must be no more than the smaller side span ({}).",
                params.edge_blend_frames, min_side
            ));
        }
    }

    TimelineBridgeResolution {
        left_clip_id: Some(left_clip.id),
        right_clip_id: Some(right_clip.id),
        start_time,
        duration,
        parameters: params,
        errors,
    }
}

pub fn infer_timeline_bridge_link(project: &Project, bridge_clip: &Clip) -> ClipBridgeLink {
    let center = bridge_clip.start_time + bridge_clip.duration * 0.5;
    let mut left_candidates: Vec<(f64, Uuid)> = Vec::new();
    let mut right_candidates: Vec<(f64, Uuid)> = Vec::new();

    for clip in project
        .clips
        .iter()
        .filter(|clip| clip.id != bridge_clip.id)
    {
        let Some(asset) = project.find_asset(clip.asset_id) else {
            continue;
        };
        if !asset.is_video() {
            continue;
        }
        if clip.end_time() <= center {
            left_candidates.push(((clip.end_time() - center).abs(), clip.id));
        }
        if clip.start_time >= center {
            right_candidates.push(((clip.start_time - center).abs(), clip.id));
        }
    }

    left_candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    right_candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    ClipBridgeLink::new(
        left_candidates.first().map(|(_, id)| *id),
        right_candidates.first().map(|(_, id)| *id),
    )
}

fn role_input_name(
    provider: &ProviderEntry,
    role: InputRole,
    expected_type: ProviderInputType,
) -> Option<String> {
    provider
        .inputs
        .iter()
        .find(|input| input.role == Some(role) && input.input_type == expected_type)
        .map(|input| input.name.clone())
}

fn numeric_role_input_name(provider: &ProviderEntry, role: InputRole) -> Option<String> {
    provider
        .inputs
        .iter()
        .find(|input| {
            input.role == Some(role)
                && matches!(
                    input.input_type,
                    ProviderInputType::Integer | ProviderInputType::Number
                )
        })
        .map(|input| input.name.clone())
}

fn available_bridge_side_frames(
    clip: &Clip,
    source_duration: Option<f64>,
    left_side: bool,
    params: &TimelineBridgeParameters,
) -> u32 {
    let target_duration = if left_side {
        params.left_seconds()
    } else {
        params.right_seconds()
    };
    if target_duration <= 0.0 {
        return 0;
    }
    let local_start = if left_side {
        (clip.duration - target_duration).max(0.0)
    } else {
        0.0
    };
    let mut source_start = clip.source_time_for_local(local_start, source_duration);
    let mut source_end = clip.source_time_for_local(local_start + target_duration, source_duration);
    if let Some(source_duration) = source_duration.filter(|duration| *duration > 0.0) {
        source_start = source_start.clamp(0.0, source_duration);
        source_end = source_end.clamp(0.0, source_duration);
    }
    ((source_end - source_start).abs() * params.processing_fps).floor() as u32
}

fn input_number(
    provider: &ProviderEntry,
    config: &GenerativeConfig,
    input_name: &str,
) -> Option<f64> {
    config
        .inputs
        .get(input_name)
        .and_then(|value| match value {
            InputValue::Literal { value } => input_value_as_f64(value),
            _ => None,
        })
        .or_else(|| {
            provider
                .inputs
                .iter()
                .find(|input| input.name == input_name)
                .and_then(|input| input.default.as_ref())
                .and_then(input_value_as_f64)
        })
}

fn format_seconds(value: f64) -> String {
    if value >= 1.0 {
        format!("{value:.2}s")
    } else {
        format!("{:.0}ms", value * 1000.0)
    }
}
