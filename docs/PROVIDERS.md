# Providers

LatentSlate is built around user-owned generation backends. ComfyUI remains the
bring-your-own workflow path. LatentSlate Engine publishes eight built-in native
image/video tools plus enabled user recipes.

Both paths normalize into the same provider-facing model inside LatentSlate:
output type, creative workflow kind, semantic inputs, progress, and generated
artifacts. Their source of truth is intentionally different: users author ComfyUI
providers, while the Engine publishes its own tool catalog.

## Current Adapter Status

| Adapter | Status | Notes |
|---|---|---|
| LatentSlate Engine | Implemented | Discovers versioned tools from `/v1/catalog`, uploads media, submits/polls jobs, and downloads outputs over HTTP. |
| ComfyUI | Implemented | API workflow JSON plus embedded manifest bindings. Supports image/video/audio output detection by file extension. |
| OpenAI image | Implemented | GPT Image 2.5 Flare and Sunburst, with T2I and reference-image editing templates. Stores `connection.api_key` in provider JSON. |
| xAI image | Experimental | Stores `connection.api_key` in provider JSON. |
| xAI Grok video | Experimental | Submits/polls/downloads video results through xAI API. |
| Custom HTTP | Not implemented | Data model exists; runtime returns a planned/not-implemented error. |
| fal.ai / Replicate / Veo | Not implemented | Future adapter work. |

## OpenAI Image 2.5

