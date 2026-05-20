Fast timeline preview in desktop NLEs – architecture & design

This report reviews how professional non‑linear editors (NLEs) like DaVinci Resolve and Adobe Premiere Pro achieve smooth timeline preview and scrubbing, and what practices can be applied when implementing a Rust desktop NLE using egui/eframe and modern Windows APIs. The goal is to understand the architecture of the preview pipeline and identify Rust technologies for decoding, caching and GPU compositing.

## Local Implementation Notes (NLA AI Video Creator)

- Current preview path decodes via `ffmpeg-next` into CPU RGBA. When hardware decode is enabled, FFmpeg uses D3D11VA/DXVA2 and frames are transferred back to CPU (`xfer`) before CPU swscale and WGPU upload.
- GPU compositing (wgpu) handles scale/translate/opacity; rotation is still pending.
- Preview stats now include a detailed `vdec` breakdown (`seek`, `pkt`, `xfer`, `scale`, `copy`) plus `hwdec` percent. Use these to pinpoint the hottest stages.
- A UI toggle in the title bar (“HW Dec”) forces CPU decode for A/B comparisons.
- Future optimization: keep decoded NV12 textures on GPU and composite directly (avoid GPU→CPU→GPU round‑trip).

1 Typical preview pipeline architecture

Professional NLEs implement a multi‑stage pipeline to display the timeline preview efficiently. A simplified architecture is shown below:

Decode queue (worker threads) – Video assets are decoded from disk into uncompressed frames. Resolve uses hardware decoders (NVDEC, Intel Quick Sync) or CPU when necessary. Media Foundation’s Direct3D 11 pipeline describes how a software decoder must receive a pointer to a ID3D11Device through IMFDXGIDeviceManager and allocate Direct3D 11 textures for uncompressed video buffers
learn.microsoft.com
. Optional CPU fallback is negotiated when a suitable hardware configuration is unavailable
learn.microsoft.com
.

Frame cache & render cache – Decoded frames and intermediate render results are cached to avoid re‑computation. Blender’s Video Sequence Editor distinguishes a “raw cache” (cache of frames right after reading from disk) and a “final cache” (cached fully composited images) with prefetch capability
docs.blender.org
docs.blender.org
. DaVinci Resolve has multiple caching levels: output cache (frames before display), node cache (intermediate effects), source cache (on‑the‑fly proxy generation) and sequence cache (automatic caching of composite clips); caching is triggered after a period of inactivity and uses scratch disks
timeinpixels.com
timeinpixels.com
.

Compositing & effects – Uncompressed frames are composited with other video tracks, transitions, filters and titles. Modern NLEs rely heavily on the GPU. Chrome’s hardware‑accelerated compositor explains why GPUs are used: compositing many layers is more efficient on the GPU and avoids expensive read‑backs
chromium.org
. On Windows, Direct2D/Direct3D interop offers two compositing paths: (a) write Direct2D content to a Direct3D surface using IDXGISurface and CreateDxgiSurfaceRenderTarget, which allows adding 2‑D interfaces or overlays to a 3‑D scene; (b) create a shared bitmap from a DXGI surface and let Direct2D render a Direct3D scene as a bitmap
learn.microsoft.com
. Compositing often involves converting YUV/NV12 frames to RGB within shaders and applying transforms, color grading and blending.

Display & scheduling – Frames are delivered to the UI or preview window. Latest‑wins scheduling ensures responsiveness: reactive operators like switchMap cancel stale requests so that only the latest decode/render job emits a frame
developersvoice.com
. This is important for scrubbing—if the user drags the play‑head quickly, intermediate frames are skipped and decoding tasks in the queue are cancelled.

