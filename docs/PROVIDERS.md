# Providers

LatentSlate is built around user-owned generation backends. ComfyUI remains the
primary bring-your-own workflow path. LatentSlate Engine is the experimental
first-party path for a small, opinionated catalog of automatically described
tools.

Both paths normalize into the same provider-facing model inside LatentSlate:
output type, creative workflow kind, semantic inputs, progress, and generated
artifacts. Their source of truth is intentionally different: users author ComfyUI
providers, while the Engine publishes its own tool catalog.

## Current Adapter Status

| Adapter | Status | Notes |
|---|---|---|
| LatentSlate Engine | Experimental | Discovers versioned tools from `/v1/catalog`, uploads media, submits/polls jobs, and downloads outputs over HTTP. |
| ComfyUI | Implemented | API workflow JSON plus embedded manifest bindings. Supports image/video/audio output detection by file extension. |
| OpenAI image | Experimental | Stores `connection.api_key` in provider JSON. |
| xAI image | Experimental | Stores `connection.api_key` in provider JSON. |
| xAI Grok video | Experimental | Submits/polls/downloads video results through xAI API. |
| Custom HTTP | Not implemented | Data model exists; runtime returns a planned/not-implemented error. |
| fal.ai / Replicate / Veo | Not implemented | Future adapter work. |

## LatentSlate Engine Setup

LatentSlate checks for an Engine at `http://127.0.0.1:8765` when providers are
loaded. When reachable, its catalog tools appear automatically in provider
pickers and generation forms. There is no provider JSON to export, bind, or
repair.

For a local source checkout, initialize the portable Engine data root, inspect
it, validate its resources/variants, and start the service:

```powershell
cd C:\repos\LatentSlate-Engine
uv sync
uv run latentslate-engine data init
uv run latentslate-engine data path
uv run latentslate-engine resources list
uv run latentslate-engine variants validate
uv run latentslate-engine serve --host 127.0.0.1 --port 8765
```

`LATENTSLATE_ENGINE_HOME` owns the portable `models/`, `loras/`, `variants/`,
`cache/`, and `jobs/` trees. A local model or LoRA is added by placing it under
the matching family directory with its inspectable TOML sidecar, then placing
or editing a variant TOML that selects it. Restart the Engine to rebuild the
catalog; reload providers or restart LatentSlate to consume the refreshed
schemas. Unsupported or incomplete artifacts remain visible in Engine
diagnostics but are not advertised as runnable tools.

The machine-level connection can be changed with environment variables:

```text
LATENTSLATE_ENGINE_URL=http://127.0.0.1:8765
LATENTSLATE_ENGINE_TOKEN=optional-bearer-token
```

It can also be configured in `LatentSlateData/engine.json`:

```json
{
  "enabled": true,
  "base_url": "http://127.0.0.1:8765",
  "api_key": null,
  "catalog_timeout_ms": 800
}
```

The same protocol is used for localhost, a LAN machine, and a remote/Vast.ai
instance. LatentSlate sends media as multipart HTTP uploads and downloads the
resulting artifact over HTTP; it never assumes a shared filesystem. Remote
connections should use a secure tunnel or HTTPS reverse proxy, especially when a
bearer token is configured.

A successful live catalog is cached in `LatentSlateData/engine_catalog.json`.
When the Engine is offline, the cached schemas keep projects and generation forms
inspectable, but generation remains unavailable until a compatible Engine can be
reached.

### Engine Catalog Ownership

Engine tools use stable UUIDs and stable input keys. Labels and descriptions may
change without changing those identities. Every tool publishes a schema revision
and hash, and every submitted job includes both. A stale request is rejected with
an explicit `schema_mismatch` rather than being silently reinterpreted.

Engine-derived tools are read-only in LatentSlate. The existing **AI Providers**
editor lists local provider JSON files, not dynamic Engine catalog entries. Change
an Engine tool schema in the Engine repository; the next catalog refresh becomes
the single source of truth for its UI.

Project-level schema snapshots and the reconciliation screen for older Engine
schemas are not implemented yet. The revision/hash contract and stable IDs are in
place so that feature can be added conservatively. Until then, breaking Engine
schema changes may require manually repairing affected generative configs.

### Current Engine Tools

The built-in catalog covers H3 and LTX video, Wan video, and Klein 4B/9B image
families. Data-defined variants become normal catalog tools only when their
selected resources and runtime contracts validate. This includes the locally
proven Klein 4B Comfy-native stored-FP8 text-to-image and one-to-three-reference
image-edit paths, plus the staged native Wan 2.2 14B I2V recipe when all required
Comfy-aligned components are present.

H3 and LTX complete BF16 repositories are pinned and validated before loading;
their full target-hardware output acceptance remains part of hands-on testing.
The Engine owns model loading, stored-precision contracts, optimization profiles,
residency, and model-family eviction. LatentSlate consumes the same catalog,
schema, queue, upload, and artifact contract for every image/video variant.

## ComfyUI Setup

1. Start ComfyUI and confirm it responds at `http://127.0.0.1:8188`.
2. Build and test the workflow inside ComfyUI first.
3. Export the workflow as **API JSON**.
4. In LatentSlate, open `Settings > AI Providers...`.
5. Add a `ComfyUI Workflow` provider.
6. Use the Provider Builder to pick the workflow JSON.
7. Select the output node and output type.
8. Expose only the inputs that should appear in the editor UI.
9. Save the provider.

