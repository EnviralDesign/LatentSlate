# Agent Control API

This is the v1 API for allowing an AI agent to operate a running LatentSlate
desktop instance. It is intentionally designed as an evolution of the existing
loopback automation harness, not as a separate control system.

## Goals

- Let an agent read the current editor/project state and write changes through
  the same editor/runtime paths the UI uses.
- Keep payloads compact, predictable, and close to the serialized project
  model where possible.
- Prefer semantic operations over UI clicks, while keeping UI discovery and
  widget actions as a fallback.
- Route state-changing commands through the highest practical app/editor path
  so the UI, preview, selection, dirty flags, audio scrub state, and caches
  update naturally.
- Give agents a visual feedback loop through rendered timeline, clip, and asset
  stills, not only full-window screenshots.
- Provide both normal compositor captures and enhanced inspection captures that
  make clip boundaries, IDs, selection, and timing easier for an agent to see.
- Keep the first implementation local and loopback-only.

## Non-Goals For v1

- Replacing the human Asset Lab UI with a full graph-authoring system.
- Implementing an MCP server before the local HTTP contract is stable.
- Requiring external providers for smoke tests.

## Transport

Use the current loopback HTTP server, versioned under `/agent/v1`.

```text
127.0.0.1:<port>/agent/v1
```

The existing endpoints can remain as compatibility aliases:

```text
GET  /health
GET  /state
GET  /ui
POST /ui/click
POST /ui/text
POST /screenshot
POST /command
```

Recommended v1 endpoints:

```text
GET  /agent/v1/health
GET  /agent/v1/capabilities
GET  /agent/v1/help
GET  /agent/v1/schema
GET  /agent/v1/state
GET  /agent/v1/projects
GET  /agent/v1/jobs
GET  /agent/v1/jobs/{job_id}
GET  /agent/v1/export
POST /agent/v1/command
POST /agent/v1/capture
POST /agent/v1/wait/generation
```

`/agent/v1/command` routes into the same internal `AutomationCommand` queue used
by the desktop harness.

The server binds to `127.0.0.1` and is disabled until the Agent API toggle,
`--automation`, or `LATENTSLATE_AUTOMATION=1` enables it. POST requests with a
body must use `Content-Type: application/json`; browser `Origin` headers are
accepted only for loopback or `null` origins. Request bodies are capped at 2 MiB.

## Self-Documentation

Agents can bootstrap without reading this file:

```text
GET /agent/v1/help
GET /agent/v1/schema
```

`/agent/v1/help` returns routes, envelope shape, a recommended workflow, and
copyable examples. `/agent/v1/schema` returns command groups, required and
optional fields, capture shapes, enums, and operational notes. These two routes,
plus health/capabilities, are readable on localhost even when the API popover
toggle is off; editor read/write routes still require the API to be enabled.

The top-right **API** popover also exposes **Copy Primer**. It copies a
skill-style plain-text bootstrap block with the base URL, current API status,
workflow notes, endpoint topology, current project settings/counts, selection,
and the first visible IDs for tracks/assets/clips. This is meant to be pasted
directly into another agent session.

## Enablement

The API should be a simple app setting backed by the same launch/env flags that
exist today. It should default to off for now, but the design should not depend
on that default staying off forever.

Supported enablement:

```powershell
latentslate.exe --automation --automation-port 47890
```

```powershell
$env:LATENTSLATE_AUTOMATION = "1"
$env:LATENTSLATE_AUTOMATION_PORT = "47890"
```

The top-right API popover should expose the same toggle and selected port. In v1 the
server only binds to `127.0.0.1`; that is enough for local agents and dev
harnesses.

## Generation Waits

`start_generation` is intentionally non-blocking. It queues work and returns the
job IDs the agent should track. Agents that need synchronization can call:

```http
POST /agent/v1/wait/generation
Content-Type: application/json
```

```json
{ "job_id": "uuid", "timeout_seconds": 1800, "poll_interval_ms": 500 }
```

If `job_id` is omitted, the endpoint waits until the entire generation queue has
no queued or running jobs. The default timeout is 30 minutes. Timeout responses
use HTTP `408` and include the current job or queue snapshot so the agent can
decide whether to continue waiting, cancel, or inspect the failure.