2 Caching strategies used by professional editors
Strategy	Description & examples	Notes
Ring buffer / circular buffer	A fixed‑size buffer used to hold the most recent frames; writes wrap around and overwrite the oldest entries. Useful for streaming preview where memory is limited
en.wikipedia.org
.	Good for continuous playback with limited look‑ahead; eviction is implicit (overwritten).
LRU caches & variants	LRUCache stores recently used frames; operations (read/write/delete) are O(1)
docs.rs
. Enhanced caches like Adaptive Replacement Cache (ARC) track both recency and frequency to avoid eviction of frequently used frames
docs.rs
, but add overhead
docs.rs
. Moka, a Rust crate, implements concurrent caches and uses TinyLFU (LFU admission + LRU eviction) to maintain a high hit ratio and supports time‑to‑live/idle expiration
docs.rs
.	Useful for interactive editing where user may revisit recently viewed frames. Consider concurrency when caching across decoding and compositing threads.
Prefetch windows / preloading	LightAct (a timeline‑based media server) exposes Video Buffer, Video Preload Time, and Always Ready Buffer settings. It keeps a configured number of frames in memory, preloads frames seconds before they are needed, and reserves a portion of the buffer so that sudden jumps still have cached frames
docs.lightact.com
. Blender’s VSE supports prefetching frames after the current frame to improve playback
docs.blender.org
.	Prefetch windows improve performance during forward playback but require heuristics to avoid decoding frames that will never be needed.
Render cache & proxies	Resolve uses an output cache, node cache, source cache, and sequence cache
timeinpixels.com
. Adobe Premiere supports proxies: low‑resolution versions of clips are generated during ingest; editors work with proxies and switch back to full resolution for final export
helpx.adobe.com
. Render caching writes pre‑rendered segments to disk when idle.	Proxy workflows reduce decode cost; render caches avoid repeated effects computation.
Dynamic quality reduction	When the dynamic render buffer empties, some NLEs reduce preview quality by rendering only one field (half vertical resolution) or skipping pixels
tvtechnology.com
. This allows real‑time playback when the CPU/GPU cannot keep up.	Quality adjustments are automatic based on buffer fullness.
3 Hardware decoding on Windows

Hardware decoders offload intensive operations (entropy decoding, inverse transforms, motion compensation) from the CPU. On Windows the following technologies are common:

Technology	Features & integration	NLE notes
DXVA 2.0 / Media Foundation	DirectX Video Acceleration 2.0 lets applications offload decode operations to the GPU; more operations are exposed than DXVA 1.0
learn.microsoft.com
. Media Foundation decoders can decode directly into Direct3D 11 textures. To support Direct3D 11, a decoder obtains a handle to the ID3D11Device through IMFDXGIDeviceManager, allocates uncompressed buffers as textures and decodes frames
learn.microsoft.com
. If a suitable configuration is not found, the decoder must fall back to software
learn.microsoft.com
.	Windows‑first NLEs often rely on Media Foundation for hardware decode; decoded textures can be passed to GPU compositors without round‑tripping through system memory
scalibq.wordpress.com
.
Intel Quick Sync Video (QSV)	Dedicated hardware core for video encoding/decoding integrated into Intel CPUs
en.wikipedia.org
. QSV is accessible via Media Foundation or through proprietary SDKs. Many NLEs support QSV to accelerate H.264/H.265 decode.	
Nvidia NVDEC	The NVDEC engine on Nvidia GPUs offloads video decoding; NVENC deals with encoding
en.wikipedia.org
. NVDEC can decode H.264/H.265/AV1 and provides decoded surfaces as CUDA surfaces or Direct3D textures.	
AMD Video Core Next (VCE/VCN)	AMD GPUs have hardware decode/encode engines (Video Core Next). Access via DirectX VA or platform‑specific SDKs.	
IMFMediaEngine	Newer API recommended by Microsoft for Windows 10+. It delivers uncompressed video frames as D3D11 textures via TransferVideoFrame, allowing the application or Desktop Window Manager (DWM) to composite frames
news.ycombinator.com
.	

Integrating hardware decode into the preview pipeline

Keep YUV/NV12 until compositing – Hardware decoders usually output YUV formats (e.g., NV12). Converting to RGB on the CPU defeats hardware decode; instead, feed NV12 surfaces directly into GPU shaders and convert to RGB in the pixel shader. Scali’s article warns that reading back GPU‑decoded frames to system memory is expensive
scalibq.wordpress.com
 and recommends using NV12 textures; D3D11 can sample NV12 and convert YUV to RGB in a shader
scalibq.wordpress.com
. Keeping YUV until the final composite reduces memory bandwidth and decode cost.

Share the D3D device – The preview pipeline should share the D3D11 device across decoders and renderers via IMFDXGIDeviceManager. This avoids inter‑process copy and allows the GPU to composite textures directly
learn.microsoft.com
.

Fallback to software – Implement CPU decode path for formats not supported by the user’s GPU or when hardware decode fails; ensure caches handle both sources uniformly.

