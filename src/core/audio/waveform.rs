//! Waveform peak extraction and drawing support.

#![allow(dead_code)]

use std::path::Path;

use tokio::task;
use uuid::Uuid;

use super::cache::{
    peak_cache_path, source_identity, write_peak_cache, PeakCache, PeakLevel, PeakPair,
};
use super::decode::{decode_audio_chunks, AudioDecodeConfig};
use crate::state::{Asset, AssetKind};

const PEAK_BASE_BLOCK: usize = 256;
const PEAK_LEVEL_FACTOR: usize = 4;
const PEAK_MAX_LEVELS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct PeakBuildConfig {
    pub base_block: usize,
    pub level_factor: usize,
    pub max_levels: usize,
    pub target_rate: u32,
    pub target_channels: u16,
}

impl Default for PeakBuildConfig {
    fn default() -> Self {
        Self {
            base_block: PEAK_BASE_BLOCK,
            level_factor: PEAK_LEVEL_FACTOR,
            max_levels: PEAK_MAX_LEVELS,
            target_rate: 48_000,
            target_channels: 2,
        }
    }
}

pub fn build_peak_cache(source_path: &Path, config: PeakBuildConfig) -> Result<PeakCache, String> {
    let (source_size, source_mtime) = source_identity(source_path)?;
    let mut accumulator = PeakAccumulator::new(config.base_block);

    decode_audio_chunks(
        source_path,
        AudioDecodeConfig {
            target_rate: config.target_rate,
            target_channels: config.target_channels,
        },
        |chunk| {
            accumulator.push_interleaved(chunk);
            true
        },
    )?;

    let base_peaks = accumulator.finish();
    let levels = build_levels(
        base_peaks,
        config.base_block,
        config.level_factor,
        config.max_levels,
    );

    Ok(PeakCache {
        sample_rate: config.target_rate,
        channels: config.target_channels,
        source_size,
        source_mtime,
        levels,
    })
}

pub fn build_and_store_peak_cache(
    project_root: &Path,
    asset_id: Uuid,
    source_path: &Path,
    config: PeakBuildConfig,
) -> Result<std::path::PathBuf, String> {
    let cache = build_peak_cache(source_path, config)?;
    let cache_path = peak_cache_path(project_root, asset_id);
    write_peak_cache(&cache_path, &cache)?;
    Ok(cache_path)
}

pub fn spawn_peak_cache_build(
    project_root: std::path::PathBuf,
    asset_id: Uuid,
    source_path: std::path::PathBuf,
    config: PeakBuildConfig,
) -> task::JoinHandle<Result<std::path::PathBuf, String>> {
    task::spawn_blocking(move || {
        build_and_store_peak_cache(&project_root, asset_id, &source_path, config)
    })
}

pub fn resolve_audio_source(project_root: &Path, asset: &Asset) -> Option<std::path::PathBuf> {
    match &asset.kind {
        AssetKind::Audio { path } => Some(project_root.join(path)),
        AssetKind::GenerativeAudio {
            folder,
            active_version,
            ..
        } => resolve_generative_audio_source(project_root, folder, active_version.as_deref()),
        _ => None,
    }
}

pub fn resolve_audio_or_video_source(
    project_root: &Path,
    asset: &Asset,
) -> Option<std::path::PathBuf> {
    match &asset.kind {
        AssetKind::Audio { path } => Some(project_root.join(path)),
        AssetKind::Video { path } => Some(project_root.join(path)),
        AssetKind::GenerativeAudio {
            folder,
            active_version,
            ..
        } => resolve_generative_audio_source(project_root, folder, active_version.as_deref()),
        AssetKind::GenerativeVideo {
            folder,
            active_version,
            ..
        } => resolve_generative_video_source(project_root, folder, active_version.as_deref()),
        _ => None,
    }
}

