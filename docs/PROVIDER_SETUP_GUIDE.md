# Provider Setup Guide (MVP + Planned)

This guide covers the current MVP setup and the planned Provider Builder flow.
ComfyUI is the primary open-source path today; other adapter styles are planned.

## Provider Types (Roadmap)

- **ComfyUI (current MVP)**: Local workflows (API JSON).
- **Custom HTTP (planned)**: Generic REST APIs with input mapping.
- **Hosted Adapters (planned)**: fal.ai, Replicate, Veo, etc.

## Quick Start (Current MVP - ComfyUI JSON)

1. Start ComfyUI and confirm it responds at `http://127.0.0.1:8188`.
2. Export an API workflow JSON from ComfyUI.
3. Put the JSON somewhere stable (recommended: `workflows/` in this repo).
4. In the app, open `Settings > AI Providers...` and click `New`.
5. Edit the JSON to point at your `base_url` and `workflow_path`, then `Save`.
6. Select that provider on a generative image clip and click **Generate**.

## Quick Start (Builder - ComfyUI)

The Provider Builder UI is now available for ComfyUI workflows:

1. `Settings > AI Providers...` -> **Build**.
2. Pick a ComfyUI API workflow JSON file.
3. Use search + dropdowns to select node inputs to expose.
4. Choose a single saver/output node and output type (image/video/audio).
5. Save: the builder writes a manifest and creates the provider entry.

No manual node ID editing is required. The builder records the selected ComfyUI node ID under the hood.

## Where Provider Files Live

Provider entries are stored globally (not per project) as JSON files:

```
%LOCALAPPDATA%\NLA-AI-VideoCreator\providers\
```

Each provider file is named after its UUID: `<provider-id>.json`.

Workflow manifests live alongside the workflow JSON:

```
workflows/
├── my_workflow_API.json
└── my_workflow_manifest.json
```

## Creating a Provider Entry

Open the Providers dialog:

- `Settings > AI Providers...`
- Click `New` to create a draft provider JSON (manual).
- Click **Build** to use the builder UI (recommended).

### Provider JSON Example (ComfyUI Image Gen - MVP)

```json
{
  "id": "d7c1f4a0-9db8-4d7e-a4e8-7b7e0c5a9c21",
  "name": "ComfyUI SDXL (Local)",
  "output_type": "image",
  "inputs": [
    { "name": "prompt", "label": "Prompt", "input_type": "text", "required": true },
    { "name": "negative_prompt", "label": "Negative Prompt", "input_type": "text" },
    { "name": "seed", "label": "Seed", "input_type": "integer" },
    { "name": "steps", "label": "Steps", "input_type": "integer", "default": 20 },
    { "name": "cfg", "label": "CFG", "input_type": "number", "default": 5.0 },
    { "name": "width", "label": "Width", "input_type": "integer", "default": 1024 },
    { "name": "height", "label": "Height", "input_type": "integer", "default": 1024 },
    { "name": "checkpoint", "label": "Checkpoint", "input_type": "text" },
    { "name": "sampler", "label": "Sampler", "input_type": "text" },
    { "name": "scheduler", "label": "Scheduler", "input_type": "text" },
    { "name": "start_step", "label": "Start Step", "input_type": "integer", "default": 0 }
  ],
  "connection": {
    "type": "comfy_ui",
    "base_url": "http://127.0.0.1:8188",
    "workflow_path": "workflows/sdxl_simple_example_API.json",
    "manifest_path": "workflows/sdxl_simple_example_manifest.json"
  }
}
```

### Field Notes

- `id`: Stable UUID for this provider. Keep it the same once assets depend on it.
- `output_type`: `image`, `video`, or `audio`. ComfyUI image workflows use `image`.
- `inputs`: Drives the Attributes panel UI. Required fields must be filled before Generate.
- `connection.type`: Use `comfy_ui` for the current MVP. Other adapters are planned.
- `workflow_path`: Optional. If omitted, the app uses the default
  `workflows/sdxl_simple_example_API.json`.
- `manifest_path`: Optional but recommended. When provided, the adapter binds
  inputs/outputs via node IDs captured from the workflow browser.

## ComfyUI Workflow Setup

The app expects a **ComfyUI API workflow JSON** (not a PNG or UI save).

Recommended flow:

1. Open ComfyUI.
2. Load `workflows/sdxl_simple_example_API.json`.
3. Make your edits (swap model, sampler, etc.).
4. Export as **API** JSON and save over your file.

This preserves the workflow structure and node IDs that provider bindings use.

### Input Mapping (Builder / Manifest)

The ComfyUI adapter now reads the **manifest** (if present) and binds inputs by
node ID and input key. Each exposed input maps to:

```
selector: { node_id, class_type, input_key, title?, tag? }
```

Selector matching behavior:

- `node_id + input_key` must match a node input.
- `class_type` is verified as a stale-binding guard.
- `title` and `tag` are retained as metadata.

If you don't provide a manifest (or omit `manifest_path` in the provider entry),
the adapter falls back to the legacy node-ID bindings in the SDXL example.

### Output Expectations

- With a manifest: the output selector identifies the output node. The manifest
  keeps an `input_key` for compatibility, but normal users do not need to know
  or edit it.
- At runtime the adapter checks the selected node's ComfyUI history and picks
  the first file whose extension matches the provider output type. This handles
  confusing ComfyUI cases where video saver nodes report mp4 files under a key
  named `images`.
- Without a manifest: the adapter uses the legacy SDXL image fallback on node
  `53` (PreviewImage) and then scans for the first matching output.
- Only the first matching output (or the `index` if specified) is used.

### Builder Binding (Current)

The builder lets you select workflow nodes visually, then stores the node ID under
the hood so users do not edit node IDs by hand. See
`docs/PROVIDER_MANIFEST_SCHEMA.md`. Tags are optional metadata.

## Using Your Provider in the App

1. Create a **Generative Image** asset.
2. Drag it onto a Video track.
3. Select the clip to open the Attributes panel.
4. Pick your provider from the dropdown.
5. Fill in inputs and click **Generate**.

## Pitfalls and Current Constraints

- **Asset inputs are not wired yet.** Image/video/audio inputs show a placeholder.
- **ComfyUI only (for now).** Other adapter types are planned.
- **Relative workflow paths** are resolved from the app working directory first,
  then from the executable directory. Use absolute paths if in doubt.
- **Provider ID changes** will break existing generative assets that reference it.
- **Manual JSON without a manifest** uses the legacy node ID bindings.
- **Manifest-based binding** requires valid node IDs; missing/deleted nodes will error.

## Troubleshooting

- "Missing inputs: ..." -> Required fields are not set in the Attributes panel.
- "Workflow missing node_id ..." -> The manifest references a node that no longer
  exists. Re-open the provider in the builder and expose that input/output again.
- "ComfyUI rejected prompt ..." -> Base URL is wrong or ComfyUI is not running.
- "Timed out waiting for ComfyUI image/video/audio output..." -> Workflow is
  still running, stalled, or produced no matching output before the timeout.
- "ComfyUI history did not include image/video/audio outputs." -> Ensure your
  provider output type matches the selected saver/output node.