4 GPU compositing approaches on Windows

Direct3D 11 / 12 – The dominant API for GPU compositing on Windows. Applications create D3D11 textures for each video layer; vertex shaders map each texture to a quad and pixel shaders convert YUV to RGB, apply LUTs, keying, color correction and blend using alpha. Layers are sorted by Z‑order; constant buffers store transforms (position, scale, opacity). To composite UI elements with video frames, Direct2D can draw onto DXGI surfaces; Microsoft’s interop guide shows two ways to mix 2‑D and 3‑D: write Direct2D content to a DXGI surface render target for overlays or create a shared bitmap from a DXGI surface so that Direct2D can paint a Direct3D scene
learn.microsoft.com
. D3D12 provides lower‑level control and multi‑queue parallelism but requires more boilerplate.

Vulkan / WebGPU (wgpu) – Modern cross‑platform APIs. wgpu (Rust) is built on WebGPU and runs on Vulkan, Metal, D3D12 and OpenGL; it provides safety and portability
wgpu.rs
. Using wgpu, an NLE can implement a compute/render pipeline; decode textures (NV12) are imported via external memory; shaders perform YUV→RGB conversion and compositing. wgpu is still evolving but integrates well with skia-safe for 2‑D UI drawing.

OpenGL – Many cross‑platform NLEs use OpenGL for compositing because of its wide support. However, on Windows, OpenGL drivers vary in quality; D3D11/12 are preferred for low latency and better integration with hardware decoders. Shotcut’s developer notes mention that GPU processing using OpenGL is problematic on Windows and may be deprecated
forum.shotcut.org
.

Multiplane overlay (MPO) – Some GPUs provide hardware overlay planes for video; the Desktop Window Manager may use them to reduce power consumption. However, modern Windows tends to route everything through the 3‑D pipeline; support for hardware overlays is limited
news.ycombinator.com
.

Native UI composition constraints – The current egui/eframe shell keeps editor UI, dialogs, overlays, and preview output in one native window. This removes the separate-surface overlay problem and makes UI ordering predictable, while still leaving room for a future GPU compositor behind the preview texture.

5 Latest‑wins scheduling & scrubbing

Reactive cancellation – Modern UIs treat the play‑head position as a stream of events. When the user scrubs rapidly, older decode requests become obsolete. RxJS’s switchMap operator demonstrates how to cancel stale requests and ensure that only the latest query emits results
developersvoice.com
. In an NLE, a similar pattern can be used: subscribe to play‑head changes; for each new position, cancel in‑flight decoding/compositing tasks; start decoding frames around the current position; update caches accordingly.

Task prioritization – Use a job queue with priorities: decoding frames at the current cursor gets highest priority; prefetch tasks get lower priority and can be cancelled. Worker threads should check a cancellation token regularly.

Double buffering & dynamic quality – The TV Tech article describes a dynamic render buffer that decompresses video, renders effects, stores uncompressed frames in a buffer, and uses double buffering to display frames
tvtechnology.com
. If heavy effects empty the buffer, the system reduces preview quality (skip fields/pixels) to maintain responsiveness
tvtechnology.com
.

