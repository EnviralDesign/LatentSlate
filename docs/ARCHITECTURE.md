# Architecture

This is the concise architecture reference for the current app. It describes what exists now, not an aspirational design.

## System Shape

```text
egui/eframe desktop shell
        |
        v
Editor model/controller (`src/editor.rs`)
        |
        +--> Project/state model (`src/state/`)
        +--> Preview/export/audio/media core (`src/core/`)
        +--> Provider execution (`src/providers/`)
        +--> Loopback automation (`src/core/automation.rs`)
```

The UI should call shared editor/core operations instead of duplicating behavior in widget code. The automation harness also routes through those paths where practical.

## Project Model

A project is a folder. The app stores imported and generated media inside that folder so projects can be moved or zipped more predictably.

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
- Writable app-managed state lives under `LatentSlateData/` next to the running executable unless `LATENTSLATE_HOME` is set.
- Default projects are written to `LatentSlateData/projects/`.
- Provider entries are written to `LatentSlateData/providers/`.
- Provider Builder writes new ComfyUI manifests to `LatentSlateData/provider-manifests/`; existing providers can still reference older explicit manifest paths.
- API credentials are written to `LatentSlateData/secrets/credentials.json` and are encrypted on Windows.
- App scratch files are written under `LatentSlateData/tmp/`; project-derived caches are written under each project folder's `.cache/`.
- Legacy `.latentslate/providers` and `.latentslate/secrets/credentials.json` are copied into an empty app data folder for dev compatibility.

## Timeline Model

- `Video` tracks hold video clips, image stills, and visual generative clips.
- `Audio` tracks hold audio clips and audio generative clips.
- `Marker` tracks hold point-in-time markers.
- Clips are range-based with start time and duration.
- Markers are point-based annotations.
- Image clips can display as normal stills or keyframe-reference pins, but they remain clips on video tracks.

## Assets And Generative Versions

All media is represented as an asset. Standard assets point at imported files. Generative assets point at a UUID-keyed generated folder and a config file.

Generative config tracks:

- selected provider ID
- provider input values and asset references
- batch/seed settings
- generation records
- active version
- Asset Lab node lineage

The active version is the file shown on the timeline and used when another generation references that asset. A generative asset with no active version is intentionally hollow; the preview and provider-input paths do not scan its folder for arbitrary leftover files.

## Provider Flow

Provider entries describe an output type, input schema, and connection.

Current runtime adapters:

- ComfyUI image/video/audio through workflow API JSON plus manifest bindings.
- OpenAI image.
- xAI image.
- xAI Grok video.

`CustomHttp` is modeled but not implemented at runtime.

Generation jobs flow through the shared queue:

1. Resolve provider and current input values.
2. Resolve media inputs from project assets/timeline context when supported.
3. Execute provider adapter.
4. Save output as the next version under the generative asset folder.
5. Update config, active version, thumbnails, and preview state.

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

The desktop Agent API is loopback-only and opt-in through the top-bar API
popover, `--automation`, or `LATENTSLATE_AUTOMATION=1`. It exposes semantic
commands, current UI registry data, screenshots, preview diagnostics, generation
queue control, long-running generation waits, export control, self-documenting
help/schema routes, and rendered timeline/clip/asset captures.

State-changing Agent API commands should route through the highest practical
editor/app operation so the visible UI, preview caches, selection, dirty state,
queue panels, and timeline playhead update like human-driven actions. Read-only
captures do not move the visible timeline unless the request opts into `seek_ui`.

Rendered captures are saved under
`LatentSlateData/tmp/agent-captures`. The app clears this folder on startup so
each launched session begins with an empty capture scratch area. `normal` mode
matches the compositor output as closely as practical; `enhanced` mode adds
agent-readable inspection overlays such as timing labels and clip/source
boundaries.

See [DESKTOP_TEST_HARNESS.md](./DESKTOP_TEST_HARNESS.md).
