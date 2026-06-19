# Agent Control API Draft

This is a proposed v1 API for allowing an AI agent to operate a running
LatentSlate desktop instance. It is intentionally designed as an evolution of
the existing loopback automation harness, not as a separate control system.

## Goals

- Let an agent read the current editor/project state and write changes through
  the same editor/runtime paths the UI uses.
- Keep payloads compact, predictable, and close to the serialized project
  model where possible.
- Prefer semantic operations over UI clicks, while keeping UI discovery and
  widget actions as a fallback.
- Give agents a visual feedback loop through rendered timeline, clip, and asset
  stills, not only full-window screenshots.
- Keep the first implementation local, opt-in, and loopback-only.

## Non-Goals For v1

- Exposing the API off-host.
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
GET  /agent/v1/state
POST /agent/v1/command
POST /agent/v1/capture
GET  /agent/v1/jobs/{job_id}
```

The first implementation can route `/agent/v1/command` into the same internal
`AutomationCommand` queue used today.

## Security And Enablement

The server should stay disabled by default.

Supported enablement:

```powershell
latentslate.exe --automation --automation-port 47890
```

```powershell
$env:LATENTSLATE_AUTOMATION = "1"
$env:LATENTSLATE_AUTOMATION_PORT = "47890"
```

Future settings UI can expose the same switch, but should still bind only to
`127.0.0.1` in v1. There is no auth in v1 because the server is loopback-only.
Any non-loopback mode should require authentication and a command allowlist.

## Response Envelope

All endpoints return the same envelope shape.

```json
{
  "ok": true,
  "message": null,
  "data": {},
  "error": null
}
```

Errors should use HTTP status codes and a typed error object.

```json
{
  "ok": false,
  "message": "Clip not found.",
  "data": {},
  "error": {
    "code": "not_found",
    "target": "clip",
    "retryable": false
  }
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
Use query params later if the full snapshot becomes too large:

```text
GET /agent/v1/state?include=project,providers,queue,diagnostics
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
  },
  "return": ["state", "clip"]
}
```

Command responses should return the changed resource and, when requested, a
fresh state snapshot.

```json
{
  "ok": true,
  "data": {
    "clip": {},
    "state": {}
  }
}
```

The `return` array is optional. Defaults:

- Mutations return the changed resource and IDs.
- Long-running actions return a `job`.
- Read-only commands return the requested data.

## Project Commands

```json
{ "type": "create_project", "parent_dir": "C:/projects", "name": "Demo", "settings": {} }
{ "type": "open_project", "folder": "C:/projects/Demo" }
{ "type": "save_project" }
{ "type": "set_project_settings", "patch": { "fps": 24.0, "width": 1920, "height": 1080 } }
```

`set_project_settings` should update preview renderer limits when settings
that affect preview dimensions change.

## Selection And Timeline Navigation

```json
{ "type": "seek", "time": { "seconds": 4.25 } }
{ "type": "set_selection", "assets": ["uuid"], "clips": [], "tracks": [], "markers": [] }
{ "type": "select_asset", "asset_id": "uuid" }
{ "type": "select_clip", "clip_id": "uuid" }
{ "type": "select_track", "track_id": "uuid" }
{ "type": "select_marker", "marker_id": "uuid" }
```

`seek` should route through the app runtime `seek_editor` path when available so
audio scrub state and preview invalidation stay coherent.

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
{ "type": "extract_still_to_asset", "source": { "asset_id": "uuid", "version": "v3" }, "time": { "percent": 0.5 } }
```

`extract_still_to_asset` is a persistent project mutation. Visual inspection
captures are handled separately by `/agent/v1/capture`.

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
  "time": { "seconds": 2.0 },
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
{ "type": "resize_clip", "clip_id": "uuid", "edge": "end", "time": { "seconds": 6.0 } }
{ "type": "delete_clips", "clip_ids": ["uuid"] }
```

## Marker Commands

```json
{ "type": "add_marker", "time": { "seconds": 3.5 }, "track_id": "uuid", "label": "Beat" }
{ "type": "set_marker", "marker_id": "uuid", "patch": { "time": 4.0, "label": "Cut", "description": "", "color": "#f97316" } }
{ "type": "delete_marker", "marker_id": "uuid" }
```

## Provider Commands

Provider entries should use the same shape as provider JSON files.

```json
{ "type": "list_providers" }
{ "type": "refresh_providers" }
{ "type": "create_provider", "provider": {} }
{ "type": "update_provider", "provider_id": "uuid", "patch": {} }
{ "type": "delete_provider", "provider_id": "uuid" }
{ "type": "test_provider", "provider_id": "uuid" }
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
    "output_type": "video",
    "workflow_kind": "image_to_video",
    "inputs": [],
    "connection": {
      "type": "comfy_ui",
      "base_url": "http://127.0.0.1:8188",
      "workflow_path": "workflows/wan_i2v_API.json",
      "manifest_path": "workflows/wan_i2v.manifest.json"
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
      "prompt": { "type": "literal", "value": "A wide desert shot at sunset" }
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
  "job": {
    "id": "uuid",
    "kind": "generation",
    "status": "queued",
    "asset_id": "uuid",
    "queued_jobs": ["uuid"]
  }
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
{ "type": "create_i2v_from_clip", "clip_id": "uuid", "provider_id": "uuid", "reference": "first_frame" }
{ "type": "create_i2v_from_clip", "clip_id": "uuid", "provider_id": "uuid", "reference": "last_frame" }
{ "type": "create_bridge_from_clips", "clip_ids": ["uuid", "uuid"], "provider_id": "uuid" }
```

These should return the created `asset_id`, `clip_id`, and full generative
config.

## Asset Lab v1 Scope

Asset Lab can remain a human visual flow, but the API should expose the
underlying data and core version actions:

```json
{ "type": "get_asset_lab_graph", "asset_id": "uuid" }
{ "type": "add_asset_lab_node", "asset_id": "uuid", "provider_id": "uuid", "parent_node_id": "uuid" }
{ "type": "set_asset_lab_node", "asset_id": "uuid", "node_id": "uuid", "patch": {} }
{ "type": "delete_asset_lab_node", "asset_id": "uuid", "node_id": "uuid" }
{ "type": "generate_asset_lab_node", "asset_id": "uuid", "node_id": "uuid" }
```

This is enough for agents to read lineage, select versions, and trigger a node.
More visual graph layout controls can wait.

## Capture API

`POST /agent/v1/capture` is separate from viewport screenshots. It returns
rendered media artifacts intended for visual model ingestion.

Capture outputs should default to:

```text
.tmp/agent-captures/<project-or-session>/<timestamp>-<slug>/
```

Each capture returns absolute paths and a manifest. The folder is ignored by
git. Persistent captures can later use a project-local `exports/captures/`
folder when requested.

### Timeline Frame

```json
{
  "type": "frame",
  "source": { "type": "timeline" },
  "time": { "seconds": 4.25 },
  "format": "png",
  "annotate": true
}
```

Response:

```json
{
  "capture": {
    "id": "uuid",
    "kind": "frame",
    "path": "C:/repos/.../.tmp/agent-captures/demo/frame-0001.png",
    "manifest_path": "C:/repos/.../.tmp/agent-captures/demo/manifest.json",
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
  "annotate": true
}
```

Response:

```json
{
  "capture": {
    "id": "uuid",
    "kind": "cutsheet",
    "path": "C:/repos/.../.tmp/agent-captures/demo/cutsheet.png",
    "manifest_path": "C:/repos/.../.tmp/agent-captures/demo/manifest.json",
    "frames": [
      {
        "label": "start",
        "path": "C:/repos/.../.tmp/agent-captures/demo/frame-0001.png",
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

This should keep using egui viewport screenshots and return a PNG path. It is
not a replacement for rendered timeline or asset captures.

## Capabilities

`GET /agent/v1/capabilities` should let an agent discover supported commands
without guessing.

```json
{
  "api_version": "agent-v1",
  "commands": [
    {
      "type": "set_clip",
      "mutates": ["project.clips", "selection"],
      "returns": ["clip"],
      "available": true
    }
  ],
  "capture": {
    "sources": ["timeline", "clip", "asset"],
    "formats": ["png"],
    "cutsheet": true
  }
}
```

This can start as a hand-authored static list.

## Implementation Plan

1. Add versioned routes while preserving existing automation endpoints.
2. Extend the command enum with missing semantic editor operations:
   tracks, clips, markers, project settings, providers, generative configs, and
   queue controls.
3. Move UI-only mutation logic into shared editor/app methods where needed.
4. Add capture service helpers for timeline, clip, asset, and cutsheet PNGs.
5. Extend `state_json` into an agent state snapshot with provider, queue,
   generative config, and diagnostics sections.
6. Update `docs/DESKTOP_TEST_HARNESS.md` after the API is implemented.

## Verification Plan

- `cargo check`.
- Focused unit tests for time normalization and cutsheet frame selection.
- Automation smoke script that:
  - opens or creates a project,
  - imports media,
  - edits a clip transform,
  - captures a timeline cutsheet,
  - creates or patches a generative config without running an external provider.
- Manual provider/generation verification remains opt-in.

