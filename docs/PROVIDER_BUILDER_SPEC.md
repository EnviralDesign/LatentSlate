# Provider Builder UI (Draft Spec)

This spec defines the first-pass UX for turning a working workflow/API into a
clean provider entry without hand-editing JSON. The in-app builder currently
implements the core ComfyUI flow (workflow picker + node browser + inputs).

The builder supports multiple adapter types:
- **ComfyUI** (primary OSS path)
- **Custom HTTP** (generic API)
- **Hosted adapters** (fal.ai, Veo, etc.) as separate templates later

## Entry Point

- **Settings > AI Providers...** -> `Build` button.
- The button launches the Provider Builder wizard.
- If a provider is selected, the builder opens in **Edit** mode and preloads
  the existing manifest/workflow (when available). Otherwise it starts fresh.

## Step 1: Choose Adapter

**Goal:** Pick the integration style.

Options:
- ComfyUI workflow (file-based)
- Custom HTTP API (endpoint-based)
- Future: fal.ai, Replicate, etc.

## Step 2A: ComfyUI Workflow Picker

**Goal:** Select a ComfyUI API JSON file.

UI:
- File picker
- Recent workflows list
- Inline validation (must be API JSON)
- Read-only summary: node count, output candidates

## Step 2B: Custom HTTP Setup

**Goal:** Define endpoint basics.

Fields:
- Base URL, path, method
- Headers (supports env placeholders)
- Request format (json/form)
- Response path for output

## Step 3: Output Selection

**Goal:** Define provider output before any inputs are exposed.

Layout (current build):
- **Left column:** Node browser (search + list)
- **Middle column:** Node inspector + input/output actions
- **Right column:** Provider settings + exposed inputs/output summary
- **Tabs:** `Output` comes first. `Inputs` remains disabled until a workflow output node is selected.

Left panel:
- Search bar (class type, title, input key)
- Filters: class type, category, has inputs, has outputs
- Node list with title + class type

Right panel:
- Selected node inspector
- Inputs list (key, type guess, current value)
- Outputs list (for output selection)

Actions:
- **Use as Output** button for saver/output nodes

## Step 4: Exposed Inputs Editor

**Goal:** Curate the provider UI.

Once an output node is configured, the `Inputs` tab unlocks:

- **Expose Input** button next to each input key

For each exposed input:
- Label
- Required toggle
- Default value
- UI hints: min/max/step, multiline, placeholder, unit
- Advanced toggle
- Group name
- Binding preview (selector fields)
- The output_type picker remains in Provider Settings (image/video/audio).
- ComfyUI history output keys stay hidden from normal UX. The builder auto-fills
  the manifest key for compatibility, and runtime detection scans all file arrays
  on the selected output node for a file matching the provider output type.

## Step 6: Review & Save

**Goal:** Generate the provider entry + manifest.

Outputs:
- Provider entry JSON (global providers folder)
- Manifest JSON (next to workflow file)

Review panel:
- Provider name, adapter type, output type
- List of exposed inputs
- Warnings for ambiguous bindings

## Node Linking UX (ComfyUI)

Bindings do not require manual node ID editing. Use dropdowns/search:

- **Node dropdown** (filtered by class type/title; stores workflow node ID)
- **Input dropdown** (keys for the node)
- **Tag field** (optional metadata)
- **Stale binding warning** if the saved node ID is missing or no longer has the selected input

## Workflow Drift Handling

When a workflow changes:
- Recompute workflow hash
- If changed, mark provider **Needs Rebind**
- Open builder with previous selections preloaded
- Show differences (missing nodes, renamed keys)

## JSON Output

The builder writes:

1. A provider entry (global)
2. A manifest file matching the adapter type

See `docs/PROVIDER_MANIFEST_SCHEMA.md` for format details.