6 Rust ecosystem options (Windows‑first)
Area	Crates & approaches	Pros	Cons
Video decoding	ffmpeg‑next / ffmpeg‑sys: safe wrappers around FFmpeg; support many codecs; widely used. video‑rs is a higher‑level wrapper that simplifies encoding/decoding and is built on FFmpeg
oddity.ai
. gstreamer‑rs: Rust bindings for GStreamer; good plugin ecosystem; integrates with hardware decode via VA‑API/DXVA (through plugins). windows crate: allows direct use of Media Foundation COM APIs; no high‑level wrapper; complex but permits hardware decode and device sharing. mmf crate: minimal Media Foundation wrapper.	FFmpeg/GStreamer provide broad codec support; video‑rs offers simple API; GStreamer pipelines can include hardware decoders. Media Foundation provides native Windows integration and D3D11 textures.	FFmpeg/GStreamer are LGPL/GPL (see licensing). Hardware decode integration in GStreamer on Windows is immature. Using Media Foundation requires unsafe COM and is not fully wrapped in Rust. Limited community support for mmf/winrt video.
GPU compositing	wgpu (WebGPU) – safe, cross‑platform graphics API; works on Vulkan, Metal, D3D12 and WebGL
wgpu.rs
. glow or glium – OpenGL wrappers; simpler but rely on OpenGL drivers. skia‑safe / tiny‑skia – 2‑D GPU rasterization via Skia; can render UI elements and draw into GPU textures; can integrate with wgpu by creating surfaces.	wgpu provides unified API and integrates with future WebGPU in browsers; safe RAII; good for both compute and graphics. Skia offers high‑quality 2‑D rendering and text.	wgpu is still evolving; some features (shared textures, YUV sampling) may require feature flags or OS‑specific extensions. OpenGL wrappers can suffer driver issues on Windows. Skia uses its own GPU backend; bridging wgpu textures to Skia may require copying.
Caching utilities	caches crate provides LRUCache, SegmentedCache, AdaptiveCache and TwoQueueCache; operations are O(1) and ARC/2Q avoid eviction of frequently used entries at the cost of extra tracking
docs.rs
. moka crate offers thread‑safe, concurrent caches with TinyLFU admission and LRU eviction; caches can be bounded by entry count or weighted size and support time‑to‑live/idle expiration
docs.rs
. mini‑moka provides non‑concurrent versions.	caches is simple and flexible; custom eviction callbacks; good for single‑threaded caches (e.g., per‑track frame caches). moka scales across threads and integrates with async tasks; TinyLFU improves hit ratio for varied access patterns.	caches and mini‑moka require manual locking in multi‑threaded use; caches uses boxes/pointers that add overhead; moka’s concurrency overhead may be overkill for small caches. IBM holds a patent on ARC; while open‑source use is typical, commercial use should evaluate patent risk
docs.rs
.
UI integration (egui/eframe)	egui renders the desktop interface directly through a native eframe window. Preview frames can be uploaded as textures and drawn in the same UI pass as panels, dialogs, overlays, and the timeline.	Immediate-mode rendering keeps UI state explicit and makes automation-friendly model/controller extraction straightforward.	Advanced compositor work still needs careful texture ownership, frame pacing, and cache invalidation to keep scrubbing smooth.
7 Licensing considerations

FFmpeg – The FFmpeg project is licensed under the LGPL v2.1 or later, but some optional components are covered by the GPL v2
ffmpeg.org
. When building FFmpeg for use in a proprietary desktop app, you must disable GPL components (--disable-gpl) and use dynamic linking to comply with the LGPL; you must provide FFmpeg’s source code and acknowledge its use
ffmpeg.org
. Including GPL components (e.g., libx264) will subject your entire application to the GPL.

GStreamer – GStreamer’s core and official plugins are licensed under the LGPL. The community requires that code included in the core or official modules be LGPL, even when linking to external GPL libraries. GPL plugins are placed in separate packages to avoid accidental license violations
gstreamer.freedesktop.org
.

Hardware decode SDKs – Intel QSV, NVDEC and AMD VCE/VCN SDKs may have their own license terms. For Windows, using Media Foundation APIs does not impose additional open‑source obligations.

Rust crates – video‑rs, ffmpeg‑next and moka are dual‑licensed under MIT/Apache‑2.0 (per docs.rs). caches is also MIT/Apache‑2.0 but note that ARC/2Q are patented
docs.rs
.

8 Trade‑offs: decode to RGBA vs keep YUV
Approach	Pros	Cons
Decode to RGBA (CPU)	Simplifies compositing because all images share the same color space; CPU decoders like FFmpeg directly output RGBA; easy to draw with Skia/wgpu; may avoid YUV→RGB conversion in shaders.	For high‑resolution video, copying YUV from GPU to system memory and converting to RGB can negate the benefits of hardware decoding. Scali notes that retrieving pixel data from a hardware decoder often requires copying the frame from GPU to system memory, causing performance loss
scalibq.wordpress.com
.
Keep YUV (NV12) until compositing	Hardware decoders output NV12; using NV12 textures avoids copies and leverages GPU bandwidth. YUV→RGB conversion can be performed in the pixel shader; Direct3D supports sampling NV12 textures and converting to RGB
scalibq.wordpress.com
. This also allows color space corrections in the shader.	Requires writing custom shaders and ensuring color space conversion is accurate. Not all GPU APIs support sampling NV12 directly (OpenGL requires extensions).
9 Recommendations for a Rust + egui desktop NLE

