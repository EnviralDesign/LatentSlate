use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::Instant;

use ffmpeg_next as ffmpeg;
use image::RgbaImage;

use crate::core::ffmpeg_runtime::init_ffmpeg;

const AV_TIME_BASE: i64 = 1_000_000;
const MAX_DECODERS: usize = 8;
const MAX_SEQUENTIAL_JUMP_SECONDS: f64 = 2.0;
const MAX_DECODE_WORKERS: usize = 4;

#[derive(Clone, Copy, Debug, Default)]
pub struct DecodeTimings {
    pub queue_ms: f64,
    pub seek_ms: f64,
    pub packet_ms: f64,
    pub transfer_ms: f64,
    pub scale_ms: f64,
    pub copy_ms: f64,
}

impl DecodeTimings {
    pub fn total_ms(&self) -> f64 {
        self.queue_ms
            + self.seek_ms
            + self.packet_ms
            + self.transfer_ms
            + self.scale_ms
            + self.copy_ms
    }
}

#[cfg(target_os = "windows")]
const HW_DEVICE_CANDIDATES: &[ffmpeg::ffi::AVHWDeviceType] = &[
    ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
    ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_DXVA2,
];

#[cfg(not(target_os = "windows"))]
const HW_DEVICE_CANDIDATES: &[ffmpeg::ffi::AVHWDeviceType] = &[];

struct DecodeRequest {
    path: PathBuf,
    time_seconds: f64,
    mode: DecodeMode,
    lane_id: u64,
    allow_hw: bool,
    epoch: u64,
    enqueued_at: Instant,
    respond_to: mpsc::Sender<DecodeResponse>,
}

#[derive(Clone, Copy, Debug)]
pub enum DecodeMode {
    Seek,
    Sequential,
}

pub struct DecodeResponse {
    pub image: Option<RgbaImage>,
    pub timings: DecodeTimings,
    pub used_hw: bool,
    pub source_width: u32,
    pub source_height: u32,
}

/// A dedicated worker pool for in-process video decoding with FFmpeg.
#[derive(Clone)]
pub struct VideoDecodeWorker {
    senders: Vec<mpsc::Sender<DecodeRequest>>,
    epoch: Arc<AtomicU64>,
}

impl VideoDecodeWorker {
    /// Create workers that decode frames scaled to the preview bounds.
    pub fn new(max_width: u32, max_height: u32) -> Self {
        let worker_count = std::thread::available_parallelism()
            .map(|value| value.get().min(MAX_DECODE_WORKERS).max(1))
            .unwrap_or(1);

        let mut senders = Vec::with_capacity(worker_count);
        let epoch = Arc::new(AtomicU64::new(0));
        for _ in 0..worker_count {
            let (sender, receiver) = mpsc::channel::<DecodeRequest>();
            senders.push(sender);
            let worker_epoch = Arc::clone(&epoch);

            thread::spawn(move || {
                let _ = init_ffmpeg();
                let mut decoders: HashMap<DecoderKey, DecoderEntry> = HashMap::new();
                let mut access_counter: u64 = 0;

                for request in receiver {
                    let DecodeRequest {
                        path,
                        time_seconds,
                        mode,
                        lane_id,
                        allow_hw,
                        epoch,
                        enqueued_at,
                        respond_to,
                    } = request;

                    if epoch != worker_epoch.load(Ordering::Relaxed) {
                        let mut timings = DecodeTimings::default();
                        timings.queue_ms = elapsed_ms(enqueued_at);
                        let _ = respond_to.send(DecodeResponse {
                            image: None,
                            timings,
                            used_hw: false,
                            source_width: 0,
                            source_height: 0,
                        });
                        continue;
                    }

                    let key = DecoderKey {
                        path: path.clone(),
                        lane_id,
                        allow_hw,
                    };

                    let mut outcome = match decoders.entry(key) {
                        Entry::Occupied(mut entry) => {
                            access_counter = access_counter.wrapping_add(1);
                            entry.get_mut().last_used = access_counter;
                            entry
                                .get_mut()
                                .decoder
                                .decode_frame_at_time(time_seconds, mode)
                        }
                        Entry::Vacant(entry) => {
                            match VideoDecoder::open(&path, max_width, max_height, allow_hw) {
                                Ok(mut decoder) => {
                                    access_counter = access_counter.wrapping_add(1);
                                    let outcome = decoder.decode_frame_at_time(time_seconds, mode);
                                    entry.insert(DecoderEntry {
                                        decoder,
                                        last_used: access_counter,
                                    });
                                    outcome
                                }
                                Err(_) => DecodeOutcome::none(),
                            }
                        }
                    };
                    outcome.timings.queue_ms = elapsed_ms(enqueued_at);

                    if decoders.len() > MAX_DECODERS {
                        evict_least_used(&mut decoders);
                    }

                    let _ = respond_to.send(DecodeResponse {
                        image: outcome.image,
                        timings: outcome.timings,
                        used_hw: outcome.used_hw,
                        source_width: outcome.source_width,
                        source_height: outcome.source_height,
                    });
                }
            });
        }

        Self { senders, epoch }
    }

