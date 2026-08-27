# Architecture

This is the concise architecture reference for the current app. It describes what
exists now, not an aspirational design.

## System Shape

```text
egui/eframe desktop shell
        |
        v
Editor model/controller (`src/editor.rs`)
        |
        +--> Project/state model (`src/state/`)
        +--> Preview/export/audio/media core (`src/core/`)
        +--> Shared provider execution (`src/providers/`)
        |       +--> LatentSlate Engine over HTTP
        |       +--> ComfyUI over HTTP/WebSocket
        |       +--> cloud APIs
        +--> Loopback automation (`src/core/automation.rs`)
```

The UI should call shared editor/core operations instead of duplicating behavior
in widget code. The automation harness also routes through those paths where
practical.

## Project Model

A project is a folder. The app stores imported and generated media inside that
folder so projects can be moved or zipped more predictably.

```text
my-project/
├── project.json
├── audio/
├── images/
├── video/
├── generated/
│   ├── image/
│   ├── video/
│   └── audio/
└── exports/
```

Important rules:

- Imported media is copied into the project folder.
- Assets store project-relative paths where possible.
- Generated assets have their own UUID-keyed folder with `config.json` plus versioned output files.
- Deleting an asset removes its project-local owned media, generated folder, and asset-specific caches when no remaining asset references the same project-relative path; external or unsafe paths are only removed from the project model.
- Writable app-managed state lives under `LatentSlateData/` next to the running executable unless `LATENTSLATE_HOME` is set.
- Default projects are written to `LatentSlateData/projects/`.
- User-authored provider entries are written to `LatentSlateData/providers/`.
- LatentSlate Engine backends may be stored in `LatentSlateData/engine.json` as a `connections` list (a legacy singleton object still loads). Each backend caches its last successful catalog in `LatentSlateData/engine_catalog.json` or `LatentSlateData/engine_catalogs/<id>.json`.
- Provider entries contain inline ComfyUI manifest bindings and inline cloud provider API keys.
- Engine tools are generated in memory from a live or cached Engine catalog and are not written as editable provider JSON files.
- Project settings can optionally scope providers with a project-level allowlist; provider pickers, generation, Asset Lab provider selection, and default Agent API provider metadata honor that scope.
- `LatentSlateData/workflows/` is created as an optional local home for ComfyUI API workflow JSON files.
- App scratch files are written under `LatentSlateData/tmp/`; project-derived caches are written under each project folder's `.cache/`.

## Timeline Model

- `Video` tracks hold video clips, image stills, and visual generative clips.
- `Audio` tracks hold audio clips and audio generative clips.
- `Marker` tracks hold point-in-time markers.
- New projects start with three video tracks above one audio track and one marker track: `Video 3`, `Video 2`, `Video 1`, `Audio 1`, `Markers` from top to bottom.
- Video tracks keep visual output and embedded-audio mute as separate states. Video output affects preview and export compositing; audio mute affects playback and export mixdown.
- Clips are range-based with start time and duration.
- Time-based clips default to `crop` time mapping. Video clips can use `stretch` to map remaining source media across the visible clip duration.
- Timeline bridge clips are generated video clips with a `bridge` link to left/right source clips. They are anchored to those clips, reflow when source clips move, and expose edge resizing as left/right bridge frame counts instead of free timeline movement.
- Markers are point-based annotations.
- Image clips can display as normal stills or keyframe-reference pins, but they remain clips on video tracks.

## Assets And Generative Versions

All media is represented as an asset. Standard assets point at imported files.
Generative assets point at a UUID-keyed generated folder and a config file.

Generative config tracks:

- selected provider ID
- provider input values and asset references
- batch/seed settings
- generation records
- active version
- Asset Lab node lineage

The active version is the file shown on the timeline and used when another
generation references that asset. A generative asset with no active version is
intentionally hollow; the preview and provider-input paths do not scan its folder
for arbitrary leftover files.

Generative video assets store target timing as duration, FPS, and frame count. For
a hollow generative video used by one clip, resizing the clip updates that target
timing. After a version exists, clip resizing is treated as timeline editing;
target timing remains an explicit asset setting for future generations.

## Provider And Tool Model

`ProviderEntry` remains the shared frontend/runtime shape. It describes:

- stable provider/tool UUID
- output media type
- LatentSlate creative workflow kind
- schema-driven inputs and semantic roles
- adapter-specific connection/execution data