## State And UI Update Policy

Commands that change editor state should behave like a human performed the
equivalent action:

- `seek` should drive the same app-runtime seek path as timeline scrubbing, so
  playhead position, audio scrub behavior, preview invalidation, and visible UI
  state all update.
- clip, marker, track, asset, provider, generation, and project settings
  mutations should mark the project and preview dirty when appropriate.
- selection-changing commands should update inspector panels and visual
  selection affordances naturally.
- provider and generation commands should invalidate thumbnails, preview caches,
  and Asset Lab runtime state through the same paths the UI uses.

Read-only commands should not move the UI unless requested. For example,
capturing a cutsheet or reading an asset config should not seek the visible
timeline by default. Requests can opt in to UI movement:

```json
{ "type": "frame", "source": { "type": "timeline" }, "time": { "seconds": 8.0 }, "seek_ui": true }
```

The implementation should avoid brittle alternative control paths. If an API
operation would duplicate a complex UI mutation, promote that mutation into a
shared app/editor helper first, then call the helper from both UI and API.

## Response Envelope

All endpoints return the same envelope shape.

```json
{
  "ok": true,
  "message": null,
  "data": {}
}
```

Errors use HTTP status codes and the same envelope. A typed error object can be
added later if agents need richer retry decisions.

```json
{
  "ok": false,
  "message": "Clip not found.",
  "data": {}
}
```

Suggested error codes:

```text
invalid_request
not_found
conflict
unsupported
provider_offline
provider_error
timeout
internal
```

## Time And Frame References

Agents should be able to address visual positions using seconds, frames, or
percentages. The API normalizes these to seconds and returns the normalized
value.

```json
{ "time": { "seconds": 4.25 } }
{ "time": { "frame": 102 } }
{ "time": { "percent": 0.5 } }
{ "time": { "key": "first" } }
{ "time": { "key": "last" } }
{ "time": { "key": "current" } }
```

For clip-local or asset-local requests, `percent`, `first`, and `last` refer to
that clip or asset duration. For timeline requests, they refer to project
duration.

Returned time shape:

```json
{
  "seconds": 4.25,
  "frame": 102,
  "fps": 24.0,
  "scope": "timeline"
}
```

## State Snapshot

`GET /agent/v1/state` returns a compact snapshot of all agent-relevant state.
The `diagnostics` block is opt-in:

```text
GET /agent/v1/state?include=diagnostics
```

Shape:

```json
{
  "project": {
    "name": "Demo",
    "path": "C:/projects/Demo",
    "settings": {},
    "duration_seconds": 12.5,
    "tracks": [],
    "assets": [],
    "clips": [],
    "markers": [],
    "generative_configs": {}
  },
  "selection": {
    "assets": [],
    "clips": [],
    "tracks": [],
    "markers": []
  },
  "timeline": {
    "current_time": { "seconds": 0.0, "frame": 0, "fps": 24.0, "scope": "timeline" },
    "is_playing": false
  },
  "layout": {},
  "overlays": {},
  "providers": [],
  "queue": [],
  "diagnostics": {}
}
```

The resource objects should reuse the existing Rust/serde field names wherever
they are already stable: `Asset`, `AssetKind`, `Clip`, `ClipTransform`,
`Track`, `Marker`, `ProviderEntry`, `GenerativeConfig`, `GenerationRecord`, and
`AssetLabNode`.

## Command Envelope

`POST /agent/v1/command` accepts one semantic command.

```json
{
  "type": "set_clip",
  "clip_id": "uuid",
  "patch": {
    "start_time": 1.0,
    "duration": 3.0,
    "transform": {
      "position_x": 0.0,
      "position_y": 0.0,
      "scale_x": 1.0,
      "scale_y": 1.0,
      "rotation_deg": 0.0,
      "opacity": 1.0
    }
  }
}
```

Command responses return command-specific payloads in the standard response
envelope. If an agent needs a fresh global snapshot after a mutation, call
`GET /agent/v1/state?include=diagnostics`.

