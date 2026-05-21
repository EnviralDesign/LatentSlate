//! Audio playback engine (cpal mixer + audio clock).

#![allow(dead_code)]

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat};

#[derive(Clone)]
pub struct PlaybackItem {
    pub samples: Arc<Vec<f32>>,
    pub start_frame: u64,
    pub sample_offset_frames: u64,
    pub frame_count: u64,
    pub channels: u16,
    pub gain: f32,
}

impl PlaybackItem {
    pub fn frames(&self) -> u64 {
        self.frame_count
    }

    pub fn end_frame(&self) -> u64 {
        self.start_frame + self.frames()
    }
}

pub struct AudioPlaybackEngine {
    stream: cpal::Stream,
    items: Arc<Mutex<Vec<PlaybackItem>>>,
    playing: Arc<AtomicBool>,
    playhead_frames: Arc<AtomicU64>,
    scrub_hold: Arc<AtomicBool>,
    scrub_preview_frames: Arc<AtomicU64>,
    sample_rate: u32,
    channels: u16,
    sample_format: SampleFormat,
}

impl AudioPlaybackEngine {
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "No default audio output device found.".to_string())?;
        let output = select_output_config(&device)?;
        let sample_rate = output.config.sample_rate.0;
        let channels = output.config.channels;

        let items = Arc::new(Mutex::new(Vec::<PlaybackItem>::new()));
        let playing = Arc::new(AtomicBool::new(false));
        let playhead_frames = Arc::new(AtomicU64::new(0));
        let scrub_hold = Arc::new(AtomicBool::new(false));
        let scrub_preview_frames = Arc::new(AtomicU64::new(0));

        let channels_for_cb = channels;

        let stream = match output.sample_format {
            SampleFormat::F32 => build_output_stream::<f32>(
                &device,
                &output.config,
                Arc::clone(&items),
                Arc::clone(&playing),
                Arc::clone(&playhead_frames),
                Arc::clone(&scrub_hold),
                Arc::clone(&scrub_preview_frames),
                channels_for_cb,
            )?,
            SampleFormat::I16 => build_output_stream::<i16>(
                &device,
                &output.config,
                Arc::clone(&items),
                Arc::clone(&playing),
                Arc::clone(&playhead_frames),
                Arc::clone(&scrub_hold),
                Arc::clone(&scrub_preview_frames),
                channels_for_cb,
            )?,
            SampleFormat::U16 => build_output_stream::<u16>(
                &device,
                &output.config,
                Arc::clone(&items),
                Arc::clone(&playing),
                Arc::clone(&playhead_frames),
                Arc::clone(&scrub_hold),
                Arc::clone(&scrub_preview_frames),
                channels_for_cb,
            )?,
            SampleFormat::I32 => build_output_stream::<i32>(
                &device,
                &output.config,
                Arc::clone(&items),
                Arc::clone(&playing),
                Arc::clone(&playhead_frames),
                Arc::clone(&scrub_hold),
                Arc::clone(&scrub_preview_frames),
                channels_for_cb,
            )?,
            SampleFormat::U32 => build_output_stream::<u32>(
                &device,
                &output.config,
                Arc::clone(&items),
                Arc::clone(&playing),
                Arc::clone(&playhead_frames),
                Arc::clone(&scrub_hold),
                Arc::clone(&scrub_preview_frames),
                channels_for_cb,
            )?,
            SampleFormat::F64 => build_output_stream::<f64>(
                &device,
                &output.config,
                Arc::clone(&items),
                Arc::clone(&playing),
                Arc::clone(&playhead_frames),
                Arc::clone(&scrub_hold),
                Arc::clone(&scrub_preview_frames),
                channels_for_cb,
            )?,
            SampleFormat::I8 => build_output_stream::<i8>(
                &device,
                &output.config,
                Arc::clone(&items),
                Arc::clone(&playing),
                Arc::clone(&playhead_frames),
                Arc::clone(&scrub_hold),
                Arc::clone(&scrub_preview_frames),
                channels_for_cb,
            )?,
            SampleFormat::U8 => build_output_stream::<u8>(
                &device,
                &output.config,
                Arc::clone(&items),
                Arc::clone(&playing),
                Arc::clone(&playhead_frames),
                Arc::clone(&scrub_hold),
                Arc::clone(&scrub_preview_frames),
                channels_for_cb,
            )?,
            other => {
                return Err(format!("Unsupported output sample format: {}", other));
            }
        };

        stream.play().map_err(|err| err.to_string())?;

        Ok(Self {
            stream,
            items,
            playing,
            playhead_frames,
            scrub_hold,
            scrub_preview_frames,
            sample_rate,
            channels,
            sample_format: output.sample_format,
        })
    }

    pub fn set_items(&self, items: Vec<PlaybackItem>) {
        if let Ok(mut guard) = self.items.lock() {
            *guard = items;
        }
    }

    pub fn play(&self) {
        self.playing.store(true, Ordering::Relaxed);
    }

    pub fn pause(&self) {
        self.playing.store(false, Ordering::Relaxed);
    }

    pub fn seek_seconds(&self, time_seconds: f64) {
        let frame = (time_seconds.max(0.0) * self.sample_rate as f64).round() as u64;
        self.playhead_frames.store(frame, Ordering::Relaxed);
    }

    pub fn playhead_seconds(&self) -> f64 {
        self.playhead_frames.load(Ordering::Relaxed) as f64 / self.sample_rate as f64
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_format(&self) -> SampleFormat {
        self.sample_format
    }

    pub fn set_scrub_hold(&self, hold: bool) {
        self.scrub_hold.store(hold, Ordering::Relaxed);
        if !hold {
            self.scrub_preview_frames.store(0, Ordering::Relaxed);
        }
    }

    pub fn trigger_scrub_preview(&self, frames: u64) {
        self.scrub_preview_frames.store(frames, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}

struct OutputConfig {
    config: cpal::StreamConfig,
    sample_format: SampleFormat,
}

fn select_output_config(device: &cpal::Device) -> Result<OutputConfig, String> {
    let configs: Vec<_> = device
        .supported_output_configs()
        .map_err(|err| err.to_string())?
        .collect();

    let target_rate = cpal::SampleRate(48_000);
    if let Some(config) = configs.iter().find(|config| {
        config.sample_format() == SampleFormat::F32
            && config.channels() == 2
            && config.min_sample_rate() <= target_rate
            && config.max_sample_rate() >= target_rate
    }) {
        return Ok(OutputConfig {
            config: config.with_sample_rate(target_rate).config(),
            sample_format: SampleFormat::F32,
        });
    }

    if let Some(config) = configs.iter().find(|config| {
        config.sample_format() == SampleFormat::F32
            && config.min_sample_rate() <= target_rate
            && config.max_sample_rate() >= target_rate
    }) {
        return Ok(OutputConfig {
            config: config.with_sample_rate(target_rate).config(),
            sample_format: SampleFormat::F32,
        });
    }

    let default_config = device
        .default_output_config()
        .map_err(|err| err.to_string())?;
    Ok(OutputConfig {
        config: default_config.config(),
        sample_format: default_config.sample_format(),
    })
}

fn build_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    items: Arc<Mutex<Vec<PlaybackItem>>>,
    playing: Arc<AtomicBool>,
    playhead: Arc<AtomicU64>,
    scrub_hold: Arc<AtomicBool>,
    scrub_preview_frames: Arc<AtomicU64>,
    channels: u16,
) -> Result<cpal::Stream, String>
where
    T: Sample + FromSample<f32> + cpal::SizedSample,
{
    let mut mix_buffer: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                if !playing.load(Ordering::Relaxed) {
                    for sample in data.iter_mut() {
                        *sample = T::from_sample(0.0);
                    }
                    return;
                }

                let frames = data.len() / channels as usize;
                if mix_buffer.len() != data.len() {
                    mix_buffer.resize(data.len(), 0.0);
                }
                for sample in mix_buffer.iter_mut() {
                    *sample = 0.0;
                }

                let start_frame = playhead.load(Ordering::Relaxed);
                let end_frame = start_frame + frames as u64;

                if let Ok(items) = items.lock() {
                    for item in items.iter() {
                        if item.channels != channels {
                            continue;
                        }
                        let item_start = item.start_frame;
                        let item_end = item.end_frame();
                        if item_end <= start_frame || item_start >= end_frame {
                            continue;
                        }

                        let overlap_start = start_frame.max(item_start);
                        let overlap_end = end_frame.min(item_end);
                        let overlap_frames = (overlap_end - overlap_start) as usize;
                        let buffer_offset =
                            (overlap_start - start_frame) as usize * channels as usize;
                        let item_offset_frames =
                            (overlap_start - item_start) + item.sample_offset_frames;
                        let item_offset = item_offset_frames as usize * channels as usize;

                        let slice_end = item_offset + overlap_frames * channels as usize;
                        if slice_end > item.samples.len() {
                            continue;
                        }

                        for i in 0..(overlap_frames * channels as usize) {
                            mix_buffer[buffer_offset + i] +=
                                item.samples[item_offset + i] * item.gain;
                        }
                    }
                }

                if scrub_hold.load(Ordering::Relaxed) {
                    let preview_remaining = scrub_preview_frames.load(Ordering::Relaxed);
                    if preview_remaining == 0 {
                        for sample in data.iter_mut() {
                            *sample = T::from_sample(0.0);
                        }
                        return;
                    }
                    let consumed = preview_remaining.saturating_sub(frames as u64);
                    scrub_preview_frames.store(consumed, Ordering::Relaxed);
                }

                for (out, sample) in data.iter_mut().zip(mix_buffer.iter()) {
                    *out = T::from_sample(sample.clamp(-1.0, 1.0));
                }
                if !scrub_hold.load(Ordering::Relaxed) {
                    playhead.store(end_frame, Ordering::Relaxed);
                }
            },
            move |err| {
                println!("Audio output error: {}", err);
            },
            None,
        )
        .map_err(|err| err.to_string())
}
