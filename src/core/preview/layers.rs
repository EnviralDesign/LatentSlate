use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use image::imageops::{overlay, resize, FilterType};
use image::{Rgba, RgbaImage};
use imageproc::geometric_transformations::{rotate_about_center, Interpolation};

use crate::state::ClipTransform;

use super::types::{FrameKey, PreviewLayerPlacement};

pub(crate) struct PendingDecode {
    pub(crate) clip_id: uuid::Uuid,
    pub(crate) track_index: usize,
    pub(crate) start_time: f64,
    pub(crate) path: PathBuf,
    pub(crate) frame_time: f64,
    pub(crate) cache_key: FrameKey,
    pub(crate) transform: ClipTransform,
    pub(crate) lane_id: u64,
}

pub(crate) struct DecodedFrame {
    pub(crate) image: RgbaImage,
    pub(crate) source_width: u32,
    pub(crate) source_height: u32,
}

pub(crate) struct PreviewLayer {
    pub(crate) clip_id: uuid::Uuid,
    pub(crate) texture_key: u64,
    pub(crate) track_index: usize,
    pub(crate) start_time: f64,
    pub(crate) image: Arc<RgbaImage>,
    pub(crate) transform: ClipTransform,
    pub(crate) source_width: u32,
    pub(crate) source_height: u32,
}

pub(crate) fn preview_canvas_size(
    project_width: u32,
    project_height: u32,
    max_width: u32,
    max_height: u32,
) -> (u32, u32, f32) {
    if project_width == 0 || project_height == 0 {
        return (max_width, max_height, 1.0);
    }

    let scale_w = max_width as f32 / project_width as f32;
    let scale_h = max_height as f32 / project_height as f32;
    let scale = scale_w.min(scale_h).min(1.0).max(0.01);

    let canvas_w = (project_width as f32 * scale).round().max(1.0) as u32;
    let canvas_h = (project_height as f32 * scale).round().max(1.0) as u32;

    (canvas_w, canvas_h, scale)
}

pub(crate) fn composite_layer(
    canvas: &mut RgbaImage,
    image: &RgbaImage,
    source_width: u32,
    source_height: u32,
    transform: ClipTransform,
    preview_scale: f32,
) {
    let placement = match compute_layer_placement(
        image,
        source_width,
        source_height,
        transform,
        preview_scale,
        canvas.width() as f32,
        canvas.height() as f32,
    ) {
        Some(placement) => placement,
        None => return,
    };

    let image = if placement.opacity < 1.0 {
        let mut working = image.clone();
        apply_opacity(&mut working, placement.opacity);
        Cow::Owned(working)
    } else {
        Cow::Borrowed(image)
    };
    let scaled_w = placement.scaled_w.round() as u32;
    let scaled_h = placement.scaled_h.round() as u32;
    if scaled_w == 0 || scaled_h == 0 {
        return;
    }

    let resized = resize(image.as_ref(), scaled_w, scaled_h, FilterType::Triangle);
    if placement.rotation_deg.abs() <= 0.01 {
        overlay(
            canvas,
            &resized,
            placement.offset_x.round() as i64,
            placement.offset_y.round() as i64,
        );
        return;
    }

    let rotated = rotate_rgba(&resized, placement.rotation_deg);
    let center_x = placement.offset_x + placement.scaled_w * 0.5;
    let center_y = placement.offset_y + placement.scaled_h * 0.5;
    let dest_x = (center_x - rotated.width() as f32 * 0.5).round() as i64;
    let dest_y = (center_y - rotated.height() as f32 * 0.5).round() as i64;
    overlay(canvas, &rotated, dest_x, dest_y);
}

pub(crate) fn rotate_rgba(image: &RgbaImage, rotation_deg: f32) -> RgbaImage {
    let angle = rotation_deg.to_radians();
    let (sin, cos) = angle.sin_cos();
    let abs_sin = sin.abs();
    let abs_cos = cos.abs();
    let src_w = image.width().max(1) as f32;
    let src_h = image.height().max(1) as f32;
    let new_w = (src_w * abs_cos + src_h * abs_sin).ceil().max(1.0) as u32;
    let new_h = (src_w * abs_sin + src_h * abs_cos).ceil().max(1.0) as u32;

    let mut expanded = RgbaImage::from_pixel(new_w, new_h, Rgba([0, 0, 0, 0]));
    let offset_x = ((new_w as f32 - src_w) * 0.5).round() as i64;
    let offset_y = ((new_h as f32 - src_h) * 0.5).round() as i64;
    overlay(&mut expanded, image, offset_x, offset_y);

    rotate_about_center(
        &expanded,
        angle,
        Interpolation::Bilinear,
        Rgba([0, 0, 0, 0]),
    )
}

pub(crate) fn compute_layer_placement(
    image: &RgbaImage,
    source_width: u32,
    source_height: u32,
    transform: ClipTransform,
    preview_scale: f32,
    canvas_w: f32,
    canvas_h: f32,
) -> Option<PreviewLayerPlacement> {
    let (decoded_w, decoded_h) = (image.width().max(1) as f32, image.height().max(1) as f32);
    if decoded_w <= 0.0 || decoded_h <= 0.0 {
        return None;
    }

    let source_w = if source_width > 0 {
        source_width as f32
    } else {
        decoded_w
    };
    let source_h = if source_height > 0 {
        source_height as f32
    } else {
        decoded_h
    };

    let base_scale_x = (source_w * preview_scale) / decoded_w;
    let base_scale_y = (source_h * preview_scale) / decoded_h;
    let scaled_w = decoded_w * base_scale_x * transform.scale_x.max(0.01);
    let scaled_h = decoded_h * base_scale_y * transform.scale_y.max(0.01);
    if scaled_w <= 0.0 || scaled_h <= 0.0 {
        return None;
    }

    let offset_x = ((canvas_w - scaled_w) * 0.5) + (transform.position_x * preview_scale);
    let offset_y = ((canvas_h - scaled_h) * 0.5) + (transform.position_y * preview_scale);
    let opacity = transform.opacity.clamp(0.0, 1.0);

    Some(PreviewLayerPlacement {
        offset_x,
        offset_y,
        scaled_w,
        scaled_h,
        opacity,
        rotation_deg: transform.rotation_deg,
    })
}

pub(crate) fn apply_opacity(image: &mut RgbaImage, opacity: f32) {
    for pixel in image.pixels_mut() {
        let alpha = (pixel.0[3] as f32 * opacity).round().clamp(0.0, 255.0) as u8;
        pixel.0[3] = alpha;
    }
}