```json
{
  "ok": true,
  "data": {
    "clip_id": "uuid"
  }
}
```

## Project Commands

```json
{ "type": "list_projects" }
{ "type": "create_project", "parent_dir": "C:/projects", "name": "Demo", "settings": {} }
{ "type": "open_project", "folder": "C:/projects/Demo" }
{ "type": "save_project" }
{ "type": "set_project_settings", "patch": { "fps": 24.0, "width": 1920, "height": 1080 } }
```

`set_project_settings` should update preview renderer limits when settings
that affect preview dimensions change.

Project settings are a first-class API surface, not an incidental modal-only
operation. Agents need to set canvas size, FPS, duration, preview limits, and
other serialized settings before building or validating a project.

## Selection And Timeline Navigation

```json
{ "type": "seek", "time": 4.25 }
{ "type": "set_playback", "playing": false }
{ "type": "step_timeline", "frames": 1 }
{ "type": "set_selection", "assets": ["uuid"], "clips": [], "tracks": [], "markers": [] }
{ "type": "select_asset", "asset_id": "uuid" }
{ "type": "select_clip", "clip_id": "uuid" }
{ "type": "select_track", "track_id": "uuid" }
{ "type": "select_marker", "marker_id": "uuid" }
```

`seek` should route through the app runtime `seek_editor` path so audio scrub
state, preview invalidation, visible playhead state, and performance sampling
stay coherent.

## Asset Commands

```json
{ "type": "import_asset", "path": "C:/media/source.mp4" }
{ "type": "rename_asset", "asset_id": "uuid", "name": "Hero shot" }
{ "type": "duplicate_asset", "asset_id": "uuid" }
{ "type": "delete_assets", "asset_ids": ["uuid"] }
{ "type": "set_asset_duration", "asset_id": "uuid", "duration_seconds": 8.0 }
```

Generative asset creation:

```json
{ "type": "create_generative_asset", "output_type": "image", "name": "Concept still" }
{ "type": "create_generative_asset", "output_type": "video", "fps": 16.0, "frame_count": 81 }
{ "type": "create_generative_asset", "output_type": "audio" }
```

Extraction:

```json
{ "type": "extract_active_generation", "asset_id": "uuid" }
{ "type": "extract_generation_version", "asset_id": "uuid", "version": "v3" }
{ "type": "extract_still_to_asset", "source": { "type": "asset", "asset_id": "uuid", "version": "v3" }, "time": { "percent": 0.5 } }
```

`extract_still_to_asset` is a persistent project mutation. Visual inspection
captures are handled separately by `/agent/v1/capture`.

The implemented source shape matches capture sources:

```json
{ "type": "extract_still_to_asset", "source": { "type": "timeline" }, "time": { "seconds": 12.0 }, "name": "Timeline proof" }
{ "type": "extract_still_to_asset", "source": { "type": "clip", "clip_id": "uuid" }, "time": { "percent": 0.5 } }
{ "type": "extract_still_to_asset", "source": { "type": "asset", "asset_id": "uuid", "version": "v3" }, "time": { "key": "last" } }
```

## Track Commands

```json
{ "type": "add_track", "track_type": "Video", "index": 0, "name": "Plate" }
{ "type": "set_track", "track_id": "uuid", "patch": { "name": "Dialog", "muted": false, "volume": 0.8 } }
{ "type": "move_track", "track_id": "uuid", "index": 2 }
{ "type": "delete_track", "track_id": "uuid" }
```

`delete_track` should return deletion counts before applying when
`dry_run: true` is supplied.

```json
{ "type": "delete_track", "track_id": "uuid", "dry_run": true }
```

## Clip Commands

```json
{
  "type": "add_asset_to_timeline",
  "asset_id": "uuid",
  "track_id": "uuid",
  "time": 2.0,
  "duration_seconds": 4.0
}
```

Patch clip fields:

