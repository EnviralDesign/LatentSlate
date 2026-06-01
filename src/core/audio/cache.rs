//! Audio cache storage and invalidation for waveform peaks.

#![allow(dead_code)]

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

const PEAK_MAGIC: [u8; 4] = *b"LTS1";
const PEAK_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug)]
pub struct PeakPair {
    pub min_l: i16,
    pub max_l: i16,
    pub min_r: i16,
    pub max_r: i16,
}

#[derive(Clone, Debug)]
pub struct PeakLevel {
    pub block_size: usize,
    pub peaks: Vec<PeakPair>,
}

#[derive(Clone, Debug)]
pub struct PeakCache {
    pub sample_rate: u32,
    pub channels: u16,
    pub source_size: u64,
    pub source_mtime: u64,
    pub levels: Vec<PeakLevel>,
}

pub fn peak_cache_path(project_root: &Path, asset_id: Uuid) -> PathBuf {
    project_root
        .join(".cache")
        .join("audio")
        .join("peaks")
        .join(format!("{}.peaks", asset_id))
}

pub fn load_peak_cache(path: &Path) -> Result<PeakCache, String> {
    let mut file = File::open(path).map_err(|err| err.to_string())?;
    let mut magic = [0_u8; 4];
    file.read_exact(&mut magic).map_err(|err| err.to_string())?;
    if magic != PEAK_MAGIC {
        return Err("Invalid peak cache magic.".to_string());
    }
    let version = read_u32(&mut file)?;
    if version != PEAK_VERSION {
        return Err(format!("Unsupported peak cache version {}", version));
    }
    let sample_rate = read_u32(&mut file)?;
    let channels = read_u16(&mut file)?;
    let level_count = read_u16(&mut file)? as usize;
    let source_size = read_u64(&mut file)?;
    let source_mtime = read_u64(&mut file)?;

    let mut level_info = Vec::with_capacity(level_count);
    for _ in 0..level_count {
        let block_size = read_u32(&mut file)? as usize;
        let peak_count = read_u32(&mut file)? as usize;
        level_info.push((block_size, peak_count));
    }

    let mut levels = Vec::with_capacity(level_count);
    for (block_size, peak_count) in level_info {
        let mut peaks = Vec::with_capacity(peak_count);
        for _ in 0..peak_count {
            if channels == 1 {
                let min = read_i16(&mut file)?;
                let max = read_i16(&mut file)?;
                peaks.push(PeakPair {
                    min_l: min,
                    max_l: max,
                    min_r: min,
                    max_r: max,
                });
            } else {
                peaks.push(PeakPair {
                    min_l: read_i16(&mut file)?,
                    max_l: read_i16(&mut file)?,
                    min_r: read_i16(&mut file)?,
                    max_r: read_i16(&mut file)?,
                });
            }
        }
        levels.push(PeakLevel { block_size, peaks });
    }

    Ok(PeakCache {
        sample_rate,
        channels,
        source_size,
        source_mtime,
        levels,
    })
}

pub fn write_peak_cache(path: &Path, cache: &PeakCache) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut file = File::create(path).map_err(|err| err.to_string())?;
    file.write_all(&PEAK_MAGIC).map_err(|err| err.to_string())?;
    write_u32(&mut file, PEAK_VERSION)?;
    write_u32(&mut file, cache.sample_rate)?;
    write_u16(&mut file, cache.channels)?;
    write_u16(&mut file, cache.levels.len() as u16)?;
    write_u64(&mut file, cache.source_size)?;
    write_u64(&mut file, cache.source_mtime)?;

    for level in cache.levels.iter() {
        write_u32(&mut file, level.block_size as u32)?;
        write_u32(&mut file, level.peaks.len() as u32)?;
    }

    for level in cache.levels.iter() {
        for peak in level.peaks.iter() {
            if cache.channels == 1 {
                write_i16(&mut file, peak.min_l)?;
                write_i16(&mut file, peak.max_l)?;
            } else {
                write_i16(&mut file, peak.min_l)?;
                write_i16(&mut file, peak.max_l)?;
                write_i16(&mut file, peak.min_r)?;
                write_i16(&mut file, peak.max_r)?;
            }
        }
    }

    Ok(())
}

pub fn cache_matches_source(cache: &PeakCache, source_path: &Path) -> Result<bool, String> {
    let (size, mtime) = source_identity(source_path)?;
    Ok(cache.source_size == size && cache.source_mtime == mtime)
}

pub fn source_identity(path: &Path) -> Result<(u64, u64), String> {
    let meta = fs::metadata(path).map_err(|err| err.to_string())?;
    let size = meta.len();
    let mtime = modified_seconds(&meta.modified().map_err(|err| err.to_string())?);
    Ok((size, mtime))
}

fn modified_seconds(time: &SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_u16(file: &mut File) -> Result<u16, String> {
    let mut buf = [0_u8; 2];
    file.read_exact(&mut buf).map_err(|err| err.to_string())?;
    Ok(u16::from_le_bytes(buf))
}

fn read_u32(file: &mut File) -> Result<u32, String> {
    let mut buf = [0_u8; 4];
    file.read_exact(&mut buf).map_err(|err| err.to_string())?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64(file: &mut File) -> Result<u64, String> {
    let mut buf = [0_u8; 8];
    file.read_exact(&mut buf).map_err(|err| err.to_string())?;
    Ok(u64::from_le_bytes(buf))
}

fn read_i16(file: &mut File) -> Result<i16, String> {
    let mut buf = [0_u8; 2];
    file.read_exact(&mut buf).map_err(|err| err.to_string())?;
    Ok(i16::from_le_bytes(buf))
}

fn write_u16(file: &mut File, value: u16) -> Result<(), String> {
    file.write_all(&value.to_le_bytes())
        .map_err(|err| err.to_string())
}

fn write_u32(file: &mut File, value: u32) -> Result<(), String> {
    file.write_all(&value.to_le_bytes())
        .map_err(|err| err.to_string())
}

fn write_u64(file: &mut File, value: u64) -> Result<(), String> {
    file.write_all(&value.to_le_bytes())
        .map_err(|err| err.to_string())
}

fn write_i16(file: &mut File, value: i16) -> Result<(), String> {
    file.write_all(&value.to_le_bytes())
        .map_err(|err| err.to_string())
}