fn build_levels(
    base_peaks: Vec<PeakPair>,
    base_block: usize,
    factor: usize,
    max_levels: usize,
) -> Vec<PeakLevel> {
    let mut levels = Vec::new();
    levels.push(PeakLevel {
        block_size: base_block,
        peaks: base_peaks,
    });

    while levels.len() < max_levels {
        let prev = levels.last().unwrap();
        if prev.peaks.len() <= 1 {
            break;
        }
        let next_peaks = combine_peaks(&prev.peaks, factor);
        if next_peaks.len() == prev.peaks.len() {
            break;
        }
        levels.push(PeakLevel {
            block_size: prev.block_size * factor,
            peaks: next_peaks,
        });
    }

    levels
}

fn combine_peaks(peaks: &[PeakPair], factor: usize) -> Vec<PeakPair> {
    if factor == 0 {
        return peaks.to_vec();
    }

    let mut combined = Vec::new();
    for chunk in peaks.chunks(factor) {
        let mut min_l = i16::MAX;
        let mut max_l = i16::MIN;
        let mut min_r = i16::MAX;
        let mut max_r = i16::MIN;
        for peak in chunk {
            min_l = min_l.min(peak.min_l);
            max_l = max_l.max(peak.max_l);
            min_r = min_r.min(peak.min_r);
            max_r = max_r.max(peak.max_r);
        }
        combined.push(PeakPair {
            min_l,
            max_l,
            min_r,
            max_r,
        });
    }

    combined
}

struct PeakAccumulator {
    block_size: usize,
    count: usize,
    min_l: f32,
    max_l: f32,
    min_r: f32,
    max_r: f32,
    peaks: Vec<PeakPair>,
}

impl PeakAccumulator {
    fn new(block_size: usize) -> Self {
        Self {
            block_size: block_size.max(1),
            count: 0,
            min_l: 1.0,
            max_l: -1.0,
            min_r: 1.0,
            max_r: -1.0,
            peaks: Vec::new(),
        }
    }

    fn push_interleaved(&mut self, samples: &[f32]) {
        for frame in samples.chunks_exact(2) {
            self.push_frame(frame[0], frame[1]);
        }
    }

    fn push_frame(&mut self, left: f32, right: f32) {
        self.min_l = self.min_l.min(left);
        self.max_l = self.max_l.max(left);
        self.min_r = self.min_r.min(right);
        self.max_r = self.max_r.max(right);
        self.count += 1;

        if self.count >= self.block_size {
            self.flush_block();
        }
    }

    fn finish(mut self) -> Vec<PeakPair> {
        if self.count > 0 {
            self.flush_block();
        }
        self.peaks
    }

    fn flush_block(&mut self) {
        let min_l = to_i16(self.min_l);
        let max_l = to_i16(self.max_l);
        let min_r = to_i16(self.min_r);
        let max_r = to_i16(self.max_r);
        self.peaks.push(PeakPair {
            min_l,
            max_l,
            min_r,
            max_r,
        });
        self.count = 0;
        self.min_l = 1.0;
        self.max_l = -1.0;
        self.min_r = 1.0;
        self.max_r = -1.0;
    }
}

fn to_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * i16::MAX as f32).round() as i16
}

fn resolve_generative_audio_source(
    project_root: &Path,
    folder: &std::path::PathBuf,
    active_version: Option<&str>,
) -> Option<std::path::PathBuf> {
    let active_version = active_version?;
    let folder_path = project_root.join(folder);
    let extensions = ["wav", "mp3", "ogg", "flac", "m4a"];

    for ext in extensions.iter() {
        let candidate = folder_path.join(format!("{}.{}", active_version, ext));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn resolve_generative_video_source(
    project_root: &Path,
    folder: &std::path::PathBuf,
    active_version: Option<&str>,
) -> Option<std::path::PathBuf> {
    let active_version = active_version?;
    let folder_path = project_root.join(folder);
    let extensions = ["mp4", "mov", "mkv", "webm", "avi"];

    for ext in extensions.iter() {
        let candidate = folder_path.join(format!("{}.{}", active_version, ext));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}