The builder writes one provider JSON file under `LatentSlateData/providers/`.
`LatentSlateData/` is created beside the running executable unless
`LATENTSLATE_HOME` points at an explicit app data folder. The app also creates an
empty `LatentSlateData/workflows/` folder for users who want workflow JSON files
kept beside the rest of the app data.

## Provider Entries

A provider entry stores:

- `id`: stable UUID referenced by generative assets
- `name`: display name
- `description`: optional multi-line guidance for humans and agents choosing a provider
- `output_type`: `image`, `video`, or `audio`
- `workflow_kind`: UX intent such as T2I, I2V, V2V, first/last-frame video, or video-to-bridge
- `timeline_bridge`: optional settings for `video_to_bridge` providers
- `inputs`: editor-visible schema fields
- `connection`: adapter-specific execution data

Cloud adapters store `connection.api_key` directly in provider JSON. ComfyUI
providers store their manifest bindings in `connection.manifest`. Engine tools are
created in memory from the live or cached catalog and carry the Engine URL, tool
identity, availability, and schema revision/hash.

Do not change provider or Engine tool UUIDs casually. Existing generative assets
store provider IDs in their `config.json`.

## ComfyUI Manifests

The manifest is the bridge between a full ComfyUI graph and a clean editor form.
Current ComfyUI bindings use:

- workflow node ID
- input key
- class type as a stale-binding guard
- optional title/tag metadata for display and diagnosis

Minimal shape:

```json
{
  "schema_version": 1,
  "adapter_type": "comfy_ui",
  "name": "SDXL Simple",
  "description": "Text-to-image workflow for generating still keyframes.",
  "output_type": "image",
  "workflow": {
    "workflow_path": "workflows/sdxl_simple_example_API.json",
    "workflow_hash": null
  },
  "inputs": [
    {
      "name": "prompt",
      "label": "Prompt",
      "input_type": "text",
      "required": true,
      "ui": { "multiline": true, "group": "Prompt" },
      "bind": {
        "selector": {
          "node_id": "6",
          "class_type": "CLIPTextEncode",
          "input_key": "text"
        }
      }
    }
  ],
  "output": {
    "selector": {
      "node_id": "53",
      "class_type": "PreviewImage",
      "input_key": "images"
    },
    "index": 0
  }
}
```

At runtime, the adapter scans the selected output node's file arrays and chooses
the first file whose extension matches the provider output type.

## Input Types And Roles

LatentSlate currently renders:

- `text`
- `number`
- `integer`
- `boolean`
- `enum`/Engine `choice`
- `image`
- `video`
- `audio`

The Engine protocol reserves a `resource` type for future model, LoRA, and style
catalogs. LatentSlate conservatively skips tools that require it until a native
resource picker exists.

Inputs can declare semantic roles. Width, height, and seed roles support existing
setup and batching behavior. I2V providers should mark their source image as
`start_image`; first/last-frame video providers should use `start_image` and
`end_image`. Video providers can additionally mark `duration_seconds`, `fps`, or
`frame_count`; LatentSlate syncs those fields from the generative video's target
timing before generation.

Timeline bridge video providers set `workflow_kind: "video_to_bridge"` and expose
roles for `width`, `height`, `seed`, `left_video`, `right_video`, `fps`,
`left_replace_frames`, `right_replace_frames`, and `edge_blend_frames`.

Media inputs can use project asset references and timeline-context suggestions.
For the Agent API, `inputs.<provider_field>` is canonical:
`{ "type": "asset_ref", "asset_id": "...", "pinned": true }`.
`reference_slots` remain compatibility aliases when the slot matches a provider
field or semantic media role.

## Drift And Compatibility

### Engine

The current catalog revision/hash is checked on every job. Refresh/restart
LatentSlate after changing an Engine schema. Safe project reconciliation is a
future UI; no fuzzy label matching or automatic destructive migration is
performed today.

### ComfyUI

Workflow edits can change node IDs or input keys. If generation fails with
missing node/input errors:

1. Open the provider in the builder.
2. Re-select the output node.
3. Re-expose or repair changed inputs.
4. Save the provider again.

Automatic Comfy workflow drift repair is not implemented.

## Troubleshooting

- **Engine tools do not appear:** start the Engine, check `LATENTSLATE_ENGINE_URL` or `LatentSlateData/engine.json`, then reload providers or restart LatentSlate.
- **Engine tool says unavailable:** install the required Engine bundle/dependencies; the V0 H3 tools require the Engine `h3` extra and `h3-basic` bundle.
- **Engine schema mismatch:** refresh the catalog by restarting/reloading, then inspect the affected generative config before changing stored values.
- **Missing inputs:** fill required fields in the Attributes panel or asset/provider editor.
- **Workflow missing node_id:** the Comfy manifest references a node that no longer exists; re-save through Provider Builder.
- **ComfyUI rejected prompt:** base URL is wrong, ComfyUI is offline, or the workflow failed validation.
- **Timed out waiting for ComfyUI output:** the workflow is still running, stalled, cached without a matching file, or produces an unexpected output type.

## Example Workflows

Tracked Comfy examples live in [../workflows](../workflows). Personal workflows
are intentionally ignored by default unless they are sanitized and useful to
contributors.