```json
{
  "type": "set_clip",
  "clip_id": "uuid",
  "patch": {
    "track_id": "uuid",
    "start_time": 2.0,
    "duration": 4.0,
    "trim_in_seconds": 0.5,
    "volume": 1.0,
    "label": "I2V bridge",
    "image_mode": "still",
    "transform": {
      "position_x": 0.0,
      "position_y": 0.0,
      "scale_x": 1.0,
      "scale_y": 1.0,
      "rotation_deg": 0.0,
      "opacity": 1.0
    }
  }
}
```

Convenience actions:

```json
{ "type": "move_clip", "clip_id": "uuid", "start_time": 3.0, "track_id": "uuid" }
{
  "type": "move_clips",
  "mode": "absolute",
  "moves": [
    { "clip_id": "uuid", "start_time": 3.0, "track_id": "uuid" },
    { "clip_id": "uuid", "start_time": 5.5 }
  ]
}
{
  "type": "move_clips",
  "mode": "relative",
  "clip_ids": ["uuid", "uuid"],
  "delta_seconds": 1.5,
  "track_delta": 1
}
{ "type": "resize_clip", "clip_id": "uuid", "start_time": 3.0, "duration": 6.0 }
{ "type": "delete_clips", "clip_ids": ["uuid"] }
```

`move_clips` preflights every target before mutating the project. Absolute
mode accepts per-clip `start_time` and/or `track_id`. Relative mode preserves
each selected clip's offsets while applying `delta_seconds`; use `track_delta`
to move each clip up/down by timeline row, or `track_id` to move every clip to
one explicit target track. `track_delta` and `track_id` are mutually exclusive.

## Marker Commands

```json
{ "type": "add_marker", "time": 3.5, "track_id": "uuid", "label": "Beat" }
{ "type": "set_marker", "marker_id": "uuid", "patch": { "time": 4.0, "label": "Cut", "description": "", "color": "#f97316" } }
{ "type": "delete_marker", "marker_id": "uuid" }
```

## Provider Commands

Provider entries should use the same shape as provider JSON files.
Responses redact provider API keys; cloud and custom provider
`connection.api_key` values are replaced with `api_key_present`.

Provider and provider-input descriptions are first-class metadata for agents.
`ProviderEntry.description` is an optional multi-line string describing when to
use the provider. Each `ProviderInputField.description` is an optional
multi-line string explaining the parameter. Blank descriptions are omitted from
JSON. Agents should prefer these descriptions over guessing from ComfyUI node
names when choosing providers or setting input values.

```json
{ "type": "list_providers" }
{ "type": "refresh_providers" }
{ "type": "create_provider", "provider": {} }
{ "type": "update_provider", "provider_id": "uuid", "provider": {} }
{ "type": "delete_provider", "provider_id": "uuid" }
{ "type": "test_provider", "provider_id": "uuid", "live": true }
```

Template creation:

```json
{ "type": "create_provider_from_template", "template": "comfy_ui" }
{ "type": "create_provider_from_template", "template": "openai_image" }
{ "type": "create_provider_from_template", "template": "xai_image" }
{ "type": "create_provider_from_template", "template": "xai_video" }
```

Provider Builder can remain UI-first in v1, but the API should support the
minimum file-based ComfyUI setup:

```json
{
  "type": "create_provider",
  "provider": {
    "name": "Wan I2V",
    "description": "Image-to-video provider that turns a still frame into a short motion clip.",
    "output_type": "video",
    "workflow_kind": "image_to_video",
    "inputs": [
      {
        "name": "prompt",
        "label": "Prompt",
        "description": "Describe the motion, camera movement, and visual changes to apply.",
        "input_type": { "type": "text" },
        "required": true,
        "ui": { "multiline": true }
      }
    ],
    "connection": {
      "type": "comfy_ui",
      "base_url": "http://127.0.0.1:8188",
      "workflow_path": "workflows/wan_i2v_API.json",
      "manifest": {
        "adapter_type": "comfy_ui",
        "schema_version": 1,
        "output_type": "video",
        "workflow": { "workflow_path": "workflows/wan_i2v_API.json" },
        "inputs": [],
        "output": {
          "selector": {
            "node_id": "42",
            "class_type": "SaveVideo",
            "input_key": "videos"
          }
        }
      }
    }
  }
}
```

