//! Exact width/height canvas contracts for generation providers.
//!
//! LatentSlate Engine publishes the legal pixel grid. The editor snaps fallbacks
//! and picker values onto that grid, but never rewrites an explicit off-grid
//! literal: generate fails early instead.

use crate::state::{
    CanvasContract, InputRole, InputUi, ProviderEntry, ProviderInputField, ProviderInputType,
};

pub const ASPECT_PRESETS: [AspectPreset; 10] = [
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
        label: "3:2",
        width: 3,
        height: 2,
    },
    AspectPreset {
        label: "2:3",
        width: 2,
        height: 3,
    },
    AspectPreset {
        label: "5:4",
        width: 5,
        height: 4,
    },
    AspectPreset {
        label: "4:5",
        width: 4,
        height: 5,
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

/// Direction used by provider-grid nudge controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridNudgeDirection {
    /// Move to the next supported value below the current value.
    Down,
    /// Move to the next supported value above the current value.
    Up,
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

pub fn canvas_from_dimension_ui(
    width: Option<&InputUi>,
    height: Option<&InputUi>,
) -> Option<CanvasContract> {
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

/// Returns the next provider-grid value in `direction`, bounded by the canvas limits.
///
/// An already aligned value advances by one full grid step. An off-grid value
/// rounds directionally to the adjacent supported value.
pub fn nudge_grid_value(
    value: i64,
    alignment: u32,
    min: i64,
    max: Option<i64>,
    direction: GridNudgeDirection,
) -> Option<i64> {
    let step = alignment.max(1) as i64;
    let min_grid = ceil_to_step(min, step);
    let max_grid = floor_to_step(max.unwrap_or(i64::MAX), step);
    if min_grid > max_grid {
        return None;
    }

    let remainder = value.rem_euclid(step);
    let candidate = match direction {
        GridNudgeDirection::Down => {
            let directional = if remainder == 0 {
                value.checked_sub(step)?
            } else {
                value.saturating_sub(remainder)
            };
            directional.min(max_grid)
        }
        GridNudgeDirection::Up => {
            let directional = if remainder == 0 {
                value.checked_add(step)?
            } else {
                value.saturating_add(step - remainder)
            };
            directional.max(min_grid)
        }
    };

    (min_grid..=max_grid)
        .contains(&candidate)
        .then_some(candidate)
}

#[cfg(test)]
fn snap_integer_to_input(value: i64, ui: Option<&InputUi>) -> i64 {
    let Some(ui) = ui else {
        return value;
    };
    let step = ui
        .step
        .filter(|step| *step >= 1.0)
        .map(|step| step.round() as i64)
        .unwrap_or(1)
        .max(1);
    let origin = ui.min.map(|min| min.round() as i64).unwrap_or(0);
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
            let pixel_err =
                ((pixels as f64 - target_pixels as f64).abs()) / target_pixels.max(1) as f64;
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

/// Nudges to the adjacent exact-aspect target on the provider's pixel grid.
///
/// This enumerates target sizes rather than guaranteed provider outputs. The
/// ordinary canvas resolver still applies pixel budgets and other provider
/// limits, allowing the UI to warn when an exact target cannot be produced.
pub fn nudge_exact_aspect_target(
    canvas: &CanvasContract,
    aspect: AspectPreset,
    target_pixels: u64,
    max_target_side: u32,
    direction: GridNudgeDirection,
) -> Option<(u32, u32)> {
    let alignment = canvas.alignment.max(1);
    let gcd = gcd(aspect.width.max(1), aspect.height.max(1));
    let aspect_width = aspect.width / gcd;
    let aspect_height = aspect.height / gcd;
    let width_unit = alignment.checked_mul(aspect_width)?;
    let height_unit = alignment.checked_mul(aspect_height)?;
    let min_side = canvas.min_side.max(1);
    let min_scale = min_side
        .div_ceil(width_unit.max(1))
        .max(min_side.div_ceil(height_unit.max(1)))
        .max(1);
    let max_scale = max_target_side
        .checked_div(width_unit.max(1))?
        .min(max_target_side.checked_div(height_unit.max(1))?);
    if min_scale > max_scale {
        return None;
    }

    match direction {
        GridNudgeDirection::Down => (min_scale..=max_scale).rev().find_map(|scale| {
            let width = width_unit.checked_mul(scale)?;
            let height = height_unit.checked_mul(scale)?;
            let pixels = width as u64 * height as u64;
            (pixels < target_pixels).then_some((width, height))
        }),
        GridNudgeDirection::Up => (min_scale..=max_scale).find_map(|scale| {
            let width = width_unit.checked_mul(scale)?;
            let height = height_unit.checked_mul(scale)?;
            let pixels = width as u64 * height as u64;
            (pixels > target_pixels).then_some((width, height))
        }),
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

/// Fits a legal canvas whose pixel area is at least the requested size when
/// the provider contract permits it.
///
/// The boolean is `false` when hard provider limits force the returned canvas
/// below the requested pixel area.
pub fn fit_canvas_at_least(
    canvas: &CanvasContract,
    target_width: u32,
    target_height: u32,
) -> ((u32, u32), bool) {
    let target_width = target_width.max(1);
    let target_height = target_height.max(1);
    let target_pixels = target_width as u64 * target_height as u64;
    if canvas_is_valid(canvas, target_width as i64, target_height as i64) {
        return ((target_width, target_height), true);
    }

    let alignment = canvas.alignment.max(1);
    let min_side = round_up(canvas.min_side.max(1), alignment);
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
    let target_aspect = target_width as f64 / target_height as f64;
    let mut best_meeting: Option<(f64, u64, u32, u32)> = None;
    let mut best_fallback: Option<(u64, f64, u32, u32)> = None;

    let mut width = min_side;
    while width <= max_side {
        let max_height_for_width = canvas
            .max_pixels
            .map(|pixels| (pixels / width.max(1) as u64).min(max_side as u64) as u32)
            .unwrap_or(max_side);
        let max_height_for_width = (max_height_for_width / alignment) * alignment;
        let required_height = target_pixels.div_ceil(width as u64).min(u32::MAX as u64) as u32;
        let ideal_height = (width as f64 / target_aspect).round().max(1.0) as u32;
        let candidates = [
            round_up(required_height.max(min_side), alignment),
            round_up(target_height.max(min_side), alignment),
            snap_positive(ideal_height as i64, alignment) as u32,
            max_height_for_width,
        ];

        for height in candidates {
            if height < min_side
                || height > max_side
                || !canvas_is_valid(canvas, width as i64, height as i64)
            {
                continue;
            }
            let pixels = width as u64 * height as u64;
            let aspect = width as f64 / height as f64;
            let aspect_error = ((aspect - target_aspect) / target_aspect).abs();

            if pixels >= target_pixels {
                let width_error = (width as f64 / target_width as f64 - 1.0).abs();
                let height_error = (height as f64 / target_height as f64 - 1.0).abs();
                let overshoot = (pixels - target_pixels) as f64 / target_pixels as f64;
                let score = width_error + height_error + aspect_error * 0.5 + overshoot * 0.25;
                let should_replace =
                    best_meeting
                        .as_ref()
                        .is_none_or(|(best_score, best_pixels, _, _)| {
                            score < *best_score || (score == *best_score && pixels < *best_pixels)
                        });
                if should_replace {
                    best_meeting = Some((score, pixels, width, height));
                }
            }

            let should_replace_fallback =
                best_fallback
                    .as_ref()
                    .is_none_or(|(best_pixels, best_aspect_error, _, _)| {
                        pixels > *best_pixels
                            || (pixels == *best_pixels && aspect_error < *best_aspect_error)
                    });
            if should_replace_fallback {
                best_fallback = Some((pixels, aspect_error, width, height));
            }
        }

        width = width.saturating_add(alignment);
        if width < alignment {
            break;
        }
    }

    if let Some((_, _, width, height)) = best_meeting {
        ((width, height), true)
    } else if let Some((_, _, width, height)) = best_fallback {
        ((width, height), false)
    } else {
        ((min_side, min_side), false)
    }
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

fn round_up(value: u32, alignment: u32) -> u32 {
    let alignment = alignment.max(1);
    value
        .saturating_add(alignment - 1)
        .div_euclid(alignment)
        .saturating_mul(alignment)
}

fn floor_to_step(value: i64, step: i64) -> i64 {
    value.saturating_sub(value.rem_euclid(step.max(1)))
}

fn ceil_to_step(value: i64, step: i64) -> i64 {
    let step = step.max(1);
    let remainder = value.rem_euclid(step);
    if remainder == 0 {
        value
    } else {
        value.saturating_add(step - remainder)
    }
}

fn snap_positive(value: i64, alignment: u32) -> i64 {
    snap_to_step(value.max(0), 0, alignment as i64).max(alignment as i64)
}

pub fn megapixels(width: u32, height: u32) -> f64 {
    (width as f64 * height as f64) / 1_000_000.0
}

pub fn canvas_readout(canvas: &CanvasContract, width: u32, height: u32) -> String {
    let aspect = matching_aspect_preset(width, height)
        .map(|preset| preset.label.to_string())
        .unwrap_or_else(|| format!("{:.3}:1", width as f64 / height.max(1) as f64));
    format!(
        "{width} × {height} · {:.2} MP · {aspect} · {} px grid",
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
    fn grid_nudges_round_directionally_and_advance_aligned_values() {
        assert_eq!(
            nudge_grid_value(1_001, 64, 64, Some(2_048), GridNudgeDirection::Down),
            Some(960)
        );
        assert_eq!(
            nudge_grid_value(1_001, 64, 64, Some(2_048), GridNudgeDirection::Up),
            Some(1_024)
        );
        assert_eq!(
            nudge_grid_value(1_024, 64, 64, Some(2_048), GridNudgeDirection::Up),
            Some(1_088)
        );
        assert_eq!(
            nudge_grid_value(64, 64, 64, Some(2_048), GridNudgeDirection::Down),
            None
        );
    }

    #[test]
    fn megapixel_nudge_uses_exact_aspect_grid_targets() {
        let canvas = ltx_dev();
        assert_eq!(
            nudge_exact_aspect_target(
                &canvas,
                ASPECT_PRESETS[1],
                490_000,
                4_096,
                GridNudgeDirection::Up,
            ),
            Some((1_024, 576))
        );
        assert_eq!(
            nudge_exact_aspect_target(
                &canvas,
                ASPECT_PRESETS[1],
                589_824,
                4_096,
                GridNudgeDirection::Up,
            ),
            Some((2_048, 1_152))
        );
        assert_eq!(
            nudge_exact_aspect_target(
                &canvas,
                ASPECT_PRESETS[1],
                589_824,
                4_096,
                GridNudgeDirection::Down,
            ),
            None
        );
    }

    #[test]
    fn sixteen_by_nine_is_detected_exactly() {
        assert_eq!(
            matching_aspect_preset(960, 540).map(|preset| preset.label),
            Some("16:9")
        );
        assert_eq!(
            matching_aspect_preset(960, 544).map(|preset| preset.label),
            None
        );
    }
}