    pub fn current_epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }

    pub fn invalidate_pending(&self) {
        self.epoch.fetch_add(1, Ordering::Relaxed);
    }

    /// Decode a single frame at the requested timestamp (seconds).
    #[allow(dead_code)]
    // Synchronous decode is retained for diagnostic and non-UI callers; the egui
    // preview path currently uses async decode to keep painting responsive.
    pub fn decode(
        &self,
        path: &Path,
        time_seconds: f64,
        lane_id: u64,
        allow_hw: bool,
    ) -> Option<DecodeResponse> {
        self.decode_with_mode(path, time_seconds, DecodeMode::Seek, lane_id, allow_hw)
    }

    /// Decode a single frame using sequential decode when possible.
    #[allow(dead_code)]
    // Synchronous sequential decode is retained for future playback-oriented
    // preview scheduling.
    pub fn decode_sequential(
        &self,
        path: &Path,
        time_seconds: f64,
        lane_id: u64,
        allow_hw: bool,
    ) -> Option<DecodeResponse> {
        self.decode_with_mode(
            path,
            time_seconds,
            DecodeMode::Sequential,
            lane_id,
            allow_hw,
        )
    }

    pub fn decode_async(
        &self,
        path: &Path,
        time_seconds: f64,
        mode: DecodeMode,
        lane_id: u64,
        allow_hw: bool,
    ) -> Option<mpsc::Receiver<DecodeResponse>> {
        self.decode_async_at_epoch(
            path,
            time_seconds,
            mode,
            lane_id,
            allow_hw,
            self.current_epoch(),
        )
    }

    pub fn decode_async_at_epoch(
        &self,
        path: &Path,
        time_seconds: f64,
        mode: DecodeMode,
        lane_id: u64,
        allow_hw: bool,
        epoch: u64,
    ) -> Option<mpsc::Receiver<DecodeResponse>> {
        let sender = self.select_sender(lane_id, epoch)?;
        let (respond_to, response) = mpsc::channel();
        let request = DecodeRequest {
            path: path.to_path_buf(),
            time_seconds,
            mode,
            lane_id,
            allow_hw,
            epoch,
            enqueued_at: Instant::now(),
            respond_to,
        };

        sender.send(request).ok()?;
        Some(response)
    }

    #[allow(dead_code)]
    // Shared by the preserved synchronous decode helpers above.
    fn decode_with_mode(
        &self,
        path: &Path,
        time_seconds: f64,
        mode: DecodeMode,
        lane_id: u64,
        allow_hw: bool,
    ) -> Option<DecodeResponse> {
        let response = self.decode_async(path, time_seconds, mode, lane_id, allow_hw)?;
        response.recv().ok()
    }

    fn select_sender(&self, lane_id: u64, epoch: u64) -> Option<&mpsc::Sender<DecodeRequest>> {
        if self.senders.is_empty() {
            return None;
        }
        let mixed = lane_id ^ epoch.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let index = (mixed as usize) % self.senders.len();
        self.senders.get(index)
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct DecoderKey {
    path: PathBuf,
    lane_id: u64,
    allow_hw: bool,
}

struct DecoderEntry {
    decoder: VideoDecoder,
    last_used: u64,
}

fn evict_least_used(decoders: &mut HashMap<DecoderKey, DecoderEntry>) {
    while decoders.len() > MAX_DECODERS {
        let mut oldest_key: Option<DecoderKey> = None;
        let mut oldest_stamp: u64 = u64::MAX;
        for (key, entry) in decoders.iter() {
            if entry.last_used < oldest_stamp {
                oldest_stamp = entry.last_used;
                oldest_key = Some(key.clone());
            }
        }
        if let Some(key) = oldest_key {
            decoders.remove(&key);
        } else {
            break;
        }
    }
}

struct DecodeOutcome {
    image: Option<RgbaImage>,
    used_hw: bool,
    timings: DecodeTimings,
    source_width: u32,
    source_height: u32,
}

impl DecodeOutcome {
    fn none() -> Self {
        Self {
            image: None,
            used_hw: false,
            timings: DecodeTimings::default(),
            source_width: 0,
            source_height: 0,
        }
    }
}

struct HwDecodeState {
    hw_pix_fmt: ffmpeg::ffi::AVPixelFormat,
    device_ctx: *mut ffmpeg::ffi::AVBufferRef,
}

impl Drop for HwDecodeState {
    fn drop(&mut self) {
        unsafe {
            if !self.device_ctx.is_null() {
                ffmpeg::ffi::av_buffer_unref(&mut self.device_ctx);
            }
        }
    }
}

unsafe extern "C" fn get_hw_format(
    ctx: *mut ffmpeg::ffi::AVCodecContext,
    pix_fmts: *const ffmpeg::ffi::AVPixelFormat,
) -> ffmpeg::ffi::AVPixelFormat {
    if ctx.is_null() || pix_fmts.is_null() {
        return ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_NONE;
    }
    let state = (*ctx).opaque as *mut HwDecodeState;
    if state.is_null() {
        return ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_NONE;
    }
    let desired = (*state).hw_pix_fmt;
    let mut p = pix_fmts;
    while *p != ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_NONE {
        if *p == desired {
            return desired;
        }
        p = p.add(1);
    }
    ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_NONE
}

fn setup_hwaccel(
    context: &mut ffmpeg::codec::context::Context,
    codec: &ffmpeg::Codec,
) -> Option<Box<HwDecodeState>> {
    if HW_DEVICE_CANDIDATES.is_empty() {
        return None;
    }
    unsafe {
        for &device_type in HW_DEVICE_CANDIDATES {
            let Some(hw_pix_fmt) = find_hw_pix_fmt(codec, device_type) else {
                continue;
            };

            let mut device_ctx: *mut ffmpeg::ffi::AVBufferRef = std::ptr::null_mut();
            if ffmpeg::ffi::av_hwdevice_ctx_create(
                &mut device_ctx,
                device_type,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            ) < 0
            {
                continue;
            }

            if device_ctx.is_null() {
                continue;
            }

            let mut state = Box::new(HwDecodeState {
                hw_pix_fmt,
                device_ctx,
            });

            let ctx_ptr = context.as_mut_ptr();
            (*ctx_ptr).opaque = state.as_mut() as *mut _ as *mut _;
            (*ctx_ptr).get_format = Some(get_hw_format);
            (*ctx_ptr).hw_device_ctx = ffmpeg::ffi::av_buffer_ref(device_ctx);

            return Some(state);
        }
    }
    None
}

unsafe fn find_hw_pix_fmt(
    codec: &ffmpeg::Codec,
    device_type: ffmpeg::ffi::AVHWDeviceType,
) -> Option<ffmpeg::ffi::AVPixelFormat> {
    let mut index = 0;
    loop {
        let config = ffmpeg::ffi::avcodec_get_hw_config(codec.as_ptr(), index);
        if config.is_null() {
            break;
        }
        let config = &*config;
        if (config.methods & ffmpeg::ffi::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32) != 0
            && config.device_type == device_type
        {
            return Some(config.pix_fmt);
        }
        index += 1;
    }
    None
}

struct VideoDecoder {
    input: ffmpeg::format::context::Input,
    stream_index: usize,
    decoder: ffmpeg::decoder::Video,
    hw_state: Option<Box<HwDecodeState>>,
    scaler: Option<ffmpeg::software::scaling::Context>,
    scaler_src: Option<(ffmpeg::util::format::Pixel, u32, u32)>,
    target_width: u32,
    target_height: u32,
    source_width: u32,
    source_height: u32,
    time_base: ffmpeg::Rational,
    last_pts: Option<i64>,
    last_time_seconds: Option<f64>,
}

impl VideoDecoder {
    fn open(
        path: &Path,
        max_width: u32,
        max_height: u32,
        allow_hw: bool,
    ) -> Result<Self, ffmpeg::Error> {
        let input = ffmpeg::format::input(path)?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let stream_index = stream.index();
        let time_base = stream.time_base();

        let mut context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        let hw_state = if allow_hw {
            if let Some(codec) = context
                .codec()
                .or_else(|| ffmpeg::codec::decoder::find(context.id()))
            {
                setup_hwaccel(&mut context, &codec)
            } else {
                None
            }
        } else {
            None
        };

        let decoder = context.decoder().video()?;

        let src_width = decoder.width().max(1);
        let src_height = decoder.height().max(1);
        let (target_width, target_height) =
            fit_dimensions(src_width, src_height, max_width, max_height);

        Ok(Self {
            input,
            stream_index,
            decoder,
            hw_state,
            scaler: None,
            scaler_src: None,
            target_width,
            target_height,
            source_width: src_width,
            source_height: src_height,
            time_base,
            last_pts: None,
            last_time_seconds: None,
        })
    }

    fn decode_frame_at_time(&mut self, time_seconds: f64, mode: DecodeMode) -> DecodeOutcome {
        match mode {
            DecodeMode::Seek => self.decode_with_seek(time_seconds),
            DecodeMode::Sequential => self.decode_sequential(time_seconds),
        }
    }

    fn decode_sequential(&mut self, time_seconds: f64) -> DecodeOutcome {
        let mut timings = DecodeTimings::default();
        let last_time = self.last_time_seconds.unwrap_or(f64::NEG_INFINITY);
        let delta = time_seconds - last_time;
        if self.last_pts.is_none() || delta < 0.0 || delta > MAX_SEQUENTIAL_JUMP_SECONDS {
            return self.decode_with_seek(time_seconds);
        }

        let target_pts = seconds_to_pts(time_seconds.max(0.0), self.time_base);
        if let Some((image, pts, used_hw)) = self.decode_forward(target_pts, &mut timings) {
            self.last_pts = Some(pts);
            self.last_time_seconds = Some(pts_to_seconds(pts, self.time_base));
            return DecodeOutcome {
                image: Some(image),
                used_hw,
                timings,
                source_width: self.source_width,
                source_height: self.source_height,
            };
        }

        self.decode_with_seek(time_seconds)
    }

    fn decode_with_seek(&mut self, time_seconds: f64) -> DecodeOutcome {
        let mut timings = DecodeTimings::default();
        let seek_start = Instant::now();
        let target_ts = (time_seconds.max(0.0) * AV_TIME_BASE as f64).round() as i64;
        if self.input.seek(target_ts, ..).is_err() {
            timings.seek_ms = elapsed_ms(seek_start);
            return DecodeOutcome {
                image: None,
                used_hw: false,
                timings,
                source_width: self.source_width,
                source_height: self.source_height,
            };
        }
        self.decoder.flush();
        timings.seek_ms = elapsed_ms(seek_start);

        let target_pts = seconds_to_pts(time_seconds.max(0.0), self.time_base);
        if let Some((image, pts, used_hw)) = self.decode_forward(target_pts, &mut timings) {
            self.last_pts = Some(pts);
            self.last_time_seconds = Some(pts_to_seconds(pts, self.time_base));
            return DecodeOutcome {
                image: Some(image),
                used_hw,
                timings,
                source_width: self.source_width,
                source_height: self.source_height,
            };
        }

        DecodeOutcome {
            image: None,
            used_hw: false,
            timings,
            source_width: self.source_width,
            source_height: self.source_height,
        }
    }

    fn decode_forward(
        &mut self,
        target_pts: i64,
        timings: &mut DecodeTimings,
    ) -> Option<(RgbaImage, i64, bool)> {
        let mut decoded = ffmpeg::util::frame::Video::empty();
        let mut sw_frame = ffmpeg::util::frame::Video::empty();
        let mut rgba_frame = ffmpeg::util::frame::Video::empty();
        let stream_index = self.stream_index;
        let forward_start = Instant::now();

        let VideoDecoder {
            input,
            decoder,
            hw_state,
            scaler,
            scaler_src,
            target_width,
            target_height,
            ..
        } = self;
        let target_width = *target_width;
        let target_height = *target_height;

        let mut result = receive_until_target(
            decoder,
            hw_state.as_deref(),
            scaler,
            scaler_src,
            target_width,
            target_height,
            target_pts,
            &mut decoded,
            &mut sw_frame,
            &mut rgba_frame,
            timings,
        );

        if result.is_none() {
            for (stream, packet) in input.packets() {
                if stream.index() != stream_index {
                    continue;
                }
                if decoder.send_packet(&packet).is_err() {
                    continue;
                }
                result = receive_until_target(
                    decoder,
                    hw_state.as_deref(),
                    scaler,
                    scaler_src,
                    target_width,
                    target_height,
                    target_pts,
                    &mut decoded,
                    &mut sw_frame,
                    &mut rgba_frame,
                    timings,
                );
                if result.is_some() {
                    break;
                }
            }
        }

        let elapsed = elapsed_ms(forward_start);
        let accounted = timings.transfer_ms + timings.scale_ms + timings.copy_ms;
        timings.packet_ms += (elapsed - accounted).max(0.0);

        result
    }
}

fn receive_until_target(
    decoder: &mut ffmpeg::decoder::Video,
    hw_state: Option<&HwDecodeState>,
    scaler: &mut Option<ffmpeg::software::scaling::Context>,
    scaler_src: &mut Option<(ffmpeg::util::format::Pixel, u32, u32)>,
    target_width: u32,
    target_height: u32,
    target_pts: i64,
    decoded: &mut ffmpeg::util::frame::Video,
    sw_frame: &mut ffmpeg::util::frame::Video,
    rgba_frame: &mut ffmpeg::util::frame::Video,
    timings: &mut DecodeTimings,
) -> Option<(RgbaImage, i64, bool)> {
    while decoder.receive_frame(decoded).is_ok() {
        let frame_pts = decoded.timestamp().or(decoded.pts()).unwrap_or(0);
        if frame_pts < target_pts {
            continue;
        }

        let (source, used_hw) = if let Some(hw_state) = hw_state {
            let hw_pix = ffmpeg::util::format::Pixel::from(hw_state.hw_pix_fmt);
            if decoded.format() == hw_pix {
                unsafe {
                    let transfer_start = Instant::now();
                    ffmpeg::ffi::av_frame_unref(sw_frame.as_mut_ptr());
                    if ffmpeg::ffi::av_hwframe_transfer_data(
                        sw_frame.as_mut_ptr(),
                        decoded.as_ptr() as *mut _,
                        0,
                    ) < 0
                    {
                        timings.transfer_ms += elapsed_ms(transfer_start);
                        continue;
                    }
                    timings.transfer_ms += elapsed_ms(transfer_start);
                }
                (sw_frame, true)
            } else {
                (decoded, false)
            }
        } else {
            (decoded, false)
        };

        let image = scale_to_rgba(
            source,
            rgba_frame,
            scaler,
            scaler_src,
            target_width,
            target_height,
            timings,
        )?;
        return Some((image, frame_pts, used_hw));
    }

    None
}

fn scale_to_rgba(
    source: &ffmpeg::util::frame::Video,
    rgba_frame: &mut ffmpeg::util::frame::Video,
    scaler: &mut Option<ffmpeg::software::scaling::Context>,
    scaler_src: &mut Option<(ffmpeg::util::format::Pixel, u32, u32)>,
    target_width: u32,
    target_height: u32,
    timings: &mut DecodeTimings,
) -> Option<RgbaImage> {
    let src_width = source.width().max(1);
    let src_height = source.height().max(1);
    let src_format = source.format();

    let needs_new = match scaler_src {
        Some((fmt, width, height)) => {
            *fmt != src_format || *width != src_width || *height != src_height
        }
        None => true,
    };

    if needs_new {
        let new_scaler = ffmpeg::software::scaling::Context::get(
            src_format,
            src_width,
            src_height,
            ffmpeg::util::format::Pixel::RGBA,
            target_width,
            target_height,
            ffmpeg::software::scaling::Flags::BILINEAR,
        )
        .ok()?;
        *scaler = Some(new_scaler);
        *scaler_src = Some((src_format, src_width, src_height));
    }

    let scaler = scaler.as_mut()?;
    let scale_start = Instant::now();
    if scaler.run(source, rgba_frame).is_err() {
        return None;
    }
    timings.scale_ms += elapsed_ms(scale_start);

    let copy_start = Instant::now();
    let image = frame_to_rgba(rgba_frame)?;
    timings.copy_ms += elapsed_ms(copy_start);
    Some(image)
}

fn fit_dimensions(src_width: u32, src_height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let max_width = max_width.max(1);
    let max_height = max_height.max(1);

    if src_width <= max_width && src_height <= max_height {
        return (src_width, src_height);
    }

    let scale_w = max_width as f64 / src_width as f64;
    let scale_h = max_height as f64 / src_height as f64;
    let scale = scale_w.min(scale_h).max(0.01);

    let target_width = (src_width as f64 * scale).round().max(1.0) as u32;
    let target_height = (src_height as f64 * scale).round().max(1.0) as u32;

    (target_width, target_height)
}

fn seconds_to_pts(time_seconds: f64, time_base: ffmpeg::Rational) -> i64 {
    let numerator = time_base.numerator() as f64;
    let denominator = time_base.denominator() as f64;
    if numerator <= 0.0 || denominator <= 0.0 {
        return 0;
    }

    (time_seconds * denominator / numerator).round() as i64
}

fn pts_to_seconds(pts: i64, time_base: ffmpeg::Rational) -> f64 {
    let numerator = time_base.numerator() as f64;
    let denominator = time_base.denominator() as f64;
    if numerator <= 0.0 || denominator <= 0.0 {
        return 0.0;
    }

    pts as f64 * numerator / denominator
}

fn frame_to_rgba(frame: &ffmpeg::util::frame::Video) -> Option<RgbaImage> {
    let width = frame.width() as usize;
    let height = frame.height() as usize;
    if width == 0 || height == 0 {
        return None;
    }

    let stride = frame.stride(0);
    let row_bytes = width * 4;
    if stride < row_bytes {
        return None;
    }

    let data = frame.data(0);
    let mut buffer = vec![0_u8; row_bytes * height];

    for y in 0..height {
        let src_offset = y * stride;
        let dst_offset = y * row_bytes;
        let src_slice = data.get(src_offset..src_offset + row_bytes)?;
        buffer[dst_offset..dst_offset + row_bytes].copy_from_slice(src_slice);
    }

    RgbaImage::from_vec(width as u32, height as u32, buffer)
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
