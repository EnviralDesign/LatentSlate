# Preview Stats Reference

This doc explains the preview performance overlay shown in the Preview panel.

Each value reflects the most recent preview render request.

- `async`: Whether a preview render worker is idle or currently rendering, plus current busy time when active.
- `worker`: Time spent on the background render worker for the accepted preview request (ms).
- `delivery`: Time from scheduling the render request until the UI accepted and uploaded the result (ms).
- `total`: Total wall-clock time for the render (ms).
- `scan`: Time spent scanning tracks, resolving assets, and cache checks (excludes decode and still load) (ms).
- `vdec`: Time spent decoding video frames (ms).
- `seek`: Time spent seeking the source stream (ms).
- `pkt`: Time spent demuxing/decoding packets up to the target frame (ms).
- `xfer`: Time spent transferring GPU frames back to CPU (ms).
- `scale`: Time spent scaling decoded frames to preview size (ms).
- `copy`: Time spent copying the RGBA frame into the preview buffer (ms).
- `hwdec`: Percentage of decoded video frames that used hardware acceleration (per render).
- `still`: Time spent loading still images (ms).
- `comp`: Time spent CPU-compositing layers into an RGBA canvas (ms). This should be `0` for the normal egui interactive preview path because layer placement now happens in the egui paint pass; export still uses the RGBA compositor path.
- `upload`: Time spent preparing or uploading preview layer textures for display (ms). Cached layer textures should make this near-zero after the first render of a frame.
- `gpu`: Legacy WGPU upload timing from the Dioxus/native-overlay path. The egui shell no longer uses the split overlay surface.
- `hit`: Cache hit percentage for frame lookups during this render.
- `layers`: Number of visual layers composited for this render.
- `stale`: Number of completed async render requests discarded because the playhead/project had already moved on.

Note: `total` is the only wall-clock timer. The other fields are per-stage durations. `vdec` is the sum of `seek`, `pkt`, `xfer`, `scale`, and `copy`. `hwdec` shows `--` when no video decode ran for the render.

When Preview Stats is enabled, timeline clips also draw a narrow cache bucket strip along the clip bottom. Bright green/cyan segments are cached frame buckets; muted brown segments are cold buckets. This mirrors the old Dioxus debug visualization at a lower visual weight so the timeline remains readable.

## Automation Diagnostics

When the app is launched with `--automation`, the loopback API exposes:

- `get_performance_diagnostics`: current playhead, async render state, latest stats, frame-cache occupancy, recent render samples, and aggregate recent timing summaries.
- `scrub_timeline_profile`: repeatable egui-side scrub over a time range. It calls the same seek path as ruler/playhead dragging, then uses the direct renderer for each requested sample so stage timings are deterministic. Use repeated `seek` commands plus `get_performance_diagnostics` when measuring non-blocking UI responsiveness.

The interactive egui preview uses the preserved layer-stack renderer rather than the old encoded-frame store. Cached frame hits now avoid both video decode and CPU RGBA compositing; the remaining warm-path work should mainly be lightweight cache lookup and egui texture reuse.
