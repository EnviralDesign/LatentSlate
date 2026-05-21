//! Audio resampling utilities.

#![allow(dead_code)]

use ffmpeg_next as ffmpeg;

use ffmpeg::channel_layout::ChannelLayout;
use ffmpeg::format::{sample, Sample};
use ffmpeg::frame;
use ffmpeg::software::resampling::context::Context as ResampleContext;

#[derive(Clone, Copy, Debug)]
pub struct AudioResampleConfig {
    pub target_rate: u32,
    pub target_channels: u16,
}

impl Default for AudioResampleConfig {
    fn default() -> Self {
        Self {
            target_rate: 48_000,
            target_channels: 2,
        }
    }
}

pub struct AudioResampler {
    ctx: ResampleContext,
    target_format: Sample,
    target_layout: ChannelLayout,
}

impl AudioResampler {
    pub fn new(
        input_format: Sample,
        input_rate: u32,
        input_layout: ChannelLayout,
        config: AudioResampleConfig,
    ) -> Result<Self, String> {
        let target_layout = channel_layout_for_channels(config.target_channels);
        let target_format = Sample::F32(sample::Type::Packed);
        let ctx = ResampleContext::get(
            input_format,
            input_layout,
            input_rate,
            target_format,
            target_layout,
            config.target_rate,
        )
        .map_err(|err| err.to_string())?;

        Ok(Self {
            ctx,
            target_format,
            target_layout,
        })
    }

    pub fn target_rate(&self) -> u32 {
        self.ctx.output().rate
    }

    pub fn target_channels(&self) -> u16 {
        self.target_layout.channels() as u16
    }

    pub fn resample(&mut self, input: &frame::Audio) -> Result<frame::Audio, String> {
        let output_samples = estimate_output_samples(&self.ctx, input.samples());
        if output_samples == 0 {
            return Ok(frame::Audio::empty());
        }
        let mut output = frame::Audio::new(self.target_format, output_samples, self.target_layout);
        self.ctx
            .run(input, &mut output)
            .map_err(|err| err.to_string())?;
        Ok(output)
    }

    pub fn flush(&mut self) -> Result<Option<frame::Audio>, String> {
        let delay_samples = self.ctx.delay().map(|delay| delay.output).unwrap_or(0);
        if delay_samples <= 0 {
            return Ok(None);
        }

        let mut output = frame::Audio::new(
            self.target_format,
            delay_samples as usize,
            self.target_layout,
        );
        self.ctx.flush(&mut output).map_err(|err| err.to_string())?;
        if output.samples() == 0 {
            return Ok(None);
        }
        Ok(Some(output))
    }
}

fn estimate_output_samples(ctx: &ResampleContext, input_samples: usize) -> usize {
    if input_samples == 0 {
        return 0;
    }
    let delay = ctx.delay().map(|delay| delay.output).unwrap_or(0);
    let input_rate = ctx.input().rate as i64;
    let output_rate = ctx.output().rate as i64;
    let total = delay + input_samples as i64;
    if input_rate <= 0 || output_rate <= 0 || total <= 0 {
        return 0;
    }
    ((total * output_rate + input_rate - 1) / input_rate) as usize
}

fn channel_layout_for_channels(channels: u16) -> ChannelLayout {
    match channels {
        1 => ChannelLayout::MONO,
        2 => ChannelLayout::STEREO,
        count => ChannelLayout::default(count as i32),
    }
}

pub fn frame_to_f32_interleaved(frame: &frame::Audio) -> Result<Vec<f32>, String> {
    let format = frame.format();
    if format != Sample::F32(sample::Type::Packed) {
        return Err(format!("Expected packed f32 samples, got {:?}", format));
    }
    let data = frame.data(0);
    let expected_samples = frame.samples().saturating_mul(frame.channels() as usize);
    let expected_bytes = expected_samples.saturating_mul(std::mem::size_of::<f32>());
    if expected_samples == 0 {
        return Ok(Vec::new());
    }
    if data.len() < expected_bytes {
        return Err(format!(
            "Packed f32 data too small: got {} bytes expected {}",
            data.len(),
            expected_bytes
        ));
    }
    if data.len() % std::mem::size_of::<f32>() != 0 {
        return Err(format!(
            "Packed f32 data size not aligned: {} bytes",
            data.len()
        ));
    }
    let samples: &[f32] = bytemuck::cast_slice(data);
    Ok(samples[..expected_samples].to_vec())
}
