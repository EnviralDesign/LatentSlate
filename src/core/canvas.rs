//! Exact width/height canvas contracts for generation providers.
//!
//! LatentSlate Engine publishes the legal pixel grid. The editor snaps fallbacks
//! and picker values onto that grid, but never rewrites an explicit off-grid
//! literal: generate fails early instead.

use crate::state::{
    CanvasContract, InputRole, InputUi, ProviderEntry, ProviderInputField, ProviderInputType,
};

pub const ASPECT_PRESETS: [AspectPreset; 6] = [
    AspectPreset {
        label: "1:1",
        width: 1,
        height: 1,
    },
    AspectPreset {
        label: "16:9",
        width: 16,
        height: 9,
    },
    AspectPreset {
        label: "9:16",
        width: 9,
        height: 16,
    },
    AspectPreset {
        label: "4:3",
        width: 4,
        height: 3,
    },
    AspectPreset {
        label: "3:4",
        width: 3,
        height: 4,
    },
    AspectPreset {
        label: "21:9",
        width: 21,
        height: 9,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AspectPreset {
    pub label: &'static str,
    pub width: u32,
    pub height: u32,
}

impl AspectPreset {
    pub fn ratio(self) -> f64 {
        self.width as f64 / self.height.max(1) as f64
    }
}

pub fn dimension_pair<'a>(
    provider: &'a ProviderEntry,
) -> Option<(&'a ProviderInputField, &'a ProviderInputField)> {
    let width = provider
        .inputs
        .iter()
        .find(|input| input.role == Some(InputRole::Width))?;
    let height = provider
        .inputs
        .iter()
        .find(|input| input.role == Some(InputRole::Height))?;
    if !matches!(
        width.input_type,
        ProviderInputType::Integer | ProviderInputType::Number
    ) || !matches!(
        height.input_type,
        ProviderInputType::Integer | ProviderInputType::Number
    ) {
        return None;
    }
    Some((width, height))
}

pub fn canvas_from_provider(provider: &ProviderEntry) -> Option<CanvasContract> {
    if let Some(canvas) = provider.canvas.clone() {
        return Some(canvas);
    }
    let (width, height) = dimension_pair(provider)?;
    canvas_from_dimension_ui(width.ui.as_ref(), height.ui.as_ref())
}

pub fn canvas_from_dimension_ui(width: Option<&InputUi>, height: Option<&InputUi>) -> Option<CanvasContract> {
    let width = width?;
    let height = height?;
    let alignment = width
        .step
        .or(height.step)
        .filter(|step| *step >= 1.0)
        .map(|step| step.round() as u32)?;
    let min_side = width
        .min
        .or(height.min)
        .filter(|min| *min >= 1.0)
        .map(|min| min.round() as u32)
        .unwrap_or(alignment)
        .max(alignment);
    let max_side = width
        .max
        .or(height.max)
        .filter(|max| *max >= 1.0)
        .map(|max| max.round() as u32);
    Some(CanvasContract {
        alignment,
        min_side,
        max_side,
        max_pixels: None,
        max_aspect: None,
    })
}

pub fn snap_to_step(value: i64, origin: i64, step: i64) -> i64 {
    let step = step.max(1);
    let offset = value.saturating_sub(origin);
    let n = ((offset as f64) / (step as f64)).round() as i64;
    origin.saturating_add(n.saturating_mul(step))
}

pub fn snap_integer_to_input(value: i64, ui: Option<&InputUi>) -> i64 {
    let Some(ui) = ui else {
        return value;
    };
    let step = ui
        .step
        .filter(|step| *step >= 1.0)
        .map(|step| step.round() as i64)
        .unwrap_or(1)
        .max(1);
    let origin = ui
        .min
        .map(|min| min.round() as i64)
        .unwrap_or(0);
    let snapped = snap_to_step(value, origin, step);
    let min = ui.min.map(|min| min.round() as i64).unwrap_or(snapped);
    let max = ui.max.map(|max| max.round() as i64).unwrap_or(snapped);
    snapped.clamp(min.min(max), max.max(min))
}

pub fn validate_canvas(canvas: &CanvasContract, width: i64, height: i64) -> Result<(), String> {
    let alignment = canvas.alignment.max(1);
    let min_side = canvas.min_side.max(1) as i64;
    for (name, value) in [("width", width), ("height", height)] {
        if value < min_side {
            return Err(format!("{name} must be at least {min_side}"));
        }
        if let Some(max_side) = canvas.max_side {
            if value > max_side as i64 {
                return Err(format!("{name} must be at most {max_side}"));
            }
        }
        if value % alignment as i64 != 0 {
            return Err(format!(
                "width and height must be divisible by {alignment} pixels; received {width}x{height}"
            ));
        }
    }
    let pixels = (width as u128).saturating_mul(height as u128);
    if let Some(max_pixels) = canvas.max_pixels {
        if pixels > max_pixels as u128 {
            return Err(format!(
                "dimensions exceed this model family's pixel budget ({width}x{height} > {max_pixels} pixels)"
            ));
        }
    }
    if let Some(max_aspect) = canvas.max_aspect.filter(|value| *value > 1.0) {
        let wide = width as f64 > height as f64 * max_aspect;
        let tall = height as f64 > width as f64 * max_aspect;
        if wide || tall {
            return Err(format!(
                "dimensions must stay within a 1:{max_aspect} to {max_aspect}:1 aspect ratio"
            ));
        }
    }
    Ok(())
}

pub fn canvas_is_valid(canvas: &CanvasContract, width: i64, height: i64) -> bool {
    validate_canvas(canvas, width, height).is_ok()
}

pub fn matching_aspect_preset(width: u32, height: u32) -> Option<AspectPreset> {
    let height = height.max(1);
    ASPECT_PRESETS.iter().copied().find(|preset| {
        let left = width as i64 * preset.height as i64;
        let right = height as i64 * preset.width as i64;
        left == right
    })
}

pub fn fit_preset(canvas: &CanvasContract, preset: AspectPreset, target_pixels: u64) -> (u32, u32) {
    fit_exact_ratio(canvas, preset.width, preset.height, target_pixels)
        .unwrap_or_else(|| fit_canvas(canvas, preset.ratio(), target_pixels))
}

pub fn fit_canvas(canvas: &CanvasContract, aspect: f64, target_pixels: u64) -> (u32, u32) {
    let alignment = canvas.alignment.max(1);
    let min_side = round_up(canvas.min_side.max(1), alignment);
    let mut aspect = aspect.max(1.0 / 64.0);
    if let Some(max_aspect) = canvas.max_aspect.filter(|value| *value > 1.0) {
        aspect = aspect.clamp(1.0 / max_aspect, max_aspect);
    }
    let max_side = canvas
        .max_side
        .unwrap_or(4096)
        .min(
            canvas
                .max_pixels
                .map(|pixels| (pixels / min_side.max(1) as u64).max(min_side as u64) as u32)
                .unwrap_or(4096),
        )
        .max(min_side);
    let max_side = (max_side / alignment) * alignment;
    let target_pixels = target_pixels
        .max(min_side as u64 * min_side as u64)
        .min(canvas.max_pixels.unwrap_or(u64::MAX));

    let mut best = (min_side, min_side);
    let mut best_score = f64::INFINITY;
    let mut found = false;
    let mut width = min_side;
    while width <= max_side {
        let ideal_height = (width as f64 / aspect).round();
        for delta in [0_i64, -(alignment as i64), alignment as i64] {
            let height = snap_positive(ideal_height as i64 + delta, alignment);
            if height < min_side as i64 || height > max_side as i64 {
                continue;
            }
            let height = height as u32;
            if !canvas_is_valid(canvas, width as i64, height as i64) {
                continue;
            }
            found = true;
            let pixels = width as u64 * height as u64;
            let current_aspect = width as f64 / height.max(1) as f64;
            let aspect_err = ((current_aspect - aspect) / aspect).abs();
            let pixel_err = ((pixels as f64 - target_pixels as f64).abs())
                / target_pixels.max(1) as f64;
            let score = aspect_err * 4.0 + pixel_err;
            if score < best_score {
                best_score = score;
                best = (width, height);
            }
        }
        width = width.saturating_add(alignment);
        if width < alignment {
            break;
        }
    }
    if found {
        best
    } else {
        (min_side, min_side)
    }
}

pub fn nearest_legal_canvas(canvas: &CanvasContract, width: u32, height: u32) -> (u32, u32) {
    let width = width.max(1);
    let height = height.max(1);
    if canvas_is_valid(canvas, width as i64, height as i64) {
        return (width, height);
    }
    let pixels = width as u64 * height as u64;
    if let Some(preset) = matching_aspect_preset(width, height) {
        return fit_preset(canvas, preset, pixels);
    }
    fit_canvas(canvas, width as f64 / height as f64, pixels)
}

pub fn snap_side(canvas: &CanvasContract, value: i64) -> u32 {
    let alignment = canvas.alignment.max(1) as i64;
    let min_side = canvas.min_side.max(1) as i64;
    let max_side = canvas.max_side.map(|value| value as i64).unwrap_or(i64::MAX);
    let snapped = snap_to_step(value, 0, alignment).max(min_side);
    snapped.min(max_side).max(min_side) as u32
}

pub fn adjust_width(canvas: &CanvasContract, width: i64, height: u32, lock_aspect: Option<f64>) -> (u32, u32) {
    let width = snap_side(canvas, width);
    if let Some(aspect) = lock_aspect.filter(|value| *value > 0.0) {
        return fit_canvas(canvas, aspect, width as u64 * ((width as f64 / aspect).round() as u64).max(1));
    }
    clamp_pair(canvas, width, snap_side(canvas, height as i64))
}

pub fn adjust_height(canvas: &CanvasContract, width: u32, height: i64, lock_aspect: Option<f64>) -> (u32, u32) {
    let height = snap_side(canvas, height);
    if let Some(aspect) = lock_aspect.filter(|value| *value > 0.0) {
        return fit_canvas(
            canvas,
            aspect,
            height as u64 * ((height as f64 * aspect).round() as u64).max(1),
        );
    }
    clamp_pair(canvas, snap_side(canvas, width as i64), height)
}

pub fn scale_to_megapixels(canvas: &CanvasContract, width: u32, height: u32, megapixels: f64) -> (u32, u32) {
    let target = (megapixels.max(0.01) * 1_000_000.0).round() as u64;
    if let Some(preset) = matching_aspect_preset(width, height) {
        return fit_preset(canvas, preset, target);
    }
    fit_canvas(
        canvas,
        width.max(1) as f64 / height.max(1) as f64,
        target,
    )
}

fn fit_exact_ratio(
    canvas: &CanvasContract,
    aspect_w: u32,
    aspect_h: u32,
    target_pixels: u64,
) -> Option<(u32, u32)> {
    let alignment = canvas.alignment.max(1);
    let gcd = gcd(aspect_w.max(1), aspect_h.max(1));
    let aspect_w = aspect_w / gcd;
    let aspect_h = aspect_h / gcd;
    let mut best = None;
    let mut best_error = u64::MAX;
    for scale in 1..=256 {
        let width = alignment.saturating_mul(aspect_w).saturating_mul(scale);
        let height = alignment.saturating_mul(aspect_h).saturating_mul(scale);
        if width == 0 || height == 0 {
            break;
        }
        if !canvas_is_valid(canvas, width as i64, height as i64) {
            if width > 8192 || height > 8192 {
                break;
            }
            continue;
        }
        let pixels = width as u64 * height as u64;
        let error = pixels.abs_diff(target_pixels);
        if error < best_error {
            best = Some((width, height));
            best_error = error;
        }
    }
    best
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

fn clamp_pair(canvas: &CanvasContract, width: u32, height: u32) -> (u32, u32) {
    if canvas_is_valid(canvas, width as i64, height as i64) {
        return (width, height);
    }
    nearest_legal_canvas(canvas, width, height)
}

fn round_up(value: u32, alignment: u32) -> u32 {
    let alignment = alignment.max(1);
    value.div_ceil(alignment) * alignment
}

fn snap_positive(value: i64, alignment: u32) -> i64 {
    snap_to_step(value.max(0), 0, alignment as i64).max(alignment as i64)
}

pub fn megapixels(width: u32, height: u32) -> f64 {
    (width as f64 * height as f64) / 1_000_000.0
}

pub fn canvas_readout(canvas: &CanvasContract, width: u32, height: u32) -> String {
    format!(
        "{width} × {height} · {:.2} MP · /{} grid",
        megapixels(width, height),
        canvas.alignment.max(1)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ltx_dev() -> CanvasContract {
        CanvasContract {
            alignment: 64,
            min_side: 64,
            max_side: None,
            max_pixels: Some(942_080),
            max_aspect: None,
        }
    }

    fn h3() -> CanvasContract {
        CanvasContract {
            alignment: 32,
            min_side: 64,
            max_side: None,
            max_pixels: Some(1_032_192),
            max_aspect: Some(4.0),
        }
    }

    #[test]
    fn project_preview_size_snaps_to_exact_16_by_9_on_ltx_dev_grid() {
        assert_eq!(nearest_legal_canvas(&ltx_dev(), 960, 540), (1024, 576));
        assert_eq!(nearest_legal_canvas(&ltx_dev(), 1280, 720), (1024, 576));
        assert_eq!(nearest_legal_canvas(&ltx_dev(), 1024, 576), (1024, 576));
    }

    #[test]
    fn already_legal_h3_default_is_left_alone() {
        assert_eq!(nearest_legal_canvas(&h3(), 960, 544), (960, 544));
    }

    #[test]
    fn off_grid_values_fail_validation_instead_of_rewriting() {
        let error = validate_canvas(&ltx_dev(), 960, 540).expect_err("off-grid");
        assert!(error.contains("divisible by 64"));
        assert!(error.contains("960x540"));
    }

    #[test]
    fn integer_steps_snap_from_the_published_origin() {
        let ui = InputUi {
            min: Some(64.0),
            max: Some(2048.0),
            step: Some(64.0),
            ..InputUi::default()
        };
        assert_eq!(snap_integer_to_input(962, Some(&ui)), 960);
        assert_eq!(snap_integer_to_input(33, Some(&ui)), 64);
    }

    #[test]
    fn sixteen_by_nine_is_detected_exactly() {
        assert_eq!(matching_aspect_preset(960, 540).map(|preset| preset.label), Some("16:9"));
        assert_eq!(matching_aspect_preset(960, 544).map(|preset| preset.label), None);
    }
}