Cloud provider API keys live directly in provider JSON as
`connection.api_key`. API responses redact provider API keys, but agents can set
or replace them by creating/updating the provider entry.

```json
{
  "type": "update_provider",
  "provider_id": "uuid",
  "provider": {
    "id": "uuid",
    "name": "OpenAI Image",
    "output_type": "image",
    "workflow_kind": "text_to_image",
    "inputs": [],
    "connection": {
      "type": "open_ai_image",
      "api_key": "sk-...",
      "model": "gpt-image-2"
    }
  }
}
```

## Generative Commands

Read a generative asset config:

```json
{ "type": "get_generative_config", "asset_id": "uuid" }
```

Patch a generative config:

```json
{
  "type": "set_generative_config",
  "asset_id": "uuid",
  "patch": {
    "provider_id": "uuid",
    "inputs": {
      "prompt": { "type": "literal", "value": "A wide desert shot at sunset" },
      "nla_input_image": {
        "type": "asset_ref",
        "asset_id": "uuid",
        "source_clip_id": "uuid",
        "pinned": true,
        "frame_reference": "first"
      }
    },
    "reference_slots": {
      "start_image": {
        "type": "asset_ref",
        "asset_id": "uuid",
        "source_clip_id": "uuid",
        "pinned": true,
        "frame_reference": "first"
      }
    },
    "batch": { "count": 2, "seed_strategy": "increment" }
  }
}
```

`inputs` is the canonical API shape for provider parameters, including
image/video/audio assets. `reference_slots` remains useful for timeline hints
and older agent calls; when a reference slot matches a media provider field name
or semantic slot (`image`, `start_image`, `end_image`, `video`, `audio`) and
`inputs.<field>` is absent, LatentSlate copies it into `inputs`.

Version operations:

```json
{ "type": "set_active_generation_version", "asset_id": "uuid", "version": "v2" }
{ "type": "duplicate_generation_version", "asset_id": "uuid", "version": "v2" }
{ "type": "delete_generation_version", "asset_id": "uuid", "version": "v2" }
```

Start generation:

```json
{
  "type": "start_generation",
  "asset_id": "uuid",
  "context_clip_id": "uuid",
  "wait": false
}
```

Response:

```json
{
  "jobs": [
    { "id": "uuid", "status": "queued", "asset_id": "uuid" }
  ],
  "wait_requested": false
}
```

Generation queue commands:

```json
{ "type": "list_jobs" }
{ "type": "get_job", "job_id": "uuid" }
{ "type": "cancel_job", "job_id": "uuid" }
```

For v1, generation progress can be polled. Server-sent events can be added
later once the command and job model stabilizes.

## Agent-Friendly Continuation Commands

These are semantic wrappers around existing UI flows. They are not required for
the first implementation, but they should be preferred over forcing an agent to
manually create every config reference.

```json
{ "type": "create_i2i_from_clip", "clip_id": "uuid", "provider_id": "uuid" }
{ "type": "create_i2v_from_clip", "clip_id": "uuid", "provider_id": "uuid", "reference": "image" }
{ "type": "create_i2v_from_clip", "clip_id": "uuid", "provider_id": "uuid", "reference": "video_first_frame" }
{ "type": "create_i2v_from_clip", "clip_id": "uuid", "provider_id": "uuid", "reference": "video_last_frame" }
{ "type": "create_bridge_from_clips", "clip_ids": ["uuid", "uuid"], "provider_id": "uuid" }
```

These return created `asset_ids`, `clip_ids`, the resulting selection, and
status text.

## Export Commands

Agents can start, inspect, and cancel video export through the same export modal
state and worker used by the human UI.

```json
{
  "type": "export_video",
  "request": {
    "output_path": "C:/projects/Demo/exports/demo.mp4",
    "codec": "h264",
    "width": 1280,
    "height": 720,
    "fps": 24.0,
    "start_seconds": 0.0,
    "duration_seconds": 30.0,
    "include_audio": true,
    "quality": "balanced",
    "frame_format": "png",
    "timestamp_overlay": { "enabled": true, "position": "bottom_center" },
    "open_panel": true
  }
}
```

```json
{ "type": "get_export_status" }
{ "type": "cancel_export" }
```

