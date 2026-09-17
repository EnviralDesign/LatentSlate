//! Derived, non-persistent audio reference inspection shared by picker and preflight.
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use super::materialize::audio_stream_count;
use super::MediaResolvePlan;
use crate::state::BoundMediaType;

#[derive(Clone, Debug)]
pub enum AudioInspection {
    Checking,
    Analyzing,
    Ready,
    Quiet,
    MissingTrack,
    MultipleTracks,
    Invalid(String),
    LevelUnavailable,
}

impl AudioInspection {
    pub fn blocking_error(&self) -> Option<&str> {
        match self {
            Self::MissingTrack => Some("No audio track in selected source."),
            Self::Invalid(error) => Some(error),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Checking => "Checking audio…",
            Self::Analyzing => "Checking audio level…",
            Self::Ready => "Audio ready",
            Self::Quiet => "Selected range appears silent or very quiet",
            Self::MissingTrack => "No audio track in selected source",
            Self::MultipleTracks => "Multiple audio tracks; level check unavailable",
            Self::Invalid(_) => "Audio unavailable",
            Self::LevelUnavailable => "Audio level check unavailable",
        }
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct Key {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
    range: Option<(i64, i64)>,
}

type Check = Arc<Mutex<AudioInspection>>;
static CHECKS: OnceLock<Mutex<HashMap<Key, Check>>> = OnceLock::new();

/// Starts at most two background inspections. Level checks are advisory; submission
/// independently validates the actual materialized source's audio stream.
pub fn audio_reference_inspection(plan: &MediaResolvePlan) -> Option<AudioInspection> {
    if plan.media_type != BoundMediaType::Audio || !plan.is_ok() {
        return None;
    }
    let path = plan.source_path_absolute.as_ref()?;
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Some(AudioInspection::Invalid(
                "Audio source is unavailable.".into(),
            ))
        }
    };
    let range = if plan.uses_original_source {
        None
    } else {
        plan.source_range
    };
    let key = Key {
        path: path.clone(),
        size: metadata.len(),
        modified: metadata.modified().ok(),
        range: range.map(|r| {
            (
                (r.start_seconds * 1_000_000.0).round() as i64,
                (r.end_seconds * 1_000_000.0).round() as i64,
            )
        }),
    };
    let mut checks = CHECKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()?;
    if let Some(check) = checks.get(&key) {
        return check.lock().ok().map(|state| state.clone());
    }
    let pending = |check: &Check| {
        check
            .lock()
            .map(|state| {
                matches!(
                    *state,
                    AudioInspection::Checking | AudioInspection::Analyzing
                )
            })
            .unwrap_or(false)
    };
    if checks.values().filter(|check| pending(check)).count() >= 2 {
        return Some(AudioInspection::Checking);
    }
    if checks.len() >= 64 {
        if let Some(old) = checks
            .iter()
            .find(|(_, check)| !pending(check))
            .map(|(key, _)| key.clone())
        {
            checks.remove(&old);
        }
    }
    let check = Arc::new(Mutex::new(AudioInspection::Checking));
    checks.insert(key.clone(), check.clone());
    std::thread::spawn(move || {
        let status = inspect_source(&key, &check);
        if let Ok(mut state) = check.lock() {
            *state = status;
        }
    });
    Some(AudioInspection::Checking)
}

fn inspect_source(key: &Key, check: &Check) -> AudioInspection {
    match audio_stream_count(&key.path) {
        Err(error) => AudioInspection::Invalid(error.message("Audio")),
        Ok(0) => AudioInspection::MissingTrack,
        Ok(count) if count > 1 => AudioInspection::MultipleTracks,
        Ok(_) => {
            if let Ok(mut state) = check.lock() {
                *state = AudioInspection::Analyzing;
            }
            inspect_level(&key)
        }
    }
}

fn inspect_level(key: &Key) -> AudioInspection {
    let mut command = Command::new("ffmpeg");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command.args(["-nostdin", "-hide_banner", "-nostats"]);
    if let Some((start, end)) = key.range {
        command.args([
            "-ss",
            &(start as f64 / 1_000_000.0).to_string(),
            "-t",
            &((end - start) as f64 / 1_000_000.0).to_string(),
        ]);
    }
    let output = command
        .arg("-i")
        .arg(&key.path)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-af",
            "volumedetect",
            "-f",
            "null",
            "-",
        ])
        .output();
    let Ok(output) = output else {
        return AudioInspection::LevelUnavailable;
    };
    if !output.status.success() {
        return AudioInspection::LevelUnavailable;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let level = stderr.lines().find_map(|line| {
        line.split_once("max_volume:")?
            .1
            .trim()
            .split_whitespace()
            .next()?
            .parse::<f64>()
            .ok()
    });
    match level {
        Some(db) if db <= -60.0 => AudioInspection::Quiet,
        Some(_) => AudioInspection::Ready,
        None => AudioInspection::LevelUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_level_warning_covers_selected_interval_and_remains_advisory() {
        let root =
            std::env::temp_dir().join(format!("latentslate-audio-level-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("levels.wav");
        assert!(Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=48000:cl=mono:d=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=2",
                "-filter_complex",
                "[0:a][1:a]concat=n=2:v=0:a=1",
                "-c:a",
                "pcm_s16le"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success());
        let mut key = Key {
            path,
            size: 0,
            modified: None,
            range: Some((0, 1_000_000)),
        };
        assert!(matches!(inspect_level(&key), AudioInspection::Quiet));
        key.range = Some((2_500_000, 3_500_000));
        assert!(matches!(inspect_level(&key), AudioInspection::Ready));
        key.range = None;
        assert!(matches!(inspect_level(&key), AudioInspection::Ready));
        let multitrack = root.join("tracks.mka");
        assert!(Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-i"])
            .arg(&key.path)
            .args(["-map", "0:a:0", "-map", "0:a:0", "-c:a", "copy"])
            .arg(&multitrack)
            .status()
            .unwrap()
            .success());
        key.path = multitrack;
        let status = inspect_source(&key, &Arc::new(Mutex::new(AudioInspection::Checking)));
        assert!(matches!(status, AudioInspection::MultipleTracks));
        assert!(status.blocking_error().is_none());
        assert!(AudioInspection::Quiet.blocking_error().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