Decoding layer – Use FFmpeg via video‑rs or ffmpeg‑next for broad codec support. For Windows‑first hardware acceleration, consider using the windows crate to access Media Foundation and decode directly into D3D11 textures (NV12). Keep YUV surfaces and avoid copying them to CPU memory. Provide a fallback CPU decode path for unsupported codecs.

Caching – Implement a two‑level cache: an in‑memory ring/LRU cache for recently decoded frames and a disk‑backed render cache for segments with heavy effects. Use the caches or moka crate; for multi‑threaded decoders, moka provides concurrent caches with TinyLFU admission and LRU eviction
docs.rs
. Prefetch a configurable window of frames ahead of the play‑head and keep a fraction of frames loaded at all times (similar to LightAct’s Always Ready Buffer
docs.lightact.com
). Allow proxy generation for high‑resolution footage.

Compositing & rendering – Continue using the egui texture path for integrated UI/preview composition, then evolve the preview backend toward a GPU compositor when the model and timeline interactions are stable. A future compositor can sample NV12 textures, convert to linear RGB in shaders, apply transforms, and upload the final frame for egui to present in the preview panel.

Scheduling & responsiveness – Represent the play‑head position as a reactive signal. Use a latest‑wins strategy (similar to RxJS switchMap) to cancel outdated decode/composite jobs
developersvoice.com
. Give highest priority to frames near the current scrub position; treat prefetch tasks as cancellable. Implement double buffering to avoid tearing and adjust preview quality when the buffer empties
tvtechnology.com
.

Licensing – Build FFmpeg with LGPL‑only codecs or choose GStreamer for an LGPL pipeline. Document the use of FFmpeg in your application and provide access to its source as required
ffmpeg.org
. Avoid GPL‑licensed modules unless you are prepared to release your app under the GPL. When using ARC caches, be aware of IBM’s patents
docs.rs
; TwoQueueCache offers similar benefits without the patent.

Further reading – Study the Direct3D 11 video decoding guide for details on device sharing and buffer allocation
learn.microsoft.com
. Explore GPU texture sampling examples for YUV/NV12, Direct3D device sharing, and skia‑safe integration if richer vector/text compositing becomes necessary.
.

Conclusion

Smooth timeline preview in an NLE is achieved by combining hardware‑accelerated decoding, intelligent caching and GPU‑based compositing. Professional editors like Resolve and Premiere employ decode queues, multi‑layer caches (raw, final, node, proxy), GPU shaders and reactive scheduling to keep the preview responsive. For this Rust desktop NLE, that translates into using FFmpeg or Media Foundation for decoding, egui/eframe for the native editor shell, a future GPU compositor for heavy preview work, moka or caches for frame data, and UI code that handles asynchronous jobs and cancellations. With careful attention to licensing and platform constraints, it is possible to build a Windows‑first NLE that offers professional‑grade preview performance.

In summary, professional non‑linear editors employ a multi-stage pipeline of hardware-accelerated decoding, intelligent caching, GPU-based compositing and reactive scheduling to ensure smooth timeline previews. Hardware decoders (DXVA2, NVDEC, QuickSync) feed YUV/NV12 frames directly into GPU textures; multi-level caches (raw, final, node, proxy) and prefetch buffers avoid redundant decoding while render caches handle intensive effects
docs.blender.org
timeinpixels.com
. Reactive patterns like switchMap cancel stale decode jobs during scrubbing
developersvoice.com
. On Windows, decoded textures are shared via Direct3D 11, and two approaches exist to intermix 2-D and 3-D: writing Direct2D content to a DXGI surface or using a shared bitmap
learn.microsoft.com
.

Rust developers targeting Windows-first NLEs can build the decoding layer using FFmpeg (video-rs/ffmpeg-next) or Media Foundation bindings, and should keep data in YUV until GPU compositing to avoid unnecessary copies
scalibq.wordpress.com
. Wgpu offers a safe, cross-platform API for GPU work
wgpu.rs
, while caches or moka provide efficient LRU or TinyLFU caches for frame data
docs.rs
. The native egui shell keeps preview and UI overlays in the same window, so future compositor work should focus on texture lifetime, frame pacing, and upload efficiency rather than cross-surface layering. Licensing considerations demand using LGPL-only FFmpeg components and disclosing source, while avoiding GPL encoders and being mindful of patented ARC caching strategies
ffmpeg.org
docs.rs
.