## Asset Lab v1 Scope

Asset Lab can remain a human visual flow, but the API should expose the
underlying data and core version actions:

```json
{ "type": "get_asset_lab_graph", "asset_id": "uuid" }
{ "type": "add_asset_lab_node", "asset_id": "uuid", "provider_id": "uuid", "parent_node_id": "uuid", "inputs": {} }
{ "type": "set_asset_lab_node", "asset_id": "uuid", "node_id": "uuid", "patch": { "provider_id": "uuid", "inputs": {}, "selected": true } }
{ "type": "delete_asset_lab_node", "asset_id": "uuid", "node_id": "uuid" }
{ "type": "generate_asset_lab_node", "asset_id": "uuid", "node_id": "uuid" }
```

This is enough for agents to read lineage, select versions, and trigger a node.
More visual graph layout controls can wait.

## Capture API

`POST /agent/v1/capture` is separate from viewport screenshots. It returns
rendered media artifacts intended for visual model ingestion.

The same capture request can also be sent through `POST /agent/v1/command` by
wrapping it as `{ "type": "capture", "request": { ... } }`. Prefer the direct
capture endpoint when possible.

Capture outputs should default to:

```text
LatentSlateData/tmp/agent-captures/<timestamp>-<slug>/
```

The `LatentSlateData` root is created beside the running executable unless
`LATENTSLATE_HOME` points at an explicit app data folder. Each app startup
creates `tmp/agent-captures` if needed and removes any previous capture
artifacts inside it, so captures are session scratch output. Each capture
returns absolute paths and a manifest. Persistent captures can later use a
project-local `exports/captures/` folder when requested.

Capture mode:

```json
{ "mode": "normal" }
{ "mode": "enhanced" }
```

`normal` should match the renderer/compositor output as closely as practical.
`enhanced` should add agent-readable visual aids without mutating the project:

- clip outlines and translucent clip bounds
- selected clip/asset markers
- clip IDs or short labels
- timeline timecode and frame number
- optional safe/title frame guides
- optional layer order numbers

### Timeline Frame

```json
{
  "type": "frame",
  "source": { "type": "timeline" },
  "time": { "seconds": 4.25 },
  "format": "png",
  "mode": "normal",
  "annotate": true
}
```

Response:

```json
{
  "capture": {
    "id": "uuid",
    "kind": "frame",
    "path": "C:/path/to/latentslate/LatentSlateData/tmp/agent-captures/demo/frame-0001.png",
    "manifest_path": "C:/path/to/latentslate/LatentSlateData/tmp/agent-captures/demo/manifest.json",
    "time": { "seconds": 4.25, "frame": 102, "fps": 24.0, "scope": "timeline" },
    "stats": {}
  }
}
```

Implementation should use `PreviewRenderer::render_frame_rgba` for timeline
captures.

### Clip Frame

```json
{
  "type": "frame",
  "source": { "type": "clip", "clip_id": "uuid" },
  "time": { "percent": 0.5 },
  "format": "png",
  "mode": "enhanced",
  "annotate": true
}
```

The API resolves clip-local time to timeline time and returns both.

```json
{
  "time": { "seconds": 8.0, "frame": 192, "fps": 24.0, "scope": "timeline" },
  "local_time": { "seconds": 1.5, "frame": 36, "fps": 24.0, "scope": "clip" }
}
```

### Asset Frame

```json
{
  "type": "frame",
  "source": { "type": "asset", "asset_id": "uuid", "version": "v2" },
  "time": { "key": "last" },
  "format": "png",
  "mode": "enhanced",
  "annotate": true
}
```

Asset frame capture should use the same video decode path used by Asset Lab
previews for video assets and direct image loading for image assets.

### Cutsheet

```json
{
  "type": "cutsheet",
  "source": { "type": "timeline" },
  "frames": [
    { "label": "start", "time": { "percent": 0.0 } },
    { "label": "middle", "time": { "percent": 0.5 } },
    { "label": "end", "time": { "key": "last" } }
  ],
  "layout": { "columns": 3, "thumb_width": 384 },
  "mode": "enhanced",
  "annotate": true
}
```

