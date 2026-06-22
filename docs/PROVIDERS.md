# Providers

LatentSlate is built around user-owned providers. ComfyUI is the primary open-source path today.

## Current Adapter Status

| Adapter | Status | Notes |
|---|---|---|
| ComfyUI | Implemented | Local API workflow JSON plus manifest bindings. Supports image/video/audio output detection by file extension. |
| OpenAI image | Experimental | Uses app-managed credential ID. |
| xAI image | Experimental | Uses app-managed credential ID. |
| xAI Grok video | Experimental | Submits/polls/downloads video results through xAI API. |
| Custom HTTP | Not implemented | Data model exists; runtime returns a planned/not-implemented error. |
| fal.ai / Replicate / Veo | Not implemented | Future adapter work. |

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

The builder writes:

- a provider JSON file under `LatentSlateData/providers/`
- a manifest JSON file under `LatentSlateData/provider-manifests/`

`LatentSlateData/` is created beside the running executable unless
`LATENTSLATE_HOME` points at an explicit app data folder. Older provider files
under `.latentslate/providers/` are copied into an empty app data folder for
development compatibility. Credentials are not automatically copied.

## Provider Entries

A provider entry stores:

- `id`: stable UUID referenced by generative assets
- `name`: display name
- `description`: optional multi-line guidance for humans and agents choosing a provider
- `output_type`: `image`, `video`, or `audio`
- `workflow_kind`: optional UX intent such as T2I, I2V, V2V, first/last-frame video
- `inputs`: editor-visible schema fields; each input can include an optional multi-line `description`
- `connection`: adapter-specific connection data

Do not change provider IDs casually. Existing generative assets store provider IDs in their `config.json`.

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
  "adapter_type": "comfyui",
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
      "description": "Prompt text sent to the positive CLIP encoder.",
      "input_type": "text",
      "required": true,
      "ui": { "multiline": true, "group": "Prompt" },
      "bind": {
        "selector": {
          "node_id": "6",
          "class_type": "CLIPTextEncode",
          "input_key": "text",
          "title": "CLIP Text Encode (Prompt)"
        }
      }
    }
  ],
  "output": {
    "selector": {
      "node_id": "53",
      "class_type": "PreviewImage",
      "input_key": "images",
      "title": "Preview Image"
    },
    "index": 0
  }
}
```

The output `input_key` is retained for compatibility. Normal users should not need to know ComfyUI history keys. At runtime, the adapter scans the selected output node's file arrays and chooses the first file whose extension matches the provider output type.

## Input Types

Supported schema input types:

- `text`
- `number`
- `integer`
- `boolean`
- `enum`
- `image`
- `video`
- `audio`

Media inputs can use project asset references and timeline-context suggestions,
but compatibility still depends on the provider workflow and manifest binding.
For the Agent API, `inputs.<provider_field>` is canonical for media parameters:
`{ "type": "asset_ref", "asset_id": "...", "pinned": true }`.
`reference_slots` are accepted as compatibility aliases when the slot name
matches the provider field or a semantic media slot such as `image`,
`start_image`, `end_image`, `video`, or `audio`.

## Workflow Drift

ComfyUI workflow edits can change node IDs or input keys. If generation fails with missing node/input errors:

1. Open the provider in the builder.
2. Re-select the output node.
3. Re-expose or repair changed inputs.
4. Save the provider again.

Automatic drift detection is a roadmap item, not current behavior.

## Troubleshooting

- `Missing inputs`: fill required fields in the Attributes panel or asset/provider editor. For Agent API media fields, set `patch.inputs.<field>` to an `asset_ref`; matching `reference_slots` are compatibility aliases only.
- `Workflow missing node_id`: the manifest references a node that no longer exists. Re-save through Provider Builder.
- `ComfyUI rejected prompt`: base URL is wrong, ComfyUI is offline, or the workflow failed validation.
- `Timed out waiting for ComfyUI ... output`: the workflow is still running, stalled, cached without a matching file, or producing an unexpected output type.
- `ComfyUI history did not include ... outputs`: selected output node or provider output type does not match the files ComfyUI produced.

## Example Workflows

Tracked examples live in [../workflows](../workflows). Personal workflows are intentionally ignored by default unless they are sanitized and useful to contributors.
