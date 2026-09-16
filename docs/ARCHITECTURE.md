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

The separate Chat viewport uses `core/agent_chat.rs` for its worker-thread SSE/tool
loop and `core/agent_tools.rs` for a curated vocabulary with session-local short
handles. Tools call the shared editor and capture/generation helpers directly;
the loopback Agent API need not be enabled. Agent providers are a separate typed
model stored under `providers/agents/`, outside generation provider discovery.
Conversation history stays in memory. Binary media is sent on the next continuation
only, then replaced by text while preserving tool-call references, including after
failure or cancellation. Project document saves are explicit;
existing generation sidecar persistence is preserved.

Chat `look` accepts original image assets, frames selected by seconds or a project-FPS
frame index, and contact sheets. `timeline` uses viewer visibility; track handles
isolate a video track, including hidden tracks, on a project snapshot. `watch_video`
accepts video assets or explicit half-open timeline/track ranges. Assets are cut
with FFmpeg; timeline ranges use the existing export renderer. Silent H.264 proxies
preserve aspect within 320×320, cover at most 12 seconds, and are deleted after
encoding the attachment, including error/cancel paths. Media time starts at zero;
tool metadata supplies the original range and time domain. Native `input_video.data`
contains raw base64; backend presets determine sampling FPS. Each attachment is
limited to 32 MiB before base64 and the complete serialized request to 48 MiB.
These are app limits, not guarantees about backend context capacity.

## Shared UI language

`src/ui_kit.rs` owns shared surfaces, typography, fields, headers, tabs, and
interaction treatments. Choose surfaces by purpose, not nesting depth:

- `APP_BG`: application backdrop; `PANEL_SUNKEN`: media wells and editable fields.
- `PANEL`: panel and modal bodies; `PANEL_RAISED`: grouped cards and source entries.
- `CHROME`: headers and persistent navigation. Use borders to separate adjacent
  roles; do not add another shade for each nested container.

Modal headers share a left-aligned title, optional subtitle and icon, and one
right-aligned close control. They are 56 points high, or 72 with a subtitle;
`modal_header_layout` also reserves centered navigation and trailing actions.
`tab_bar` supplies a bounded 40-point strip and divider; `workspace_tab` anchors
its selection underline to the bottom. Asset Lab and AI Providers use these tabs.

Image-backed tool symbols live in `assets/icons` as transparent 96-pixel PNGs
with editable SVG masters. The kit caches textures and owns their tint, hit area,
hover, focus, and selected states. Existing unrelated icon controls can migrate
when their owning UI is reviewed; the application does not require two competing
styles for the same new control. Text fields use consistent left alignment in
both resting and focused states.

`Tooltip` provides a shared title, optional description, and optional shortcut hint;
tool buttons accept it directly, and other controls can apply it to their response.
Disabled tool buttons can explain their state. Create's mask editor supports B/E
for Brush/Eraser and [ / ] for brush diameter (1–512 canvas pixels). These shortcuts
yield to text editing, dialogs/popups, result audition, and active pointer gestures;
they do not change the saved authoring setup.

Source configuration uses `source_field`: a thumbnail, slot label, source value,
binding badge, and trailing chevron. Asset Lab uses its 48-point compact form in
a bounded grid; Attributes uses the comfortable form. Both open the same picker.
Canvas geometry, tool context, and lineage layout remain owned by Asset Lab.
Preview, Asset Lab Create/Compare, and lineage share `canvas_wheel_zoom_factor`
and its sensitivity multiplier. Wheel input respects UI clipping and foreground
layers, so an open picker or dialog shields the canvas behind it.

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
- Project workspace layout includes Chat's last native window position and content size. Closing Chat retains this placement in memory; saving the project persists it. Projects without a saved placement open Chat beside the main window with matching top and bottom edges.
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
- generation records, including optional accepted Engine tool/schema/recipe provenance
- active version
- Asset Lab node lineage

The active version is the file shown on the timeline and used when another
generation references that asset. A generative asset with no active version is
intentionally hollow; the preview and provider-input paths do not scan its folder
for arbitrary leftover files.

Generative video assets store media extent as FPS, frame count, and derived
duration. Hollow assets use predicted next-output timing; generated assets retain
actual media timing. Config timing inputs describe the next request independently.
Editing that request or switching providers does not retime existing output.
Providers may publish a discrete request-duration-to-output-frame mapping; LTX
uses this to distinguish nominal duration from its native encoded extent.
Ordinary hollow single-clip timing sync remains available for unmapped providers;
clip resizing after generation is timeline editing.

## Asset Lab authoring and source configuration

`GenerativeConfig` owns one current setup: native inputs, bindings, reference sizing,
batch policy, working version, mask document, prompt regions, and effect switches.
The pinned active version remains independent. Older selected drafts initialize this
setup; unfinished legacy nodes do not become separate workspaces.

Submitted jobs and completed records contain an `AssetLabSnapshot` alongside concrete
resolved-input provenance. Submitted masks and captured inputs use unique project-local
artifacts. A successful attempt contributes one version and one completed lineage node;
batch siblings share their submitted parent. Native integer seeds and reservations remain
authoritative.

The Lab session owns its result strip, revision, pending associations, and up to 20
coalesced undo actions with an approximate memory guard. Closing clears those; submission
clears undo without deleting authored content. Cameras, overlay visibility, and result
previews are presentation state. Explicit continuation restores the submitted setup and
starts a fresh session. Automatic continuation requires an unchanged, nonoverlapping
single request in its originating session, without active input, a modal, or audition.
Compare keeps the pinned output on the left and an explicit candidate on the right.
Acceptance changes only the pin and returns to Lineage.

Working-output bindings resolve through the native media resolver without rewriting fixed
version, project-current-output, or timeline bindings. The shared picker uses editor
operations for both Attributes and Asset Lab. Quick selections commit directly; Configure
stages changes for Apply/Cancel. Capture commits only after materialization succeeds.

Mask geometry records actual raster dimensions, resolved source/frame/crop, sizing, and
source identity. Painting uses canvas coordinates under zoom/pan; incompatible geometry
retains the document and requires matching alignment or explicit clearing. Region bounds
are normalized separately on each axis. Content, effect switches, and visibility are
independent. Authoring profiles select presentation only: they do not establish execution
capability. Masked and spatial execution have no supported Engine contract in this phase;
applicable enabled nonempty effects block submission, while disabled effects allow the
existing ordinary-generation path.

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

- LatentSlate Engine over HTTP: eight built-in tools and enabled user recipes across LTX 2.3, Klein 9B, and Wan 2.2 14B Turbo. The Engine owns inference and GPU worker lifecycle; the app owns project media and generation/version editing.
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

Every Engine job includes the catalog's schema revision/hash and, for user tools,
the exact recipe revision/hash. A 409 fails the attempt and refreshes providers;
it never resubmits automatically. A changed schema reconciles current asset and
Asset Lab inputs, preserving shared creative roles without repairing invalid
values. Compatible recipe refreshes retain inputs. Existing generated media and
accepted execution provenance remain unchanged. Project-level schema snapshots
and a reconciliation screen are not implemented.

## Generation Flow

All providers enter the existing shared queue:

1. Resolve provider and current input values, with local preflight diagnostics.
2. Resolve/materialize media inputs from project assets and timeline context; validate declared image/canvas requirements before submission.
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
