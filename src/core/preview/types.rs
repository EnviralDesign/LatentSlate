use std::path::PathBuf;
use std::sync::Arc;

use image::{Rgba, RgbaImage};
use serde::Serialize;
use uuid::Uuid;

pub const FFMPEG_TIME_EPSILON: f64 = 0.001;
#[allow(dead_code)]
// Retained for timeline cache diagnostics once cache bucket overlays are wired
// back into the egui timeline.
pub const MAX_CACHE_BUCKETS: usize = 120;
pub const PLATE_BORDER_WIDTH: u32 = 1;
pub const PLATE_BORDER_COLOR: Rgba<u8> = Rgba([0x27, 0x27, 0x2a, 255]);

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PreviewStats {
    pub total_ms: f64,
    pub collect_ms: f64,
    pub composite_ms: f64,
    pub encode_ms: f64,
    pub video_decode_ms: f64,
    pub video_decode_seek_ms: f64,
    pub video_decode_packet_ms: f64,
    pub video_decode_transfer_ms: f64,
    pub video_decode_scale_ms: f64,
    pub video_decode_copy_ms: f64,
    pub still_load_ms: f64,
    pub hw_decode_frames: usize,
    pub sw_decode_frames: usize,
    pub layers: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PreviewCacheStats {
    pub max_bytes: usize,
    pub total_bytes: usize,
    pub entry_count: usize,
    pub indexed_asset_count: usize,
    pub indexed_frame_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PreviewDecodeMode {
    Seek,
    Sequential,
}

#[derive(Clone, Debug)]
pub struct PreviewRgbaFrame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct RenderRgbaOutput {
    pub frame: Option<PreviewRgbaFrame>,
    #[allow(dead_code)]
    // Export may ignore this today, but retaining it keeps the renderer's RGBA
    // path observable when export diagnostics are added.
    pub stats: PreviewStats,
}

#[derive(Clone, Copy, Debug)]
pub struct PreviewLayerPlacement {
    pub offset_x: f32,
    pub offset_y: f32,
    pub scaled_w: f32,
    pub scaled_h: f32,
    pub opacity: f32,
    pub rotation_deg: f32,
}

#[derive(Clone, Debug)]
pub struct PreviewLayerGpu {
    pub clip_id: Option<Uuid>,
    pub texture_key: u64,
    pub image: Arc<RgbaImage>,
    pub placement: PreviewLayerPlacement,
}

#[derive(Clone, Debug)]
pub struct PreviewLayerStack {
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub layers: Vec<PreviewLayerGpu>,
}

#[derive(Clone, Debug)]
pub struct RenderOutput {
    pub layers: Option<PreviewLayerStack>,
    pub stats: PreviewStats,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct FrameKey {
    pub path: PathBuf,
    pub frame_index: i64,
}

#[derive(Clone)]
pub struct CachedFrame {
    pub image: Arc<RgbaImage>,
    pub source_width: u32,
    pub source_height: u32,
}

pub(crate) struct PlateCache {
    pub width: u32,
    pub height: u32,
    pub fill: Arc<RgbaImage>,
    pub border: Arc<RgbaImage>,
}
