//! Audio decoding helpers (ffmpeg-based in MVP).

#![allow(dead_code)]

use ffmpeg::channel_layout::ChannelLayout;
use ffmpeg::codec;
use ffmpeg::format;
use ffmpeg::frame;
use ffmpeg::media;
use ffmpeg_next as ffmpeg;
use std::path::Path;

use crate::core::ffmpeg_runtime::init_ffmpeg;

use super::resample::{frame_to_f32_interleaved, AudioResampleConfig, AudioResampler};

#[derive(Clone, Copy, Debug)]
pub struct AudioDecodeConfig {
    pub target_rate: u32,
    pub target_channels: u16,
}

impl Default for AudioDecodeConfig {
    fn default() -> Self {
        Self {
            target_rate: 48_000,
            target_channels: 2,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AudioDecodeMeta {
    pub source_rate: u32,
    pub source_channels: u16,
    pub target_rate: u32,
    pub target_channels: u16,
    pub duration_seconds: Option<f64>,
}

pub struct AudioDecodeResult {
    pub meta: AudioDecodeMeta,
    pub samples: Vec<f32>,
}

pub fn decode_audio_to_f32(
    path: &Path,
    config: AudioDecodeConfig,
) -> Result<AudioDecodeResult, String> {
    let mut samples = Vec::new();
    let meta = decode_audio_chunks(path, config, |chunk| {
        samples.extend_from_slice(chunk);
        true
    })?;
    Ok(AudioDecodeResult { meta, samples })
}

pub fn decode_audio_chunks<F>(
    path: &Path,
    config: AudioDecodeConfig,
    mut on_samples: F,
) -> Result<AudioDecodeMeta, String>
where
    F: FnMut(&[f32]) -> bool,
{
    init_ffmpeg()?;

    let mut ictx = format::input(path).map_err(|err| err.to_string())?;
    let stream = ictx
        .streams()
        .best(media::Type::Audio)
        .ok_or_else(|| "No audio stream found".to_string())?;
    let stream_index = stream.index();
    let time_base = stream.time_base();
    let duration_seconds = if stream.duration() > 0 {
        let numerator = time_base.numerator() as f64;
        let denominator = time_base.denominator() as f64;
        if numerator > 0.0 && denominator > 0.0 {
            Some(stream.duration() as f64 * numerator / denominator)
        } else {
            None
        }
    } else {
        None
    };

    let codec_context = codec::context::Context::from_parameters(stream.parameters())
        .map_err(|err| format!("Failed to create audio codec context: {}", err))?;
    let mut decoder = codec_context
        .decoder()
        .audio()
        .map_err(|err| format!("Failed to create audio decoder: {}", err))?;

    let mut layout = decoder.channel_layout();
    if layout.is_empty() {
        layout = ChannelLayout::default(decoder.channels() as i32);
        decoder.set_channel_layout(layout);
    }

    let source_rate = decoder.rate();
    let source_channels = decoder.channels();

    let mut resampler = AudioResampler::new(
        decoder.format(),
        source_rate,
        layout,
        AudioResampleConfig {
            target_rate: config.target_rate,
            target_channels: config.target_channels,
        },
    )?;

    let meta = AudioDecodeMeta {
        source_rate,
        source_channels,
        target_rate: resampler.target_rate(),
        target_channels: resampler.target_channels(),
        duration_seconds,
    };

    let mut decoded = frame::Audio::empty();
    let mut total_samples = 0_usize;

    for (stream, packet) in ictx.packets() {
        if stream.index() != stream_index {
            continue;
        }
        decoder
            .send_packet(&packet)
            .map_err(|err| err.to_string())?;
        drain_decoder(
            &mut decoder,
            &mut resampler,
            &mut decoded,
            &mut on_samples,
            &mut total_samples,
        )?;
    }

    decoder.send_eof().map_err(|err| err.to_string())?;
    drain_decoder(
        &mut decoder,
        &mut resampler,
        &mut decoded,
        &mut on_samples,
        &mut total_samples,
    )?;
    flush_resampler(&mut resampler, &mut on_samples, &mut total_samples)?;

    Ok(meta)
}

fn drain_decoder<F>(
    decoder: &mut ffmpeg::decoder::Audio,
    resampler: &mut AudioResampler,
    decoded: &mut frame::Audio,
    on_samples: &mut F,
    total_samples: &mut usize,
) -> Result<(), String>
where
    F: FnMut(&[f32]) -> bool,
{
    while decoder.receive_frame(decoded).is_ok() {
        let resampled = resampler.resample(decoded)?;
        if resampled.samples() == 0 {
            continue;
        }
        let buffer = frame_to_f32_interleaved(&resampled)?;
        *total_samples = total_samples.saturating_add(buffer.len());
        if !on_samples(&buffer) {
            return Ok(());
        }
    }
    Ok(())
}

fn flush_resampler<F>(
    resampler: &mut AudioResampler,
    on_samples: &mut F,
    total_samples: &mut usize,
) -> Result<(), String>
where
    F: FnMut(&[f32]) -> bool,
{
    loop {
        let Some(frame) = resampler.flush()? else {
            break;
        };
        let buffer = frame_to_f32_interleaved(&frame)?;
        *total_samples = total_samples.saturating_add(buffer.len());
        if !on_samples(&buffer) {
            break;
        }
    }
    Ok(())
}