Response:

```json
{
  "capture": {
    "id": "uuid",
    "kind": "cutsheet",
    "path": "C:/path/to/latentslate/LatentSlateData/tmp/agent-captures/demo/cutsheet.png",
    "manifest_path": "C:/path/to/latentslate/LatentSlateData/tmp/agent-captures/demo/manifest.json",
    "frames": [
      {
        "label": "start",
        "path": "C:/path/to/latentslate/LatentSlateData/tmp/agent-captures/demo/frame-0001.png",
        "time": { "seconds": 0.0, "frame": 0, "fps": 24.0, "scope": "timeline" }
      }
    ]
  }
}
```

Cutsheet annotations should be simple and machine-readable:

```text
01 start | timeline 00:00:00:00 | 0.000s
02 middle | timeline 00:00:06:06 | 6.250s
```

## Screenshot API

Viewport screenshots remain useful for UI debugging.

```json
{
  "type": "screenshot",
  "name": "providers-open"
}
```

Use this shape with `POST /command`, or post `{ "name": "providers-open" }`
directly to `POST /screenshot`. This uses egui viewport screenshots and returns
a PNG path. It is not a replacement for rendered timeline or asset captures.

## Timeline View And Playback

Agents can deterministically frame screenshots and drive transport controls:

```json
{ "type": "set_playback", "playing": true }
{ "type": "step_timeline", "frames": -1 }
{ "type": "set_layout", "timeline_zoom": 24.0, "timeline_scroll_x": 120.0, "timeline_height": 260.0 }
```

`set_playback` and `step_timeline` are runtime commands so preview/audio state is
updated through the same path as the transport UI. `set_layout` also accepts
`left_width`, `right_width`, `timeline_height`, `timeline_zoom`,
`timeline_scroll_x`, and `timeline_scroll_y` in addition to the collapsed and
preview toggles.

## Capabilities

`GET /agent/v1/capabilities` should let an agent discover supported commands
without guessing.

```json
{
  "api_version": "agent-v1",
  "commands": ["get_state", "set_clip"],
  "capture": {
    "sources": ["timeline", "clip", "asset"],
    "formats": ["png"],
    "modes": ["normal", "enhanced"],
    "cutsheet": true,
    "limits": { "max_cutsheet_frames": 24, "max_cutsheet_thumb_width": 1024 }
  }
}
```

This can start as a hand-authored static list.

## Implementation Status

The v1 API currently includes versioned routes, the API popover toggle/port UI,
semantic commands for projects/assets/timeline/providers/generation/Asset Lab,
generation wait synchronization, normal and enhanced rendered captures,
diagnostics state, and the copyable Agent API Primer.

## Coverage Checklist

The intended coverage surface is:

- project create/open/save and project settings mutation
- provider CRUD, refresh, and low-cost provider smoke checks
- asset import, rename, duplicate, delete, generative asset creation, version
  actions, and extraction
- track, clip, marker, selection, layout, timeline navigation, and preview
  state mutations
- generation queue start/cancel/status, with a few controlled OpenAI/xAI/Grok or
  ComfyUI spot checks where provider JSON API keys/providers are configured
- timeline, clip, asset, screenshot, and cutsheet capture in normal and
  enhanced modes
- state-changing calls verified through app state plus harness/screenshots
  where the expected result is visual
- read-only capture/inspection calls verified not to disturb visible timeline
  state unless `seek_ui` is requested
- final reusable validation project saved under `LatentSlateData/projects/agent-validation/`, with a
  silly but diagnostic 30-second timeline exercising images, video, audio or
  waveforms where available, labels, transforms, markers, generated versions,
  and capture outputs

## Verification

Use `cargo check` for every code change and `cargo build --release` after
Rust/UI changes, unless the running executable locks the target binary. Focused
unit tests cover schema/bootstrap behavior, capture time normalization, request
guards, provider redaction, and targeted editor command semantics. Broader
desktop validation should use `scripts/automation-scenario.ps1` plus direct
`/agent/v1/help`, `/agent/v1/schema`, and `/agent/v1/capture` smoke checks.