This lets the timeline, Attributes panel, Asset Lab, Agent API, generation queue,
media resolution, batching, seed handling, and version persistence stay shared.
The source of the entry may differ:

- **LatentSlate Engine:** automatically normalized from the Engine's versioned tool catalog.
- **ComfyUI:** loaded from a user-authored provider JSON and embedded graph manifest.
- **Cloud APIs:** loaded from a local provider JSON with adapter-specific settings.

The Engine is first-class rather than routed through generic `CustomHttp`. Its
connection records the endpoint, stable tool key, availability, and current
schema revision/hash. Engine schemas are read-only in LatentSlate; the Engine
registry is their source of truth.

`workflow_kind: "video_to_bridge"` providers are video seam tools that require
width, height, seed, left/right video, and timing roles, then receive pre-baked
source segments from the project timeline.

Current runtime adapters:

- LatentSlate Engine image/video/audio contract over HTTP; its catalog exposes built-in and data-defined H3, LTX, Wan, and Klein tools through the same schema-driven provider model.
- ComfyUI image/video/audio through workflow API JSON plus manifest bindings.
- OpenAI image.
- xAI image.
- xAI Grok video.

`CustomHttp` remains modeled but is not implemented at runtime.

## Provider Discovery

Local provider files are loaded first. LatentSlate then requests
`GET /v1/catalog` from the configured Engine and merges the resulting tools by
stable UUID. A live catalog replaces a local entry with the same UUID, preserving
the Engine as source of truth.

The last successful Engine catalog is cached. If the Engine is offline at app
startup, cached tools remain inspectable and selectable, but execution still
requires a reachable compatible Engine.

Every Engine job includes the catalog's schema revision and hash. The Engine
rejects stale requests explicitly. Project-level schema snapshots and a
reconciliation screen are not implemented yet; the stable identities and
revision/hash contract are the framework for that later work.

## Generation Flow

All providers enter the existing shared queue:

1. Resolve provider and current input values.
2. Resolve media inputs from project assets and timeline context.
3. Execute the adapter.
4. Save the returned bytes as the next project-local version.
5. Update config, active version, thumbnails, metadata, and preview state.

For LatentSlate Engine specifically:

1. Upload each resolved media input with multipart HTTP.
2. Replace local paths with Engine asset references.
3. Submit a schema-pinned asynchronous job.
4. Poll job state and forward progress into the existing queue UI.
5. Download the primary artifact over HTTP.
6. Hand the bytes back to the normal generative-version path.

The same flow works for localhost, LAN, and remote/Vast.ai deployments. There is
no shared-filesystem optimization in the public Engine contract.

See [PROVIDERS.md](./PROVIDERS.md) for setup details.

## Preview, Audio, And Export

Preview:

- Uses `ffmpeg-next` for media decode.
- Caches decoded frames and thumbnails.
- Uploads cached visual layers as egui textures for interactive preview.
- Applies transform handles and preview placement through the egui paint path.
- Exposes preview diagnostics through the UI and automation API.

Audio:

- Uses FFmpeg decode/resampling helpers.
- Uses `cpal` for playback.
- Builds waveform cache data for timeline rendering.
- Supports audio scrubbing and clip/track volume controls.

Export:

- Renders timeline frames through the preview/compositor path.
- Mixes timeline audio when enabled.
- Invokes `ffmpeg.exe` for MP4 muxing/encoding.
- Supports H.264/H.265, quality presets, optional timestamp overlay, and cancel/progress UI.

## Agent API And Automation

The desktop Agent API is loopback-only and opt-in through the top-bar API popover,
`--automation`, or `LATENTSLATE_AUTOMATION=1`. It exposes semantic commands,
current UI registry data, screenshots, preview diagnostics, generation queue
control, long-running generation waits, export control, self-documenting
help/schema routes, and rendered timeline/clip/asset captures.

State-changing Agent API commands should route through the highest practical
editor/app operation so the visible UI, preview caches, selection, dirty state,
queue panels, and timeline playhead update like human-driven actions. Read-only
captures do not move the visible timeline unless the request opts into `seek_ui`.

Rendered captures are saved under `LatentSlateData/tmp/agent-captures`. The app
clears this folder on startup. `normal` mode matches the compositor output as
closely as practical; `enhanced` mode adds agent-readable inspection overlays.

See [DESKTOP_TEST_HARNESS.md](./DESKTOP_TEST_HARNESS.md).
