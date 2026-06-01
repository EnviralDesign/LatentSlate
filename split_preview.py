from pathlib import Path

repo_root = Path(__file__).resolve().parent
text = (repo_root / 'src/core/preview.rs').read_text(encoding='utf-8')

frame_cache_start = text.index('struct FrameCache')
frame_cache_impl_end = text.index('/// Generates composited preview frames')
cache_block = text[frame_cache_start:frame_cache_impl_end]

renderer_start = text.index('/// Generates composited preview frames')
plate_start = text.index('impl PreviewRenderer {\n    fn plate_images')
renderer_block = text[renderer_start:plate_start]

layers_start = text.index('struct PendingDecode')
utils_start = text.index('fn clamp_time')
layers_block = text[layers_start:utils_start]

utils_block = text[utils_start:]

# Types block from start to FrameCache
types_block = text[:frame_cache_start]

# Extract top uses and constants/types from types_block
lines = types_block.splitlines()

# Identify first type (PreviewStats) and constants range
preview_stats_idx = next(i for i, line in enumerate(lines) if line.strip().startswith('#[derive') and 'PreviewStats' in lines[i+1])

# Extract constants by range
const_start = next(i for i, line in enumerate(lines) if line.startswith('const DEFAULT_MAX_PREVIEW_WIDTH'))
const_end = preview_stats_idx

uses = []
rest = []
for line in lines[:const_start]:
    if line.startswith('use '):
        uses.append(line)

const_block = '\n'.join(lines[const_start:const_end]).strip()
type_block = '\n'.join(lines[preview_stats_idx:]).strip()

preview_dir = repo_root / 'src/core/preview'
preview_dir.mkdir(exist_ok=True)

mod_rs = '''//! Preview rendering system\n//!\n//! Generates composited preview frames for the current timeline time.\n\nmod renderer;\nmod cache;\nmod layers;\nmod types;\nmod utils;\n\npub use renderer::PreviewRenderer;\npub use cache::{FrameCache, CachedFrame};\npub use layers::{PreviewLayer, PreviewLayerStack, PreviewLayerGpu};\npub use types::*;\npub use utils::*;\n'''

# types.rs: constants + types only
const_block = const_block + '\n' if const_block else ''
types_rs = const_block + type_block + '\n'

cache_rs = (
    'use std::collections::{HashMap, HashSet, VecDeque};\n'
    'use std::path::{Path, PathBuf};\n\n'
    'use image::RgbaImage;\n\n'
    'use super::{FrameKey, CachedFrame};\n\n'
    + cache_block.strip()
    + '\n'
)

renderer_rs = (
    'use std::path::{Path, PathBuf};\n'
    'use std::sync::{Arc, Mutex};\n'
    'use std::time::Instant;\n\n'
    'use image::{Rgba, RgbaImage};\n\n'
    'use crate::core::preview_store;\n'
    'use crate::core::video_decode::{DecodeMode, VideoDecodeWorker};\n'
    'use crate::state::{Asset, ClipTransform, Project, TrackType};\n\n'
    'use super::{\n'
    '    cache::FrameCache,\n'
    '    layers::{PreviewLayer, PreviewLayerGpu, PreviewLayerPlacement, PreviewLayerStack},\n'
    '    types::{PreviewDecodeMode, PreviewFrameInfo, PreviewStats, RenderOutput},\n'
    '    utils::{\n'
    '        clamp_time, elapsed_ms, frame_index_to_time, image_size_bytes, scale_image_to_fit,\n'
    '        time_to_frame_index, track_lane_id,\n'
    '    },\n'
    '    DecodedFrame, FrameKey, PendingDecode, PlateCache,\n'
    '};\n\n'
    + renderer_block.strip()
    + '\n'
)

layers_rs = (
    'use std::borrow::Cow;\n'
    'use std::path::{Path, PathBuf};\n'
    'use std::sync::Arc;\n\n'
    'use image::{Rgba, RgbaImage};\n'
    'use image::imageops::{overlay, resize, FilterType};\n'
    'use imageproc::geometric_transformations::{rotate_about_center, Interpolation};\n\n'
    'use crate::state::{Asset, AssetKind, ClipTransform};\n\n'
    'use super::types::{PreviewLayerPlacement};\n\n'
    + layers_block.strip()
    + '\n'
)

utils_rs = (
    'use std::path::Path;\n'
    'use std::time::Instant;\n\n'
    'use image::{Rgba, RgbaImage};\n'
    'use image::imageops::{resize, FilterType};\n\n'
    'use crate::state::{Asset, AssetKind};\n\n'
    + utils_block.strip()
    + '\n'
)

(preview_dir / 'mod.rs').write_text(mod_rs, encoding='utf-8')
(preview_dir / 'types.rs').write_text(types_rs, encoding='utf-8')
(preview_dir / 'cache.rs').write_text(cache_rs, encoding='utf-8')
(preview_dir / 'renderer.rs').write_text(renderer_rs, encoding='utf-8')
(preview_dir / 'layers.rs').write_text(layers_rs, encoding='utf-8')
(preview_dir / 'utils.rs').write_text(utils_rs, encoding='utf-8')

(repo_root / 'src/core/preview.rs').unlink(missing_ok=True)