Add **OpenAI Image 2.5 T2I** or **OpenAI Image 2.5 I2I** in AI Providers.
Both offer Flare and Sunburst in the Model field; T2I defaults to Flare and I2I
to Sunburst. Quality includes `auto`, `low`, `medium`, `high`, `xhigh`, and `max`.
I2I requires a reference image. Transparent output requires PNG or WebP.
The adapter uses `/v1/images/generations` for T2I and multipart
`/v1/images/edits` for I2I, then decodes the returned base64 image.
Existing saved provider definitions retain their settings; add the updated template
to get these fields. See the [official image API guide](https://developers.openai.com/api/docs/guides/image-generation).

## Chat Agent Providers

In AI Providers, choose **OpenAI Agent** for OpenAI, or **OpenAI-compatible Agent**
for a local/custom endpoint. Both retain a user-chosen name and model. Save and
Test the agent, then select it in Chat. Definitions live in
`LatentSlateData/providers/agents/<uuid>.json`; they never enter generation
provider selectors or queues. API keys are stored in that local JSON, like
existing cloud generation provider keys.

OpenAI offers **ChatGPT subscription** browser sign-in and **OpenAI API key**
authentication. Both use Responses, with fixed OpenAI endpoints and separate
subscription/API billing; there is no automatic fallback between them. Browser
sign-in uses OAuth PKCE and a loopback callback on port 1455 (finish other pending
Codex sign-ins if that port is busy). LatentSlate owns and refreshes its tokens;
it does not import the Codex app/CLI login. On Windows, tokens are DPAPI-encrypted
in `providers/agents/accounts/<uuid>.bin`. Sign out clears this local login.
Subscription credential storage currently requires Windows; other platforms can
use API keys. This direct Codex Responses integration does not launch a Codex
runtime or give the agent additional shell/filesystem tools.

OpenAI agents save a **Thinking strength** per agent and send it as Responses
`reasoning.effort`. **Model default** leaves the parameter unset. Refresh models
to use the subscription catalog's supported levels and default; connections that
omit this metadata offer manual levels whose availability depends on the model.
Changing the model or connection resets the choice to Model default.

Chat shows a context wheel below the composer. Its tooltip reports the latest request's input + output tokens (including cached input and reasoning), not cumulative billing usage or unsent text. Missing usage or capacity is shown as unknown.

OpenAI API-key and ChatGPT connections use native Responses compaction at 200,000 tokens, with a 272,000-token application context budget (or a smaller advertised model window). The composer stays editable while sending is disabled. Native encrypted compaction items replace earlier wire context; the visible transcript remains. After compaction, occupancy is unknown until the next usage report, since OAuth does not expose a usable standalone token-count endpoint.

For llama.cpp, the configured per-slot context comes from the loaded model's props or router preset. Where supported, prompt counting runs before each request, including tool follow-ups, without auto-loading a cold model. Sending stops when the prompt cannot leave 4,096 tokens for a reply, or the server reports context overflow; start a new chat to continue. No local summarization or transcript-recall tool is enabled.

Compatible agents accept a base URL including `/v1` and an optional Bearer key;
a blank key sends no Authorization header. New entries default to **Responses**.
Existing entries without an API-format setting retain **Chat Completions**, which
is also selectable explicitly. Requests replay full history (`store: false` for
Responses), including function-call results and provider-specific reasoning
items. Responses and Chat Completions share the editor tool execution loop and
UI events, with separate wire-format implementations.

**Refresh models** loads available model IDs. Reported image capabilities and
loaded llama.cpp model video capabilities set read-only controls. Missing
metadata leaves manual controls available. llama.cpp `/props` is queried only
for a selected model already reported as loaded, to avoid loading other models.
OpenAI native video is always disabled. Compatible Responses also disables native
video: llama.cpp's Responses adapter currently supports text, images and function
tools but rejects `input_video`. Use **Chat Completions** for native video.

Enable image/video understanding only when the endpoint supports those inputs.
Image understanding offers rendered frames and contact sheets. Video understanding
currently requires llama.cpp's native `input_video: {data: <raw base64>}` content
form and accepts whole project video assets or generated versions; it does not
replace video with extracted images. Media attachments are limited to 32 MiB.
Chat permits 12 tool rounds and four visual inspections per user turn. Stop ends
the request without undoing completed edits. Project-document changes are saved
only by `save_project`; generation configuration/version sidecars retain their
normal immediate persistence. Chat history is cleared on New Chat or project change.

## Releasing Provider Resources

The top-right `DUMP` action asks every configured backend that supports resource
release to unload cached models and free RAM/VRAM. Shared backends are deduplicated,
so multiple provider entries pointing to one ComfyUI or LatentSlate Engine instance
produce one request to that instance. Unsupported cloud providers are left alone,
and failures are reported per backend without hiding successful releases elsewhere.

Current adapters use these native contracts:

- ComfyUI: `POST /free` with `unload_models` and `free_memory` enabled.
- LatentSlate Engine: authenticated `DELETE /v1/runtime`, which requires an idle
  Engine and leaves the service process running.

The same operation is available through `POST /agent/v1/command` with
`{"type":"release_provider_resources"}` and through the global UI registry as the
`DUMP` resource action. LatentSlate disables the UI action while its local
generation queue is active.

## LatentSlate Engine Setup

Image inputs can declare `image_dimensions: "match_output_canvas"`. LTX image-to-video
and first/last-frame tools publish this requirement at schema revision 3. LatentSlate
checks still-image dimensions during preflight and checks actual materialized images
(including extracted video frames) before submission. Sources remain unchanged;
choose a matching canvas or prepare a matching source. Missing constraints preserve
existing behavior for ComfyUI, Klein, and Wan.

`workflow_kind: "reference_to_video"` is distinct from video-to-video and bridge
workflows. Reference image sizing is independent of the output canvas unless the
catalog explicitly declares otherwise. An optional audio input may declare
`paired_video_input: "video_input_key"`; LatentSlate then offers a soundtrack checkbox
and source picker beneath that video. The default preserves both streams through video
sampling. A separate source override remains paired with the declared video but retains
its own sampling; an ordinary audio input remains independent even when using the same
file. Video-sourced independent/override audio is prepared as an audio-only artifact.

H3 reference-to-video exposes mixed image/video/audio slots. Its submitted notation
uses `<Picture N>` and `<Video N>` for occupied slots in order; `<Audio N>` counts
enabled video soundtracks in video order, then occupied standalone audio slots.
Removing earlier references can change these effective numbers. Literal provider tokens
remain untouched. Explicit authored `@{Input label}` references instead bind to a stable
provider ID and input key and resolve only after submission media are prepared. Optional
`prompt_reference_token` templates (for example `<Audio {index}>`) supply effective
1-based numbers for occupied fields sharing the exact template, in catalog declaration
order; a template without `{index}` stays literal (for example Qwen's fixed `Picture 3`).
Only catalog-declared notation is offered; model names never imply support. Missing
sources or incompatible recipe identities block submission and require a source,
explicit reassignment, or removal. Source changes retain the authored input identity.

The shared prompt field offers `@` autocomplete with fuzzy input/source matching,
readable highlighted mentions, and an inspectable prompt-to-send preview. Exact
`@{input_name}` or `@{Input label}` matches bind on explicit chat/API writes and human
edits. Legacy literal prompts are not parsed on project load. Existing registrations
survive text undo and recipe changes; copy/paste into a different prompt is a new write
against that prompt's recipe. Submitted snapshots retain both authored and resolved text.

User tools may fix either canvas dimension and the request duration in catalog
metadata. These values participate in preflight and output prediction without
creating hidden project inputs. Attributes and Asset Lab support ordered numeric
lists such as adapter strengths; unsupported collection shapes skip the whole tool
with a diagnostic.

LatentSlate treats each Engine as a backend in **AI Providers**. Add one or more
Engine connections from the Add Provider dropdown, then inspect that backend's
catalog on the right. When an Engine is reachable, its tools appear automatically
in provider pickers and generation forms. There is no provider JSON to export,
bind, or repair for Engine tools.

Run the Engine with its model files and Python/CUDA dependencies configured.
Its service entry point is `python -m latentslate_engine.service --host 127.0.0.1 --port 8765`
with `src` on `PYTHONPATH`. Use the local Process Manager when working in the
managed development stack.

`LATENTSLATE_ENGINE_HOME` selects the Engine data root (default:
`LatentSlateEngineData` in the Engine checkout). The current service uses fixed
family model paths under `models/`, with optional `LATENTSLATE_KLEIN9B_VAE` and
`LATENTSLATE_WAN_MODEL_ROOT` overrides. HTTP uploads and outputs live under
`runtime/http/`. Model selection and inference policy belong to the Engine;
use Engine Recipe Studio to author and enable user recipes. Check `/v1/health`
and `/v1/catalog` for service health and per-tool availability.

The machine-level connection can be changed with environment variables:

```text
LATENTSLATE_ENGINE_URL=http://127.0.0.1:8765
LATENTSLATE_ENGINE_TOKEN=optional-bearer-token
```

It can also be configured in `LatentSlateData/engine.json`. Multiple backends are
supported:

```json
{
  "connections": [
    {
      "id": "6c617465-6e74-736c-6174-650000000001",
      "name": "LatentSlate Engine",
      "enabled": true,
      "base_url": "http://127.0.0.1:8765",
      "api_key": null,
      "catalog_timeout_ms": 800
    }
  ]
}
```

A legacy singleton `engine.json` object still loads as one backend. Environment
variables overlay the default/first connection. Additional backends can be added
from AI Providers.

The same protocol is used for localhost, a LAN machine, and a remote/Vast.ai
instance. LatentSlate sends media as multipart HTTP uploads and downloads the
resulting artifact over HTTP; it never assumes a shared filesystem. Remote
connections should use a secure tunnel or HTTPS reverse proxy, especially when a
bearer token is configured.

A successful live catalog is cached in `LatentSlateData/engine_catalog.json` for
the default backend, or `LatentSlateData/engine_catalogs/<connection-id>.json`
for additional backends. When an Engine is offline, the cached schemas keep
projects and generation forms inspectable, but generation remains unavailable
until a compatible Engine can be reached.

### Engine Catalog Ownership

Engine tools use stable UUIDs and stable input keys. Labels and descriptions may
change without changing those identities. Every tool publishes a schema revision
and hash, and every submitted job includes both. A stale request is rejected with
an explicit `schema_mismatch` rather than being silently reinterpreted.

Engine-derived tools are read-only in LatentSlate. **AI Providers** lists Engine
backends alongside local provider JSON files. Selecting an Engine backend edits
the connection and shows its discovered catalog; it does not create editable
provider JSON for those tools. Edit user recipes in Engine Recipe Studio; normal
catalog refresh is the sole freshness mechanism in the desktop. Hidden-only
recipe changes preserve current inputs. Schema changes retire removed inputs and
retain shared creative-role values for preflight to check. Disabling a recipe
makes new generation unavailable without substituting another provider.

User-tool jobs submit the exact catalog recipe identity. A stale 409 fails the
attempt and refreshes providers; submit again after reviewing the refreshed inputs.
Saved generation records keep the Engine's accepted tool/schema/recipe identity,
including after later recipe edits and project reopen.

Project-level schema snapshots and the reconciliation screen for older Engine
schemas are not implemented yet. The revision/hash contract and stable IDs are in
place so that feature can be added conservatively. Until then, breaking Engine
schema changes may require manually repairing affected generative configs.

### Current Engine Tools

The built-in catalog contains eight tools:

| Family | Operations | Output timing |
|---|---|---|
| LTX 2.3 | Text to video, image to video, first/last-frame video | 30 fps; nominal requests of 1-10 seconds in 0.5-second increments; native frame mapping; synchronized audio |
| FLUX.2 Klein 9B | Text to image, two-image generation | Still image |
| Wan 2.2 14B Turbo | Text to video, image to video, first/last-frame video | 16 fps; 1-5 seconds in 0.25-second increments |

Klein two-image generation requires both Image 1 and Image 2. All eight tools
publish required width/height inputs, seed, and their canvas constraints.
LTX image-conditioned operations additionally require source dimensions to
match the output canvas. Klein and Wan accept independently sized references.

LTX's duration input is a model request, not an exact media length. Catalog
`timing.duration_seconds.output_frame_counts` declares all 19 legal requests:
1 second produces 25 frames (0.833 seconds), and 5 produces 145 (4.833 seconds).
The controls show the next request and predicted Output Frames/Output Duration.
Completed assets retain their actual media extent when editing the next request
or switching providers. Wan uses the ordinary duration-times-FPS mapping.

The Engine owns model loading, fixed product policies, inference, and GPU worker
lifecycle. LatentSlate owns project media, input binding, generation controls,
queue presentation, and version insertion. ComfyUI remains a separate adapter
for user-authored API workflows and manifests.

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

- `id`: stable UUID referenced by generative assets. Each Add Provider action mints a new id, including a second OpenAI or xAI account. Display `name` is not unique and is not used as a key.
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
      "input_type": { "type": "text" },
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

Inputs can declare semantic roles. Width, height, and seed roles support existing
setup and batching behavior. I2V providers should mark their source image as
`start_image`; first/last-frame video providers should use `start_image` and
`end_image`. Video providers can additionally mark `duration_seconds`, `fps`, or
`frame_count`. These fields describe the next request. For generated assets they
remain independent of actual media timing; hollow assets use predicted output
extent, with ordinary target-timing sync for providers without a duration map.

Engine image and video tools use integer `width` and `height` roles rather than a
single size preset. New generation requests default to the project canvas size;
an explicit config (including continuation dimensions) wins. The current eight
tools use a legal project-derived canvas when no explicit pair is stored.
The shared canvas picker keeps the submitted width/height pair as the resolved
output while offering `Aspect + MP`, `Exact dimensions`, and `Project scale`
input modes. Its output readout reports the effective resolution, megapixels,
aspect, and provider pixel grid so grid-adjusted results are never presented as
the literal requested target. `Project scale` rounds up to a legal output that
meets the scaled project's pixel area; scale choices beyond the provider's hard
canvas limits are disabled.
Legacy `size: "WIDTHxHEIGHT"` configs are migrated for a refreshed Engine tool,
while stale `size` is omitted from the submitted request.

Timeline bridge video providers set `workflow_kind: "video_to_bridge"` and expose
roles for `width`, `height`, `seed`, `left_video`, `right_video`, `fps`,
`left_replace_frames`, `right_replace_frames`, and `edge_blend_frames`.

Media inputs use canonical `media_bindings` on the generative config: source
(Follow Timeline / Timeline Clip / Project Asset / Frozen Input), sample, and
Strict coverage. Legacy `{ "type": "asset_ref", "asset_id": "...", "pinned": true }`
values still load and migrate. `reference_slots` remain compatibility aliases.
`get_generative_config` returns the live config plus a `media_bindings` inspection
map (current resolution summary, not materialized files).

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
- **Engine tool says unavailable:** inspect its catalog reason and verify the current family model files and runtime dependencies on that Engine host.
- **Engine schema mismatch:** refresh the catalog by restarting/reloading, then inspect the affected generative config before changing stored values.
- **Missing inputs:** fill required fields in the Attributes panel or asset/provider editor.
- **Workflow missing node_id:** the Comfy manifest references a node that no longer exists; re-save through Provider Builder.
- **ComfyUI rejected prompt:** base URL is wrong, ComfyUI is offline, or the workflow failed validation.
- **Timed out waiting for ComfyUI output:** the workflow is still running, stalled, cached without a matching file, or produces an unexpected output type.

## Example Workflows

No runnable Comfy workflow examples are bundled in this checkout. Export a
working API workflow from your own ComfyUI installation; personal workflows are
ignored by default.
