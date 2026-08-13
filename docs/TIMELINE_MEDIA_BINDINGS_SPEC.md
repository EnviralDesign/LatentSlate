# Timeline-Aware Media Input Bindings

**Status:** implementation specification
**Primary repository:** `EnviralDesign/LatentSlate`
**Baseline reviewed:** `main` at `a56b8e690f5c9545ca2639aa98a2c1b579798de6`
**Audience:** LatentSlate maintainers, local Codex agents, Rust contributors, provider/Engine integrators, and reviewers
**Authority:** this document is the source of truth for the feature described here until the implementation and the concise architecture docs absorb it

## 1. Executive Summary

LatentSlate needs one coherent way to describe how any generative provider input obtains image, video, or audio media from a project. The current implementation can point a provider field at an asset, optionally associate that reference with a timeline clip, mark it as pinned or unpinned, and extract the first or last frame of a video when an image is required. That is enough for simple image-to-video and first/last-frame workflows, but it does not scale cleanly to:

- timeline-aligned image references;
- a generated clip continuing directly from the final frame of the generated clip touching it on the same track;
- arbitrary video-frame sampling;
- audio or video slices aligned to a generative clip's global timeline span;
- sparse keyframes or multiple temporal conditions;
- live timeline-following inputs versus source-locked inputs versus byte-for-byte frozen inputs;
- comprehensible dependency visualization;
- durable generation provenance.

The feature must replace the overloaded idea of a single `pinned` boolean with a unified model composed of three independent questions:

1. **Source selection:** where does the media come from?
2. **Sampling:** what frame, range, or whole item is taken from that source?
3. **Stability:** may source resolution change, is only source identity locked, or are the exact materialized bytes frozen?

The user-facing stability states are:

- **Follow** — resolve a declared timeline query each time the input is inspected or a job is queued;
- **Lock Source** — retain a specific asset, clip, and generative version while continuing to calculate the requested frame or aligned range from the current target placement;
- **Freeze Input** — retain the exact PNG, video segment, or audio segment already materialized for the input.

Timeline auto-resolution must be deterministic and explainable. For frame inputs, the default precedence is:

1. an exact keyframe reference at the requested output frame;
2. a touching same-track predecessor or successor boundary, which enables chained generative clips;
3. a compatible clip covering the requested frame.

For range inputs, a compatible clip must cover the entire requested output span under the default strict policy. Partial coverage must fail before generation. Padding, holding, looping, and overlap-only behavior may be added later only as explicit opt-in policies.

LatentSlate must resolve project/timeline semantics and materialize ordinary concrete files before provider execution. ComfyUI, cloud providers, and LatentSlate Engine must not receive LatentSlate track IDs, clip IDs, or timeline queries. Generation jobs must store immutable concrete resolutions, while persistent project configuration must keep the live binding rules that produced them.

This specification deliberately preserves both major working styles:

- traditional asset-first configuration in the Assets pane, with explicit dropdown selection and no timeline placement required;
- timeline-first editing, where generative clips derive inputs from keyframes, adjacent clips, underlying video, audio, or other explicitly declared timeline relationships.

The initial UX should be explicit rather than magical. Drag-and-drop wiring may be considered later, but the first complete slice must let users configure, inspect, lock, freeze, and understand every dependency through normal controls.

## 2. Complete Decision Record

This section captures the full product context and all clarifications that led to this specification. Later sections define the normative behavior in implementation terms.

### 2.1 Baseline generation workflows

- **Text to video** has no media dependency. A generative video asset still has target duration/frame count, frame rate, resolution, prompt, and provider settings.
- **Image to video** has an image input that normally represents the first generated frame. The source must be selectable directly from the Assets pane even when the generative video has never been placed on the timeline.
- A timeline-oriented user may instead arrange storyboard/keyframe images on one video track and place generative videos on another track. The generative clip should be able to declare that its start image follows the image aligned with its first output frame.
- **First/last-frame video** naturally maps a start image to the first output frame and an end image to the last output frame. A generative video may visually bridge two image keyframes.
- Newer providers may accept audio, video, multiple images, multiple short videos, or sparse temporal keyframes. The architecture must not require a new bespoke pinning mode for every provider family.

### 2.2 Image clip display modes

LatentSlate currently allows an image clip instance to appear as either:

- a normal still clip stretched across a time range; or
- a keyframe-reference pin anchored at a single frame.

These modes describe timeline presentation and temporal eligibility, not different underlying media. In either mode the referenced asset is still one static image.

Normative consequence:

- an **explicit asset or explicit clip binding** refers to the static image regardless of display mode;
- a **Follow Timeline query** treats a Still image clip as covering its displayed interval and a Keyframe image clip as eligible only at its exact anchored frame.

### 2.3 Same-track generative continuation

A generated video clip A may already exist on a track. A new generative clip B may be snapped so that B starts exactly where A ends. B must be able to use A's final visible frame as its image-to-video start input even though the source is on the same track rather than on a track below.

This applies whether B has never been generated or is being regenerated. A must have a resolvable output file/version. The relationship must be visible and explainable so that automatic behavior never becomes invisible magic.

### 2.4 Locking and freezing

The earlier `pinned` concept conflates two useful operations. They must remain separate:

- **Lock Source:** preserve the chosen source identity, including an exact generative version where applicable, while still computing the sample/range from the target clip's current placement.
- **Freeze Input:** preserve the exact materialized file used as provider input.

A user may move a target clip after locking its audio/video source and expect a newly aligned slice from the same source. A frozen input must remain byte-for-byte stable regardless of later timeline edits, trim changes, version changes, or source deletion.

### 2.5 Strict coverage and no hidden invention

The default coverage policy is strict.

- A frame input with no compatible source at the requested frame is unresolved and blocks generation.
- An audio/video range input that does not cover the complete requested output span is unresolved and blocks generation.
- LatentSlate must not silently pad silence, hold an edge frame, loop media, trim to overlap, synthesize black, or otherwise invent a policy.
- Such behaviors may be added later only as explicit user-selected policies that are persisted and shown in preflight.

### 2.6 Raw source media, not timeline composite

Timeline bindings read raw clip media through the clip's trim and Crop/Stretch time mapping. They do not render the timeline composite.

The initial feature therefore excludes:

- transforms, opacity, blend/composite results, and lower-layer pixels;
- rendered effects;
- timeline audio mixing, track volume, clip gain, or effects;
- a flattened frame or mixed audio result from multiple clips.

A future explicit **Timeline Composite** source may be added, but it is a separate feature and must not be implied by ordinary clip bindings.

### 2.7 Asset identity and placements

Generative settings remain asset-level, matching ordinary NLE asset behavior. Multiple timeline placements refer to the same underlying generative asset and share its configuration and active generated version. Clip instances retain their own timing, trim, transform, opacity, label, and display attributes.

When independent generative settings or lineage are desired, the user should Duplicate/Fork the asset. This feature must not silently turn each placement into a separate generative asset.

### 2.8 Visualization and timeline density

- Full dependency connectors should appear only for the selected generative clip, avoiding permanent spaghetti.
- Compact dependency indicators may remain visible on clips so dependencies are discoverable.
- Selected relationships must show the actual source point/range, state, and reason the resolver chose that source.
- Timeline tracks should receive modest additional vertical breathing room to accommodate clearer thumbnails, dependency indicators, and relationship anchors. Exact dimensions are implementation tuning, not a new zoom model.

### 2.9 Initial authoring UX

The first implementation should favor explicit configuration:

- source dropdown/query controls;
- sample controls;
- Follow/Lock/Freeze controls;
- a visible “Resolved now” explanation;
- strict preflight diagnostics.

Complex drag-and-drop wiring, gesture-based assignment, and hidden auto-link creation are deferred. Automatic resolution is allowed only when the configured source is explicitly a Follow Timeline query and the result can be inspected.

### 2.10 Provider and Engine boundary

LatentSlate owns timeline interpretation, source selection, time mapping, range validation, extraction, retiming, caching, and freezing. Providers receive concrete media files plus ordinary scalar values.

The same project-side model must serve:

- LatentSlate Engine recipes;
- ComfyUI workflow providers;
- cloud APIs;
- future provider adapters.

The Engine may declare exact media roles and cardinality, but it must not need to understand LatentSlate timeline identities.

## 3. Current Implementation Baseline and Why It Must Change

This specification is intentionally grounded in the current Rust application rather than an abstract redesign.

### 3.1 Current persistent input representation

`src/state/generative.rs` currently stores provider values through `InputValue`:

```rust
pub enum InputValue {
    AssetRef {
        asset_id: Uuid,
        source_clip_id: Option<Uuid>,
        pinned: bool,
        frame_reference: Option<SourceFrameReference>,
    },
    GenerationRef {
        asset_id: Uuid,
        version: String,
        frame_reference: Option<SourceFrameReference>,
    },
    Literal {
        value: serde_json::Value,
    },
}
```

`SourceFrameReference` can only express First or Last. `pinned` is asked to mean both “retain this source” and, indirectly, “do not rerun the timeline proximity heuristic.” There is no persisted declaration of the intended timeline query, no arbitrary frame sample, no aligned range, and no exact frozen artifact.

`GenerativeConfig` currently stores:

- `provider_id`;
- `inputs`;
- `reference_slots`;
- batch settings;
- generation records;
- active version;
- Asset Lab graph.

`GenerationRecord` currently stores only an `inputs_snapshot`; it does not separately retain the configured live rule and the concrete derived frame/range used by a job.

### 3.2 Current automatic resolver

`src/core/generation.rs` currently:

- looks for a media value in `config.inputs`, `reference_slots`, or a semantic compatibility slot;
- reruns `best_timeline_asset_ref_for_input` for an unpinned asset reference;
- uses a weighted proximity score and track penalties;
- samples only first/last video boundaries when an image field uses a video;
- returns the full source path for ordinary video/audio fields;
- has a separate purpose-built materialization path for timeline bridge segments.

This means that:

- “Auto” is a hidden heuristic rather than a persisted rule;
- an underlying video cannot provide an arbitrary aligned frame;
- a 0–10 second audio clip used by a 5–10 second generation is sent as the complete source rather than as a 5-second slice;
- lock versus freeze cannot be expressed;
- the relationship cannot be faithfully visualized because the model does not preserve the query and resolution separately.

### 3.3 Existing primitives to preserve

The implementation should build on, not replace, several useful primitives:

- `Clip::source_time_for_local` and `source_time_at_global` already define trim plus Crop/Stretch mapping.
- `ClipImageMode` already cleanly separates Still and Keyframe presentation.
- video reference-frame extraction already exists.
- timeline bridge segment extraction already demonstrates FFmpeg-backed segment materialization and retiming.
- the timeline renderer already records clip geometry and owns an overlay painter spanning track content.
- the generation queue already snapshots inputs before asynchronous execution.
- generative assets already carry exact version labels and project-local output files.

The new feature should consolidate these capabilities behind one resolver/materializer rather than add more independent special cases.

## 4. Goals

The implementation is complete only when it satisfies all of the following goals.

1. One coherent binding model serves image, video, and audio provider inputs.
2. Asset-only configuration remains a first-class workflow.
3. Timeline-following behavior is explicit, persisted, deterministic, and inspectable.
4. Same-track touching generative continuation works without a special provider mode.
5. Image fields can sample exact arbitrary frames from video.
6. Audio/video fields can materialize globally aligned slices.
7. Clip trim and Crop/Stretch mapping are honored.
8. Full-span range coverage is strict by default.
9. Follow, Lock Source, and Freeze Input have distinct semantics and controls.
10. Queued jobs are immutable and reproducible.
11. Persistent live rules are not overwritten by one concrete generation result.
12. Generation records contain sufficient provenance to explain what media was used.
13. Selected timeline relationships are clearly visualized without permanent clutter.
14. Provider adapters receive ordinary concrete files and do not learn timeline semantics.
15. Existing projects and legacy `InputValue` references remain loadable.
16. The architecture has a clear extension path for sparse/multiple temporal conditions without forcing all current providers into one mega-operation.

## 5. Non-Goals

The first implementation must not expand into unrelated NLE or provider work.

- Do not render a timeline composite for ordinary media bindings.
- Do not implement effect graphs, opacity compositing, mixed audio, or transformed source frames.
- Do not convert every provider operation into one universal workflow kind.
- Do not require runtime model conversion or provider-side understanding of the timeline.
- Do not add silent padding, looping, edge holds, black frames, or overlap trimming.
- Do not add drag-and-drop wiring as a prerequisite.
- Do not make generative configuration placement-local.
- Do not redesign Asset Lab lineage beyond what is necessary to store and replay media bindings.
- Do not replace the dedicated seam-bridge creation UX in the first tranche; it may reuse shared materialization internals later.
- Do not add an audit ledger, cryptographic content-addressed project store, migration framework, or generalized dependency engine unless concrete implementation needs prove one is necessary.
- Do not solve Engine structured sparse-condition schemas in the same initial pull unless the UI/provider contract is ready; document and preserve the extension point instead.

## 6. Design Principles

### 6.1 Explicit configuration, explainable automation

Automation is acceptable only when the configuration explicitly says to follow the timeline. Every automatic result must be explainable in plain language and traceable visually.

### 6.2 Persist intent separately from resolution

The project stores what the user asked for. The job stores what that request resolved to at queue time. These are different objects and must never be conflated.

### 6.3 Frame-accurate timeline semantics

All equality and touching decisions must be based on normalized frame positions, not fragile raw floating-point equality.

### 6.4 Fail before provider execution

Missing sources, unsupported sample modes, partial strict coverage, unavailable generated versions, invalid frozen paths, and ambiguous context must be reported before a provider job is submitted.

### 6.5 Raw media fidelity

The project-side materializer should faithfully extract the requested raw frame/range. Model-specific resizing, alignment, normalization, and recipe preprocessing remain provider/Engine responsibilities.

### 6.6 Small reusable core, provider-specific schemas

The binding machinery is generic. Provider operation contracts remain exact and opinionated. Image-to-video still declares one image field; first/last-frame video still declares two ordered image fields; audio-conditioned video still declares audio; sparse conditioning requires a structured repeated field when supported.

### 6.7 Ordinary NLE identity rules

Assets own media/generation identity. Timeline clips are placements. Forking creates a new generative identity.

## 7. Terminology

| Term | Meaning |
|---|---|
| **Target asset** | The generative asset being configured or generated. |
| **Context clip** | The specific timeline placement that supplies global start/end timing for a generation invocation. |
| **Source asset** | The project asset whose media is read. |
| **Source clip** | A particular timeline placement of the source asset, carrying trim and time mapping. |
| **Binding** | The persisted source-selection, sampling, and coverage specification for one provider media field. |
| **Resolution** | The deterministic result of evaluating a binding in a project/context at a moment in time. |
| **Materialization** | Creating the concrete provider-facing file: PNG frame, MP4 segment, WAV segment, or an unchanged whole source. |
| **Follow** | A binding whose source is reselected from a declared timeline query. |
| **Lock Source** | A binding whose source clip/asset/version is fixed while sampling may remain aligned to the target context. |
| **Freeze Input** | A binding to exact retained materialized bytes. |
| **Output time** | Time relative to the target generative output. |
| **Global time** | Absolute time on the LatentSlate timeline. |
| **Source time** | Time inside the raw source media file. |
| **Coverage** | Whether a source clip is temporally eligible for a requested frame or full range. |
| **Touching** | Two clip boundaries that normalize to the same timeline frame boundary. |
| **Keyframe image clip** | An image placement displayed and eligible as a one-frame reference pin. |
| **Still image clip** | An image placement displayed and eligible across a timeline interval. |
| **Hollow generative asset** | A generative asset without a valid active output file/version. |

## 8. User Workflows

### 8.1 Text-to-video from the Assets pane

1. Create or select a generative video asset.
2. Choose a text-to-video provider.
3. Configure prompt, target frame count/duration, frame rate, resolution, and provider parameters.
4. Generate without placing the asset on the timeline.

No media bindings are required. The feature must not add irrelevant source controls to this workflow.

### 8.2 Explicit image-to-video from the Assets pane

1. Create/select a generative video asset.
2. Choose an image-to-video provider with a required start-image field.
3. Set Source to **Project Asset** and choose an image.
4. Sample defaults to **Whole static image** or **Source first frame** as appropriate.
5. Generate without timeline context.

This is the traditional software workflow and must remain stable even after timeline-following features are added.

### 8.3 Timeline storyboard image-to-video

1. Place storyboard images on a video track as Still or Keyframe image clips.
2. Place a generative video on another track at the intended span.
3. Configure its start-image field as **Follow Timeline**.
4. Choose Auto, Below, or a specific keyframe track.
5. Sample at the target output first frame.
6. Inspect “Resolved now” and the selected timeline connector.
7. Generate.

A keyframe image is eligible only on its anchor frame. A Still image is eligible anywhere within its clip interval.

### 8.4 First/last-frame video between two keyframes

1. Place a start keyframe at the target first frame.
2. Place an end keyframe at the target final visible frame.
3. Place a generative video spanning the intended output range.
4. Bind start image to output first frame and end image to output last frame.
5. Each field may independently Follow, Lock Source, or Freeze Input.
6. Generate only when both required fields resolve.

The provider operation remains first/last-frame video; the shared machinery merely fulfills the two media fields.

### 8.5 Same-track chained image-to-video

1. Generate clip A and place it on a video track.
2. Place clip B immediately after A and snap B's start boundary to A's end boundary.
3. Configure B's start image to Follow Timeline Auto or Same Track.
4. The resolver chooses A's final visible frame as B's start input.
5. The timeline displays a selected-only same-track connection from A's end boundary to B's start input marker.
6. The inspector states that the source was selected because it is the touching previous clip.

If A is hollow or its active output is missing, B is unresolved. If B is the same underlying generative asset as A, automatic selection must exclude it to avoid self-following shared state; the user should Fork when independent continuation is intended.

### 8.6 Audio-conditioned video with aligned slicing

Given:

- source audio clip occupies global `[0 s, 10 s)`;
- target generative video occupies global `[5 s, 10 s)`;
- audio input sample is **Aligned with output**;
- coverage policy is Strict.

The resolver maps the target global span through the source clip and materializes only the source interval corresponding to global 5–10 seconds. The provider receives a 5-second audio file. The full 10-second source is not submitted.

### 8.7 Video-conditioned video with aligned slicing

A target generative clip may use a video clip beneath it or on a chosen track. The source must cover the full target span under Strict coverage. The materialized video segment follows source trim and Crop/Stretch time mapping and has the target duration.

### 8.8 Image input sampled from an underlying video

A provider image field may resolve to a video clip covering the requested target frame. LatentSlate maps the global target frame into source time and extracts that exact frame. It is not limited to the source clip's first or last boundary.

### 8.9 Regeneration after editing

- Under Follow, moving the target or source may change both the selected source and sample.
- Under Lock Source, moving the target recomputes an aligned sample/range from the same fixed source.
- Under Freeze Input, moving either clip has no effect on the frozen provider input.

The current resolution shown in the inspector must update before the user queues a new job.

### 8.10 Asset with multiple placements

Because generative configuration is asset-level, a Follow binding needs an explicit generation context.

- When generation is invoked from a selected timeline clip, that clip is the context.
- From the Assets pane with no placements, Follow is unresolved; use an explicit asset or place the target.
- From the Assets pane with exactly one placement, the UI may preselect that placement but must display it clearly as the generation context.
- With multiple placements, generation must block until the user selects a context placement. LatentSlate must not silently choose one.

### 8.11 Sparse temporal conditions

A future LTX-style provider may accept an ordered collection of image or short-video conditions placed at arbitrary output-relative frames/times with optional strength. Each collection item should reuse the same source-selection and sampling concepts defined here. The initial single-field implementation must not fake sparse conditions through parallel unsynchronized arrays in project state.

## 9. Core Conceptual Model

For each provider media field, LatentSlate persists:

```text
Binding = Source Selection + Sampling Rule + Coverage Policy
```

Stability is derived from source selection:

```text
FollowTimeline  -> Follow
TimelineClip    -> Lock Source
ProjectAsset    -> Lock Source
FrozenArtifact  -> Freeze Input
```

This is intentionally one model rather than separate I2V, FLF, audio, V2V, and sparse-keyframe pinning systems.

## 10. Provider Field Defaults

The provider schema remains authoritative for field type and semantic role. LatentSlate derives sensible default sampling without changing provider operation identity.

| Provider field | Default sampling |
|---|---|
| Image with role `start_image` or `source_image` | Frame at output first frame |
| Image with role `end_image` | Frame at output last frame |
| Unroled image field | Frame at output first frame unless explicitly configured otherwise |
| Video field | Range aligned with target output |
| Audio field | Range aligned with target output |
| Explicit project image asset | Static image, independent of target time |
| Explicit whole-media provider contract | Whole source only when the field/schema explicitly permits it |

Workflow-kind labels are for creation menus, filtering, and defaults. Resolution must primarily use the actual media field type and role.

## 11. Normative Persistent Data Model

Exact Rust names may be adjusted during implementation, but the serialized semantics are normative.

### 11.1 Media type

```rust
#[serde(rename_all = "snake_case")]
pub enum BoundMediaType {
    Image,
    Video,
    Audio,
}
```

### 11.2 Track scope

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimelineTrackScope {
    Auto,
    SameTrack,
    Below,
    SpecificTrack { track_id: Uuid },
}
```

Semantics:

- `Auto`: use the full deterministic ranking described later.
- `SameTrack`: only consider source clips on the context clip's track.
- `Below`: consider compatible video/audio tracks beneath the context track, nearest first. “Below” follows the application's current visual track order, not track name.
- `SpecificTrack`: consider only the referenced stable track UUID.

A future named-role query may be added, but track names are mutable and must not be the persistent identity.

### 11.3 Timeline query

```rust
pub struct TimelineSourceQuery {
    #[serde(default)]
    pub scope: TimelineTrackScope,
    #[serde(default = "default_true")]
    pub prefer_touching: bool,
}
```

`prefer_touching` is persisted because same-track continuation is an editorial preference, not an implementation accident. Auto defaults it to true. Disabling it allows a user to prefer a covering reference track over an adjacent same-track clip while keeping the same query model.

### 11.4 Frame point

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaFramePoint {
    OutputStart,
    OutputEnd,
    OutputOffset { seconds: f64 },
    OutputFrame { frame: u32 },
    SourceStart,
    SourceEnd,
    SourceTime { seconds: f64 },
}
```

`OutputFrame` is useful for frame-native providers and sparse-condition authoring. `OutputOffset` is useful when provider contracts are time-native. The implementation may normalize one into the other using target FPS, but both user intents should remain representable.

### 11.5 Sampling rule

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaSample {
    Auto,
    Whole,
    Frame { at: MediaFramePoint },
    AlignedRange,
    SourceRange {
        start_seconds: f64,
        duration_seconds: f64,
    },
}
```

Rules:

- `Auto` is normalized from field type/role before resolution.
- `Whole` is valid for a compatible explicit asset or a provider field that truly wants the whole source. It is not a substitute for aligned timeline slicing.
- `Frame` is valid for image provider fields. A video source is decoded at the mapped source time; an image source returns its static file.
- `AlignedRange` is valid for video/audio fields and requires context.
- `SourceRange` is explicit source-media time and does not change when the target placement moves, although the source identity may still be Follow or Locked.

### 11.6 Coverage policy

```rust
#[serde(rename_all = "snake_case")]
pub enum MediaCoveragePolicy {
    Strict,
    TrimToOverlap,
    PadSilence,
    HoldEdges,
    Loop,
}
```

Only `Strict` is supported in the initial implementation. The other serialized names reserve explicit future behavior and must produce a clear “not implemented” preflight error rather than silently degrading to Strict or applying hidden behavior.

### 11.7 Source selection

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaBindingSource {
    FollowTimeline {
        #[serde(default)]
        query: TimelineSourceQuery,
    },
    TimelineClip {
        clip_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    ProjectAsset {
        asset_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    FrozenArtifact {
        path: PathBuf,
        media_type: BoundMediaType,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        original_binding: Option<Box<MediaBindingSpec>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<FrozenMediaOrigin>,
    },
}
```

Requirements:

- `TimelineClip.version` captures the exact generative version active when source lock occurs. Imported assets leave it `None`.
- `ProjectAsset.version` is required for a locked generative source and omitted for imported media.
- `FrozenArtifact.path` is project-relative and must point to retained project-owned storage, not an evictable cache path.
- `original_binding` enables Unfreeze to restore the pre-freeze rule.
- `origin` is explanatory provenance, not a live dependency.

### 11.8 Binding specification

```rust
pub struct MediaBindingSpec {
    pub source: MediaBindingSource,
    #[serde(default)]
    pub sample: MediaSample,
    #[serde(default)]
    pub coverage: MediaCoveragePolicy,
}
```

A field's persisted binding must never store the current winning timeline candidate as if it were the rule. Follow keeps the query. Lock stores source identity. Freeze stores the artifact.

### 11.9 Generative configuration

`GenerativeConfig` should add:

```rust
#[serde(default)]
pub media_bindings: HashMap<String, MediaBindingSpec>,
```

Key is the exact provider input field name.

Existing `inputs` remains for scalar/literal values and backward-compatible `InputValue` media references during migration. New UI writes media fields to `media_bindings`, not to `inputs` or generic `reference_slots`.

`reference_slots` remain compatibility aliases for old projects and continuation actions until migrated. New feature code must not create an additional parallel source of truth.

### 11.10 Asset Lab nodes

Because Asset Lab nodes can have their own provider and media inputs, each node should add its own media binding map:

```rust
#[serde(default)]
pub media_bindings: HashMap<String, MediaBindingSpec>,
```

A node's media bindings are resolved in the same way as an asset-level config. A node without timeline context may use explicit assets/versions or require the caller to supply a context clip.

### 11.11 Resolution record

```rust
pub struct ResolvedMediaInput {
    pub media_type: BoundMediaType,
    pub source_media_type: Option<BoundMediaType>,
    pub stability: MediaBindingStability,
    pub relation: MediaBindingRelation,
    pub sample: MediaSample,
    pub source_asset_id: Option<Uuid>,
    pub source_clip_id: Option<Uuid>,
    pub source_version: Option<String>,
    pub source_path: Option<PathBuf>,
    pub materialized_path: PathBuf,
    pub target_range: Option<MediaTimeRange>,
    pub source_range: Option<MediaTimeRange>,
    pub target_frame_time: Option<f64>,
    pub source_frame_time: Option<f64>,
}
```

This record must be serializable and understandable without rerunning the resolver. It explains one concrete job input.

`materialized_path` is project-relative whenever possible. A whole imported source may be represented by its existing project-relative path rather than copied.

### 11.12 Relation enum

At minimum:

```rust
pub enum MediaBindingRelation {
    ExplicitAsset,
    ExplicitClip,
    Frozen,
    ExactKeyframe,
    TouchingPrevious,
    TouchingNext,
    CoveringFrame,
    CoveringRange,
}
```

Do not label a strict partial range as a valid relation. Partial coverage is an error. If future overlap policies are implemented, they may add a distinct relation/diagnostic.

### 11.13 Generation job and record

`GenerationJob` must carry immutable snapshots:

```rust
pub media_bindings_snapshot: HashMap<String, MediaBindingSpec>,
pub resolved_media_inputs: HashMap<String, ResolvedMediaInput>,
```

`GenerationRecord` must persist the same two maps alongside the existing scalar/input snapshot.

On successful generation:

- update the active version and output metadata;
- append/replace the generation record;
- do **not** replace live Follow bindings with resolved clips;
- do **not** replace Lock Source with a frozen file;
- do **not** alter the persistent binding unless the user explicitly performs Lock or Freeze.

## 12. Stability Semantics

### 12.1 Follow

Follow means:

- source identity is selected by the persisted query;
- the source may change when clips move, tracks reorder, versions change, sources disappear, or new higher-priority candidates appear;
- sample/range is recalculated from current context;
- “Resolved now” is advisory until a job is queued;
- queued jobs keep their concrete snapshot even if the project changes while the job runs.

Example:

```text
Source: Follow Timeline / Auto
Sample: Output first frame
Current result: Generated Video A, touching previous clip, source frame 4.958 s
```

### 12.2 Lock Source

Lock Source converts the current valid resolution into a persistent explicit source:

- timeline result -> `TimelineClip { clip_id, version }`;
- explicit project result -> `ProjectAsset { asset_id, version }`.

The sample rule remains unchanged.

For aligned audio/video:

- moving the target recalculates the source range through the same locked clip;
- changing the locked source clip's trim or Crop/Stretch mapping recalculates the sample;
- changing an unrelated track cannot switch the source;
- if the source is a generative asset, its exact version is retained even if a later version becomes active.

For a static image, Lock Source normally produces the same media regardless of target movement because the source has no temporal extent.

### 12.3 Freeze Input

Freeze Input is available only when the binding currently resolves and materializes successfully.

The operation:

1. materializes the current exact provider input if needed;
2. copies it into retained project-owned frozen storage;
3. replaces the binding source with `FrozenArtifact`;
4. records the original binding and explanatory origin;
5. saves the generative config.

After freezing:

- timeline movement does not change the input;
- source edits do not change the input;
- generative source version changes do not change the input;
- source deletion does not break the frozen input;
- queueing may use the retained file directly.

Unfreeze restores `original_binding`. If a legacy frozen entry lacks an original binding, Unfreeze must require the user to choose a new source rather than guessing.

## 13. Target Time and Interval Semantics

### 13.1 Half-open intervals

Timeline ranges use half-open intervals:

```text
[start, end)
```

The mathematical right edge is not itself a visible frame. This convention prevents adjacent clips from sharing an interval frame while still allowing boundaries to touch.

### 13.2 Frame normalization

Do not compare clip boundary `f64` values directly for editorial equality. Convert global timeline positions to frame indices/boundaries using the project timeline FPS and the existing snapping helpers.

Recommended helpers:

```text
boundary_frame(t) = round(t * timeline_fps)
frame_time(n)      = n / timeline_fps
```

Two boundaries touch when their normalized boundary frame indices are equal.

A keyframe image matches a requested frame when its anchor frame index equals the requested frame index.

Raw floating epsilon may still be used internally after frame normalization, but must not define user-visible touching behavior by itself.

### 13.3 Target output window

For a context clip:

```text
G0 = context.start_time
G1 = context.end_time()
```

The target range is `[G0, G1)`.

Target output FPS should be chosen in this order:

1. target generative video's stored FPS when valid;
2. explicit provider FPS input synchronized into the config when valid;
3. project timeline FPS as fallback.

Target output frame count should use the generative asset's stored frame count when available. A provider's explicit frame-count/duration values should already be reconciled by existing generation timing code.

### 13.4 First output frame

```text
output_first_global = G0
```

### 13.5 Last output frame

The last visible output frame is not `G1`.

Preferred calculation when frame count `N` and FPS `F` are known:

```text
output_last_global = G0 + (N - 1) / F
```

Fallback when only duration is known:

```text
output_last_global = max(G0, G1 - 1 / F)
```

This rule is mandatory for end-image sampling and same-track source final-frame extraction.

### 13.6 Output-relative sample

For `OutputOffset { seconds }`:

```text
sample_global = G0 + seconds
```

It must lie on or between the first and last output frame. Out-of-range offsets fail preflight.

For `OutputFrame { frame }`:

```text
sample_global = G0 + frame / F
```

The frame must be `< N` when frame count is known.

## 14. Source Eligibility

### 14.1 General requirements

A source candidate is eligible only when:

- the source asset exists;
- its media type can satisfy the provider field;
- the required source file exists;
- a generative source has a valid exact version for Lock or a valid active version for Follow;
- it is not the target context clip;
- automatic resolution does not create a shared-asset self-reference;
- its temporal coverage satisfies the sample and Strict policy;
- the track is within the configured scope.

### 14.2 Media compatibility

| Provider field | Eligible source media |
|---|---|
| Image | Image, or Video sampled to a frame |
| Video | Video |
| Audio | Audio |

A video containing audio is not automatically treated as an audio source in the initial feature unless the current asset model explicitly exposes it as compatible audio media. Adding audio-stream extraction from video is a separate explicit capability.

### 14.3 Hollow generative sources

A hollow generative asset is not eligible. LatentSlate must not search arbitrary leftover files in its folder. Eligibility requires a selected version whose expected output file exists.

### 14.4 Generative version semantics

- Follow uses the source asset's active version at resolution time.
- Lock captures the exact active version in the binding.
- Freeze stores exact bytes and optional origin version.
- A generation record always stores the exact source version actually used.

### 14.5 Self-reference exclusion

Automatic timeline queries must exclude every clip whose `asset_id` equals the target generative asset's ID, not merely the context clip ID. Because multiple placements share one config and active output, selecting another placement of the same asset would create unstable self-reference.

Explicit Asset Lab references to an earlier exact version of the same asset remain valid through `GenerationRef`/version-specific binding semantics; they are not timeline Follow.

## 15. Deterministic Timeline Resolution

### 15.1 Resolver input

The pure resolver receives:

- project state;
- target asset ID, when applicable;
- context clip ID, when applicable;
- provider input field;
- normalized `MediaBindingSpec`.

It returns a plan containing:

- chosen source IDs/version/path;
- relation;
- target sample time/range;
- mapped source time/range;
- source/output media types;
- diagnostics;
- no disk writes.

Inspector and timeline visualization must use this same pure plan. Generation calls the same resolver and then materializes it. There must not be separate UI and execution heuristics.

### 15.2 Explicit source resolution

Explicit sources do not enter candidate ranking.

- `TimelineClip` resolves exactly that clip and captured version.
- `ProjectAsset` resolves exactly that asset/version.
- `FrozenArtifact` validates and uses exactly that retained file.

An explicit source that cannot satisfy the requested sample fails; LatentSlate must not silently fall back to Auto.

### 15.3 Follow candidate classes for frame inputs

Candidates are classified as:

1. `ExactKeyframe`
2. `TouchingPrevious` or `TouchingNext`
3. `CoveringFrame`

Relation rank is evaluated before general track distance.

#### ExactKeyframe

A Keyframe image clip whose anchor frame equals the requested target frame.

- Valid on any track permitted by scope.
- Strongest default editorial signal.
- If multiple exact keyframes exist, track-priority and deterministic tie-breakers select one.

#### TouchingPrevious

For a start-oriented frame sample, a video clip on the same track whose end boundary touches the target clip's start boundary. The sampled source frame is the source clip's final visible frame.

For an end-oriented frame sample, this relation is normally not used unless an explicit provider/sample rule asks for a preceding boundary at that output point.

#### TouchingNext

For an end-oriented frame sample, a video clip on the same track whose start boundary touches the target clip's end boundary. The sampled source frame is the source clip's first visible frame.

This supports reverse bridging/anchoring without changing the default start-continuation use case.

#### CoveringFrame

A compatible source whose eligible timeline interval includes the requested global frame.

- Still image: covered by its displayed clip interval.
- Keyframe image: not a CoveringFrame candidate except at its exact anchor, where it is ExactKeyframe.
- Video: map the requested global frame through clip source-time mapping.

### 15.4 Follow candidate classes for range inputs

Range inputs use `CoveringRange` only in the Strict initial implementation.

A source clip covers the target range only when:

```text
source_clip.start <= target.start
source_clip.end   >= target.end
```

using normalized frame boundaries for the comparison.

A clip that overlaps only part of the target is ineligible and produces a useful partial-coverage diagnostic when it is otherwise the best candidate.

### 15.5 Track priority

Within the same relation rank, default Auto priority is:

1. same track;
2. immediately below the context track;
3. farther tracks below, nearest first;
4. tracks above, nearest first.

Exception: a touching relationship is inherently same-track. Exact keyframe still outranks touching by relation rank, as explicitly decided, but among multiple exact keyframes track priority applies.

`SameTrack`, `Below`, and `SpecificTrack` filter candidates before ranking.

### 15.6 `prefer_touching`

When `prefer_touching` is true, the ranking above applies.

When false, frame relation rank becomes:

1. ExactKeyframe
2. CoveringFrame
3. TouchingPrevious/TouchingNext

This provides an explicit way to keep a dedicated reference track authoritative while preserving same-track continuation capability.

### 15.7 Deterministic tie-breakers

After relation and track priority:

1. smallest normalized temporal distance, where applicable;
2. lower track-order distance;
3. earlier clip start time;
4. stable clip UUID lexical/order comparison.

No candidate order may depend on hash-map iteration or current vector accident without an explicit stable final comparison.

The inspector should state when multiple candidates existed and which priority selected the winner, for example:

```text
Resolved from 2 exact keyframes; chose the nearest track below.
```

### 15.8 No nearest-boundary fallback by default

The resolver must not choose an arbitrary image/video boundary several seconds away merely because it is the nearest candidate. Eligibility is based on exact keyframe, touching boundary, or temporal coverage. This avoids surprising long-distance links.

A future explicit “nearest boundary within tolerance” query may be added if a demonstrated workflow requires it.

## 16. Source-Time Mapping

### 16.1 Timeline clip source mapping is authoritative

For a source clip and global sample time:

```text
local_time  = global_time - source_clip.start_time
source_time = source_clip.source_time_for_local(local_time, asset.duration_seconds)
```

Use the existing `Clip` methods rather than duplicating Crop/Stretch math in the binding resolver.

### 16.2 Crop mode

Crop preserves playback speed. The source range advances one source second per target second, offset by trim-in and constrained to available source duration.

For aligned range:

```text
source_start = map(target_global_start)
source_end   = map(target_global_end)
```

The materialized duration should already equal the target duration, apart from frame/sample quantization.

### 16.3 Stretch mode

Stretch maps the visible source span across the clip's timeline duration. Mapping both target endpoints may produce a source range whose duration differs from target duration.

The materializer must:

1. extract the mapped source range;
2. retime it to exactly the target output duration;
3. preserve pitch behavior according to ordinary media expectations for the initial implementation:
   - video uses timestamp retiming;
   - audio uses a valid tempo filter chain rather than pitch-shifting by sample-rate abuse.

If the required stretch factor cannot be represented safely by the chosen FFmpeg filters, fail with a clear error rather than silently producing the wrong duration.

### 16.4 Source boundary samples

`SourceStart` and `SourceEnd` ignore target alignment and select boundaries of the explicit source scope:

- for a timeline clip, use its first or final visible source frame after trim/mapping;
- for a project asset, use the first or final media frame;
- for a static image, both resolve to the same image.

The final visible source frame is:

```text
max(source_visible_start, source_visible_end - 1 / source_fps)
```

Use known generative/source FPS when available; otherwise fall back to project FPS.

### 16.5 Explicit source time/range

Explicit source times are interpreted in raw source-media coordinates. A timeline clip source may still constrain whether the explicit time is inside the clip's visible mapped source interval; the implementation must state this clearly in the UI.

Recommended initial rule:

- `ProjectAsset + SourceTime/SourceRange`: validate against whole asset duration.
- `TimelineClip + SourceTime/SourceRange`: validate against the clip's visible mapped source span, because Lock Source to a clip should preserve its trim context.

## 17. Still and Keyframe Image Semantics

### 17.1 Explicit binding

An explicit binding to an image asset or image clip always yields that static image. Display mode does not alter bytes.

### 17.2 Follow binding

- Still image clip is eligible for a frame sample when the target frame lies inside its half-open clip interval.
- Keyframe image clip is eligible only when the target frame equals its anchor frame.
- A Keyframe clip should not be treated as covering an extended default duration inherited from generic clip storage.

### 17.3 Preview canvas behavior

Existing preview behavior remains:

- Still image displays across its interval.
- Keyframe image displays only at its exact frame/reference moment.

Binding resolution does not change preview visibility rules.

## 18. Strict Coverage

### 18.1 Frame coverage

A frame sample is valid when:

- static image: explicit asset, or eligible Still/Keyframe timeline placement as above;
- video: requested global frame is within the source clip's visible interval, or a touching relation explicitly selects its first/final visible frame;
- frozen image: retained file exists.

### 18.2 Range coverage

For Strict aligned range, one source clip must cover the full target interval. The initial implementation does not combine multiple adjacent source clips into one provider input.

### 18.3 Diagnostic quality

When a compatible source overlaps but does not fully cover the target, report:

- target global range;
- candidate global range;
- missing leading/trailing duration;
- current policy (`Strict`);
- an instruction to extend/move the source or choose an explicit future coverage policy when available.

Example:

```text
Reference audio covers 00:05.000–00:09.000, but this input requires
00:05.000–00:10.000. Strict coverage is enabled; 1.000 s is missing at the end.
```

## 19. Materialization

### 19.1 Separation from resolution

The pure resolver performs no disk writes. A materializer accepts only a valid plan and returns:

- concrete path;
- provider `InputValue` compatibility snapshot;
- `ResolvedMediaInput` provenance.

### 19.2 Output formats

Recommended interoperable initial formats:

- image frame: PNG;
- video range: MP4/H.264 or an existing project-standard broadly supported intermediate;
- audio range: WAV PCM;
- whole source: original file when no transformation is required.

The materializer must not resize to model alignment or apply provider-family color/normalization rules. Those remain in provider/Engine preprocessing.

### 19.3 Frame extraction

- Prefer the existing FFmpeg frame extraction path.
- Preserve the existing decoder fallback when FFmpeg command extraction fails and the decoder can produce the frame.
- Use the exact mapped source time.
- Include enough key information in the cache identity to avoid returning a frame from an older source version or clip mapping.

### 19.4 Video range extraction

- Seek/extract the mapped source range.
- Remove or preserve audio according to provider field expectations; a pure video field should not rely on incidental audio unless the provider explicitly supports it.
- Retime Stretch samples to target duration.
- Preserve raw source dimensions unless a technically required codec constraint is documented.
- Do not apply timeline transform, opacity, or composite.

### 19.5 Audio range extraction

- Seek/extract the mapped source range.
- Retime Stretch samples to target duration using valid audio tempo processing.
- Do not apply timeline track mute/solo, volume, pan, or mix.
- Produce exact target duration within an implementation-defined sample tolerance; document the tolerance in tests.

### 19.6 Cache storage

Evictable derived inputs live under:

```text
<project>/.cache/media_inputs/
```

The implementation may use readable deterministic names or a small stable digest of a serialized cache key. It must not add a project-wide cryptographic audit ledger or content store merely for this feature.

The cache key must change when any result-affecting property changes, including:

- source asset/path identity;
- source generative version;
- imported file modification identity sufficient to avoid stale reuse;
- source clip ID;
- trim-in, duration, and Crop/Stretch mode;
- source frame time/range;
- target duration/FPS where retiming occurs;
- output media type/format;
- materializer behavior revision.

### 19.7 Frozen storage

Frozen files live in retained project storage owned by the target generative asset, for example:

```text
<generated-asset-folder>/inputs/frozen/<provider-field>/<filename>
```

Exact subfolder naming is implementation detail, but requirements are:

- project-relative path;
- not inside `.cache`;
- copied, not merely referenced from an evictable cache;
- survives source deletion and cache cleanup;
- duplicate/fork operation copies or rebases ownership correctly.

### 19.8 Atomic writes

Derived and frozen files should be written to a temporary sibling path and renamed into place after successful completion. Failed materialization must not leave a file that later appears valid.

## 20. Generation Queue and Reproducibility

### 20.1 Queue-time snapshot

When the user queues generation:

1. reconcile provider schema and scalar inputs;
2. resolve each media binding against the chosen context;
3. fail if any required or explicitly configured input has errors;
4. materialize all concrete media inputs;
5. snapshot binding specs and resolutions into the job;
6. enqueue the immutable job;
7. execute the provider adapter using only concrete values.

### 20.2 Edits after queueing

Moving clips, changing active versions, or editing bindings after queueing must not alter a queued/running job. A new generation attempt resolves again.

### 20.3 Success handling

On success, store:

- provider ID/tool schema identity already used by the queue;
- scalar/literal snapshot;
- media binding snapshot;
- concrete resolved media provenance;
- output version and timestamp;
- Asset Lab node ID where applicable.

Do not mutate the active config's bindings to the concrete job resolution.

### 20.4 Duplicate output/version

Duplicating a generated version should copy its generation record, including media provenance and binding snapshot, because it represents the same output bytes. It must not silently make those historical bindings active unless existing duplicate behavior intentionally activates that version.

## 21. Inspector UX

### 21.1 Inputs card

All provider media fields appear in one **Inputs** or **Media Inputs** card. Do not create provider-family-specific pin panels.

Each field contains:

1. field label and required indicator;
2. Source control;
3. Sample control;
4. stability/action control;
5. Resolved now summary;
6. warning/error text when applicable.

### 21.2 Source control

Menu entries:

- **Follow Timeline**
  - Auto
  - Same Track
  - Tracks Below
  - Specific Track…
- **Timeline Clip…**
- **Project Asset…**
- **Generated Version…** where version choice matters
- **Frozen Input…** only for existing retained frozen inputs
- **None** for optional fields

Timeline and asset choices must include useful details:

- asset display name and active/exact version;
- track name/order;
- clip global start/end;
- image display mode;
- compatibility/frame extraction note;
- missing/unavailable state.

### 21.3 Sample control

Options depend on field type:

Image field:

- Output first frame
- Output last frame
- Output-relative time/frame
- Source first frame
- Source last frame
- Explicit source time

Video/audio field:

- Aligned with output
- Explicit source range
- Whole source, only when allowed

Avoid presenting invalid combinations and waiting until generation to reject them.

### 21.4 Stability controls

State presentation:

- Follow
- Locked Source
- Frozen Input

Actions:

- **Lock Source** — enabled for a valid Follow resolution;
- **Return to Follow** — enabled for locked source; restores default or last stored Follow query;
- **Freeze Input** — enabled for any valid materializable non-frozen binding;
- **Unfreeze** — restores original binding;
- **Choose New Source** — when locked source is missing.

A pin icon may remain, but labels/tooltips must use the unambiguous terms Lock Source and Freeze Input.

### 21.5 Resolved now summary

Example image field:

```text
Resolved now
Gen Video 1 (v3) · touching previous clip
Global 00:05.000 → source 00:04.958
Video frame → cached PNG
```

Example audio field:

```text
Resolved now
Dialogue.wav · locked timeline clip
Global 00:05.000–00:10.000
Source 00:05.000–00:10.000 · 5.000 s WAV
```

Example unresolved field:

```text
Unresolved
No compatible source covers output frame 81 on the selected Keyframes track.
```

### 21.6 Explain automatic ranking

Hover or expandable details should explain:

- configured scope;
- relation rank;
- track priority;
- number of candidates;
- why the winner beat alternatives.

This is essential to the principle that magic is acceptable only when users can track and understand it.

### 21.7 Bulk actions

At card level, optional convenience actions may be provided:

- Lock all currently resolved Follow sources;
- Freeze all currently resolved inputs;
- Return all compatible fields to Follow.

Bulk actions must show confirmation when they affect multiple fields and must report any fields that cannot transition.

### 21.8 Asset-only context UI

When no timeline context exists:

- explicit project/generation sources work normally;
- Follow shows “Timeline context required”;
- exactly one placement may be offered/preselected visibly;
- multiple placements require a context picker before generation.

The UI must not pretend a saved stale candidate is a current Follow resolution.

## 22. Timeline Visualization

### 22.1 Vertical spacing

Increase timeline track/clip height modestly. Recommended first tuning target, subject to visual review:

- track height approximately 42–46 px instead of 36 px;
- clip body approximately 36–40 px instead of 32 px;
- keyframe thumbnail approximately 27–30 px instead of 24 px;
- default expanded timeline panel height increased enough to preserve a practical visible track count.

Acceptance is visual clarity, not exact constants. Horizontal zoom behavior remains unchanged.

### 22.2 Always-visible compact indicators

Generative clips with configured media dependencies may show compact non-connector marks:

- left-edge point for start-oriented frame input;
- right-edge point for end-oriented frame input;
- interior ticks for output-relative/sparse frame conditions;
- thin lower/upper rail for aligned range input;
- small dependency count/chain badge;
- warning badge for unresolved required input.

These indicators should not obscure thumbnails or labels.

### 22.3 Selected-only relationship overlay

When exactly one generative clip is selected, draw connectors from its target input anchors to resolved source geometry.

- Frame source: connect to exact source point.
- Range source: connect to or highlight the exact source span.
- Touching same-track continuation: use a compact local arc/elbow between adjacent clip boundaries rather than a long cross-track line.
- Frozen input: no live source is required; show a short provenance/frozen indicator rather than implying an active timeline dependency.

### 22.4 Visual state grammar

Recommended grammar using existing UI-kit colors where practical:

- Follow: dashed line;
- Lock Source: solid line with lock/pin indicator;
- Freeze Input: subdued line/marker with snowflake/frozen indicator;
- unresolved: warning/danger marker and no false completed connector.

Do not hard-code a color palette in core logic. State and relation drive style in the timeline painter.

### 22.5 Hover information

Hovering a connector/source highlight should show:

- provider field label;
- source asset/version;
- source clip/track;
- relation;
- global target time/range;
- source time/range;
- Follow/Locked/Frozen state.

### 22.6 Interaction scope

The first implementation's overlay is explanatory, not a second authoring system. Clicking a connector may select/open the relevant input or source clip if straightforward, but drag-to-rewire is deferred.

### 22.7 Renderer integration

Use the existing timeline clip geometry collection and overlay painter. Resolver output should be computed before/around paint without invoking materialization or expensive media probes on every frame.

## 23. Editing and Invalidation Behavior

| Edit | Follow | Lock Source | Freeze Input |
|---|---|---|---|
| Move target clip | reselect/recompute | recompute from same source | unchanged |
| Resize target clip | reselect/recompute | recompute; strict coverage may fail | unchanged |
| Move source clip | may reselect | same clip; alignment may change/fail | unchanged |
| Trim source clip | may reselect/recompute | same clip; mapped sample changes | unchanged |
| Change Crop/Stretch | recompute | recompute | unchanged |
| Reorder tracks | Auto may choose differently | unchanged source | unchanged |
| Rename track | no semantic change | no semantic change | no semantic change |
| Delete source clip | may choose another | unresolved | unchanged |
| Delete source asset | may choose another | unresolved | unchanged |
| Change source active version | uses new active version | remains captured version | unchanged |
| Generate new source version | may change next resolution | remains captured version | unchanged |
| Change target provider | reconcile fields; keep compatible bindings | same | same if field compatible |

The UI plan may be recomputed frequently, but expensive media probing/materialization should occur only when needed. Source duration/FPS metadata should use existing caches/probes.

## 24. Legacy Compatibility and Migration

### 24.1 Compatibility strategy

Projects with existing `InputValue` media references must load without destructive failure. The implementation may lazily normalize legacy values into binding specs at read time and persist the canonical form on the next successful save.

### 24.2 Legacy conversion rules

`AssetRef { pinned: false, ... }`:

- convert to `FollowTimeline { Auto }`;
- derive sample from `frame_reference` when present, otherwise provider field role/type;
- do not retain the saved candidate as an invisible fallback source.

`AssetRef { pinned: true, source_clip_id: Some(...) }`:

- convert to `TimelineClip`;
- capture current exact generative version if the source asset is generative;
- preserve first/last source-frame semantics.

`AssetRef { pinned: true, source_clip_id: None }`:

- convert to `ProjectAsset`;
- capture active version for a generative asset;
- preserve frame semantics.

`GenerationRef { asset_id, version, ... }`:

- convert to `ProjectAsset` with the exact version;
- preserve source first/last frame semantics.

`Literal`:

- remains scalar/literal and is not a media binding.

### 24.3 Compatibility aliases

During transition, media lookup may still read:

1. canonical `media_bindings.<provider_field>`;
2. legacy `inputs.<provider_field>`;
3. legacy `reference_slots.<provider_field>`;
4. semantic legacy slots such as `start_image`, `end_image`, `image`, `video`, or `audio`.

New writes use only canonical media bindings. Once canonical binding exists, legacy aliases for that field must not compete with it.

### 24.4 Continuation actions

Existing timeline and Asset Lab continuation actions should seed canonical bindings:

- video last-frame continuation -> locked source clip, sample SourceEnd or output-aligned start as appropriate;
- image continuation -> locked image clip/asset;
- internal edit step -> exact generated version source.

They should stop creating new ambiguous `pinned` media references after canonical support is available.

## 25. Asset Lab Integration

### 25.1 Node-local intent

Each Asset Lab node may own provider media bindings independently from the asset's active configuration. The node inspector should render the same media field component where practical.

### 25.2 Internal generated versions

Asset Lab frequently references an exact earlier version of the same generative asset. This is a locked exact-version source and is valid without timeline context.

### 25.3 Timeline Follow from Asset Lab

A node may use Follow only when generation is invoked with an explicit timeline context. If the Asset Lab is opened from a timeline clip, that clip may be offered as context. If opened from the asset library with multiple placements, require a selection.

### 25.4 Lineage and provenance

The Asset Lab parent/child graph remains generation lineage. Media binding provenance may create additional references but should not automatically rewrite parent lineage. A node can consume media from another output without necessarily becoming its single lineage parent.

## 26. Asset Duplicate/Fork and Deletion

### 26.1 Duplicate/Fork

Duplicating a generative asset must:

- copy canonical media binding configuration;
- preserve references to external assets/clips;
- rebase intentional internal references to the original asset only when the existing duplicate semantics indicate a fork of self-contained lineage;
- copy frozen retained files into the new asset's ownership path;
- update frozen paths in the duplicate config;
- avoid leaving the duplicate dependent on the original asset's frozen-input folder.

The implementation should be conservative: only rebase references that clearly point to the duplicated asset's own versions/owned frozen files.

### 26.2 Deleting source clips/assets

- Follow bindings remain valid rules and may resolve to another candidate.
- Locked bindings become unresolved and name the missing source.
- Frozen bindings remain valid.
- Existing historical generation records retain IDs/paths as provenance even if the live source later disappears.

### 26.3 Deleting generated versions

Version-dependency checks must include canonical media bindings and Asset Lab node bindings. A locked exact-version source blocks deletion or requires explicit dependency handling. A frozen input does not depend on the source version for runtime correctness, although origin metadata may still mention it.

## 27. Provider and LatentSlate Engine Boundary

### 27.1 Project-side responsibilities

LatentSlate:

- resolves timeline queries;
- chooses exact source asset/clip/version;
- validates context and coverage;
- maps global time to source time;
- extracts/retimes media;
- freezes retained artifacts;
- sends concrete paths to adapters;
- stores provenance.

### 27.2 Adapter responsibilities

Adapters:

- upload/read concrete media files;
- transform concrete paths into provider-specific asset handles;
- submit scalar parameters;
- report progress/errors;
- return output artifacts.

### 27.3 Engine contract

Engine recipes continue to declare exact fields and roles. Examples:

- I2V: one `start_image` field;
- first/last frame: `start_image` plus `end_image`;
- V2V: `source_video`/video field;
- audio-conditioned video: audio field;
- sparse conditions: future structured repeated field.

LatentSlate materializes and uploads each concrete item. Engine does not receive track IDs or source-time expressions.

### 27.4 Existing multiplicity gap

The Engine protocol currently supports `ToolInput.multiple`, while the Rust `EngineInput`/`ProviderInputField` normalization does not retain it and media preparation expects one string path. This feature must document that gap.

For ordinary repeated media, a later provider schema update should preserve cardinality and upload an ordered list.

For LTX-style sparse temporal conditions, `multiple: true` alone is not sufficient because each item may need:

- media binding;
- output frame/time placement;
- optional duration;
- optional strength;
- optional item role/type.

The preferred future contract is one repeated structured item, not independent parallel arrays that can become misaligned.

### 27.5 Extension shape for sparse conditions

A future field may conceptually use:

```rust
pub struct TimedMediaCondition {
    pub binding: MediaBindingSpec,
    pub output_frame: Option<u32>,
    pub output_time_seconds: Option<f64>,
    pub duration_seconds: Option<f64>,
    pub strength: Option<f64>,
}
```

The single-binding implementation should keep `MediaBindingSpec` reusable so this addition does not fork source/sampling semantics.

## 28. Error Model

### 28.1 Error categories

At minimum:

- context required;
- source missing;
- source version missing;
- incompatible media type;
- self-reference excluded;
- no timeline candidate;
- frame outside source coverage;
- strict range incomplete;
- unsupported coverage policy;
- unsupported sample/field combination;
- materialization failed;
- FFmpeg unavailable/failed;
- frozen file missing;
- multiple placements require context;
- provider field/schema no longer exists.

### 28.2 Where errors appear

- inline under the media field;
- summarized near Generate button;
- returned through automation API generation/preflight responses;
- copied into failed queue jobs only if failure occurs after queueing;
- timeline warning badge for selected/contextual unresolved fields.

### 28.3 No generic “Missing input” when detail is known

Prefer:

```text
Start Image: no compatible source exists at output frame 0 on track Keyframes.
```

over:

```text
Missing inputs: start_image.
```

The latter may remain a final summary but not the only diagnostic.

## 29. Automation API

The Agent API must be able to inspect and edit canonical bindings without reproducing UI-only logic.

### 29.1 Patch shape

`GenerativeConfigPatch` should accept:

```json
{
  "media_bindings": {
    "start_image": {
      "source": {
        "type": "follow_timeline",
        "query": {
          "scope": { "type": "auto" },
          "prefer_touching": true
        }
      },
      "sample": {
        "type": "frame",
        "at": { "type": "output_start" }
      },
      "coverage": "strict"
    }
  }
}
```

### 29.2 Read state

State responses should expose:

- configured binding;
- current context, when known;
- current resolution summary/diagnostics;
- not necessarily force materialization during a cheap state query.

### 29.3 Shared editor/core path

Automation and UI must call the same editor/core operations for:

- setting binding;
- locking source;
- freezing/unfreezing;
- choosing context;
- generation preflight.

## 30. Performance and Complexity Boundaries

### 30.1 Pure resolution must be cheap

Timeline paint and inspector refresh may call the resolver often. It should use project metadata and existing media metadata caches, not run FFmpeg or hash entire files during every frame.

### 30.2 Materialize only when needed

Materialization occurs on:

- generation preflight/queueing;
- explicit Freeze Input;
- optionally deliberate preview of a derived frame, if the UI requires it.

### 30.3 Avoid duplicate work

Within one resolution/materialization pass, if multiple provider fields request the exact same concrete file, reuse it. Engine upload preparation already deduplicates identical paths within one request; preserve that behavior.

### 30.4 No generalized dependency engine required

A field-by-field resolver with stable plans is sufficient. Do not build a reactive graph database or scheduler merely because connectors are visualized.

## 31. Recommended Rust Module Boundaries

Exact organization may evolve, but keep responsibilities separated and files reviewable.

### 31.1 State

Recommended new file:

```text
src/state/media_binding.rs
```

Contains only serializable domain types and small labels/helpers.

Updates:

- `src/state/mod.rs`
- `src/state/generative.rs`
- project duplicate/delete/reference traversal code

### 31.2 Core resolver/materializer

Recommended:

```text
src/core/media_binding.rs
src/core/media_binding/materialize.rs
src/core/media_binding/tests.rs   # or ordinary cfg(test) modules
```

Responsibilities:

- normalization/migration helpers;
- pure candidate resolution;
- source-time mapping plan;
- materialization;
- freeze/lock/unfreeze operations;
- unit tests.

Avoid putting all state, resolver, FFmpeg commands, UI labels, and tests into one giant file.

### 31.3 Generation integration

Update:

- `src/core/generation.rs`
- `src/egui_app/generation_runtime.rs`
- queue/job state

Purpose-built timeline bridge behavior may remain specialized but should not bypass canonical bindings for ordinary fields.

### 31.4 UI

Update:

- `src/egui_app/attributes_panel.rs`
- optional focused `src/egui_app/media_binding_ui.rs`
- `src/egui_app/asset_lab.rs`
- `src/egui_app/timeline_panel.rs`
- `src/egui_app/timeline_geometry.rs`
- timeline constants in `src/egui_app.rs`

Reusable widgets should not duplicate resolution logic.

### 31.5 Editor and automation

Update:

- `src/editor.rs`
- `src/core/automation.rs`
- API schemas/help documentation as required

## 32. Implementation Sequence

### Phase 1 — Data model and compatibility

1. Add serializable binding/provenance types.
2. Add canonical maps to generative config, Asset Lab node, job, and record.
3. Implement legacy conversion helpers.
4. Update duplicate/delete/version reference traversal.
5. Add serialization/migration tests.

Exit criterion: old configs load; new bindings round-trip; no generation behavior changed yet.

### Phase 2 — Pure resolver

1. Normalize field defaults.
2. Implement explicit asset/clip/frozen resolution.
3. Implement context window and frame normalization.
4. Implement track scopes.
5. Implement exact-keyframe, touching, and covering ranking.
6. Implement strict range coverage.
7. Implement deterministic ties and diagnostics.
8. Add exhaustive resolver tests.

Exit criterion: resolver plans are correct without disk writes or UI.

### Phase 3 — Materializer

1. Generalize exact frame extraction.
2. Add aligned audio slicing.
3. Add aligned video slicing.
4. Add Stretch retiming.
5. Add cache paths/atomic writes.
6. Add freeze/unfreeze/lock operations.
7. Reuse or gradually absorb bridge segment primitives where safe.
8. Add focused materialization tests/fixtures where environment permits.

Exit criterion: valid plans produce concrete files; invalid plans fail before provider calls.

### Phase 4 — Queue and provenance

1. Resolve/materialize at queue time.
2. Add detailed preflight errors.
3. Snapshot rules and resolutions.
4. Preserve live config on success.
5. Persist generation provenance.
6. Update duplicate/version behavior.

Exit criterion: queued jobs are immutable and records explain exact inputs.

### Phase 5 — Explicit inspector UX

1. Add source/sample controls.
2. Add state transitions.
3. Add Resolved now and ranking explanation.
4. Add asset-only context picker.
5. Integrate Asset Lab.
6. Add bulk actions only after individual actions are stable.

Exit criterion: every supported binding can be authored without drag-and-drop.

### Phase 6 — Timeline visualization and spacing

1. Tune track/clip/keyframe heights.
2. Add compact dependency marks.
3. Add selected-only connectors/highlights.
4. Add hover explanations.
5. Verify hit testing and dragging remain correct.

Exit criterion: dependencies are discoverable and selected relationships are unambiguous without clutter.

### Phase 7 — Provider cardinality follow-up

1. Preserve Engine `multiple` in Rust provider schema.
2. Support ordered repeated media uploads where needed.
3. Define a structured timed-condition schema before sparse LTX UI.

This phase may be a separate change if no current recipe requires it.

## 33. Resolver Pseudocode

```text
resolve(project, target_asset_id, context_clip_id, field, binding):
    media_type = media_type_for(field)
    sample = normalize_sample(binding.sample, field.role, media_type)
    target = derive_target_window(context_clip_id, target_asset_id)

    if source is FrozenArtifact:
        validate retained file and media type
        return frozen plan

    if source is ProjectAsset:
        resolve exact asset/version
        validate compatibility
        map sample without timeline clip or require context where needed
        return explicit asset plan

    if source is TimelineClip:
        resolve exact clip and captured version
        validate compatibility
        map sample through clip
        return explicit clip plan

    if source is FollowTimeline:
        require context
        candidates = eligible clips filtered by query scope
        exclude context clip and all clips sharing target asset ID

        if sample is Frame:
            classify exact keyframes
            classify touching same-track boundaries
            classify covering sources
            rank by relation, track priority, distance, stable tie
        else if sample is AlignedRange:
            classify full-covering range sources
            retain best partial candidate only for diagnostics
            rank by track priority and stable tie

        resolve winning candidate's exact active version/path
        map target sample/range through source clip
        return plan

    validate Strict coverage
    return plan or detailed errors
```

Materialization is a separate call and must not alter the plan's configured spec.

## 34. Required Automated Tests

### 34.1 Serialization and migration

- round-trip every source/sample/coverage variant;
- missing new fields default safely;
- unpinned legacy reference -> Follow Auto;
- pinned clip -> locked clip with captured version;
- pinned asset -> locked asset;
- GenerationRef -> exact version;
- first/last frame migration;
- canonical binding overrides legacy aliases;
- old project config remains readable.

### 34.2 Frame math

- output first frame;
- output last frame with known frame count/FPS;
- fallback last frame from duration;
- output-relative seconds;
- output frame index;
- normalized touching boundaries despite floating drift;
- keyframe exact-frame eligibility.

### 34.3 Candidate ranking

- exact keyframe beats touching previous;
- touching previous beats covering source when `prefer_touching` true;
- covering source beats touching when false;
- same-track continuation selects previous video final frame;
- end-oriented input can select touching next first frame;
- immediate below beats farther below within same relation;
- Below scope excludes same/above;
- SpecificTrack uses stable ID;
- deterministic UUID tie-break;
- no nearest distant boundary fallback;
- target asset self-reference excluded;
- hollow generative candidate excluded.

### 34.4 Coverage and mapping

- Still image covers interval;
- Keyframe image only exact frame;
- video covering arbitrary frame maps exact source time;
- audio 0–10 with target 5–10 maps 5–10;
- partial audio range fails Strict;
- Crop mapping with trim-in;
- Stretch mapping produces source range and target retime duration;
- locked source remains same after competing candidate added;
- Follow changes after higher-ranked candidate added;
- frozen input remains valid after source deletion.

### 34.5 Queue/provenance

- queue snapshot does not change after timeline edit;
- successful generation keeps live Follow spec;
- record stores exact source version/path/time;
- batch jobs each carry immutable snapshots;
- duplicate version preserves provenance;
- deletion dependency traversal finds canonical locked version references.

### 34.6 UI/automation unit-level behavior

Where practical:

- context-required message;
- multiple-placement context block;
- Lock Source transition;
- Freeze/Unfreeze transition;
- invalid sample options hidden;
- automation patch round-trip.

## 35. Required Manual/Native Acceptance Scenarios

Run on Windows through the actual application after `cargo check` and the release build wrapper.

### Scenario A — Explicit asset-only I2V

- No target placement.
- Select explicit image asset.
- Generate successfully.
- Confirm no timeline context warning.

### Scenario B — Storyboard keyframe below target

- Keyframe image exactly at target start.
- Follow Auto start image.
- Confirm ExactKeyframe relation and selected connector.
- Move keyframe one frame away; generation becomes unresolved rather than choosing it at a distance.

### Scenario C — Still storyboard interval

- Still image stretched across target start.
- Confirm CoveringFrame relation.
- Trim still so target frame is outside; unresolved.

### Scenario D — Same-track continuation

- Generate A.
- Snap B to A's end on same track.
- Confirm B resolves A's final visible frame.
- Confirm relation says Touching Previous.
- Lock Source, then insert another candidate; source remains A.
- Freeze, then move A; source bytes remain unchanged.

### Scenario E — First/last-frame bridge

- Two keyframes at first/final output frames.
- Both resolve independently.
- Move final keyframe to mathematical clip end rather than final visible frame; verify expected unresolved/one-frame distinction is clear.

### Scenario F — Audio aligned slice

- Audio clip global 0–10; target 5–10.
- Generate with aligned audio.
- Inspect produced WAV duration and source mapping.
- Shorten audio to 0–9; strict preflight blocks with 1-second missing tail.

### Scenario G — Video aligned slice with Stretch

- Source clip uses Stretch.
- Target uses a subrange.
- Inspect derived MP4 duration and visual timing.
- Confirm no timeline transform/opacity is baked.

### Scenario H — Source version behavior

- A v1 is active; B follows A.
- Generate A v2 and activate it; Follow B resolves v2.
- Lock B while v2 active.
- Activate A v3; locked B still resolves v2.
- Freeze B; delete/move A; B remains valid.

### Scenario I — Multiple target placements

- Same target asset on two timeline positions.
- Open from Assets pane and Generate.
- Confirm explicit context selection is required.
- Choose each placement and verify different resolution summaries.

### Scenario J — Timeline visualization regression

- Select/unselect generative clips with multiple inputs.
- Confirm only selected clip draws full connectors.
- Confirm compact marks remain legible.
- Verify clip move/resize/trim handles and hit testing after height changes.

### Scenario K — Existing project migration

- Load project with pinned/unpinned references.
- Confirm equivalent visible semantics.
- Save/reopen and verify canonical binding persistence.

### Scenario L — Asset Lab exact-version edit

- Create edit node from an earlier output.
- Confirm exact version source without timeline context.
- Generate and preserve lineage/provenance.

## 36. Definition of Done

The feature is not done merely because new controls render. It is done when:

- canonical persisted bindings exist;
- old references migrate/load;
- one pure resolver drives UI and execution;
- same-track continuation works;
- arbitrary video-frame sampling works;
- aligned audio/video slicing works;
- Strict coverage blocks incomplete requests;
- Lock Source and Freeze Input behave distinctly;
- generation records retain rules and concrete provenance;
- selected timeline connectors accurately match resolver output;
- asset-only and multiple-placement contexts behave explicitly;
- no timeline identity crosses the provider boundary;
- required automated tests pass;
- `cargo fmt --all -- --check` passes;
- `cargo check`/`cargo check --locked` passes according to repository policy;
- the Windows release wrapper is attempted for the Rust/UI change;
- manual acceptance covers the scenarios above;
- final branch contains ordinary source files and no transport artifacts.

## 37. Deferred Features and Explicit Open Edges

These are deliberately documented so they are not forgotten or accidentally implied.

### 37.1 Timeline Composite source

Future explicit source that renders/mixes the timeline at a frame/range. It requires decisions about transforms, effects, opacity, multiple layers, audio mix, caching, and recursion. Not part of ordinary raw clip bindings.

### 37.2 Non-strict coverage policies

Pad Silence, Hold Edges, Loop, Trim to Overlap, and black-frame behavior require explicit UI, persistence, diagnostics, and materialization tests. Reserved but unsupported initially.

### 37.3 Drag-and-drop wiring

Potential future convenience once explicit controls and visualization are stable. Must call the same binding editor operations and never create hidden links.

### 37.4 Multi-clip range composition

Combining adjacent audio/video clips into one provider input is not supported by Strict single-source range resolution. It may later be represented as Timeline Composite or an explicit sequence binding.

### 37.5 Sparse/multiple timed conditions

Requires provider schema/cardinality work and a structured repeated item. The single-binding core is reusable, but the complete UX is deferred.

### 37.6 Track roles/named queries

A future stable track-role property may make queries such as “Keyframes track” portable without persisting mutable names. Until then, SpecificTrack persists UUID and Auto/Below use order.

### 37.7 Source audio from video files

May be added explicitly if provider workflows require extracting an audio stream from a video asset. It is not assumed by initial media compatibility.

## 38. Example Serialized Bindings

### 38.1 Follow the best timeline start frame

```json
{
  "source": {
    "type": "follow_timeline",
    "query": {
      "scope": { "type": "auto" },
      "prefer_touching": true
    }
  },
  "sample": {
    "type": "frame",
    "at": { "type": "output_start" }
  },
  "coverage": "strict"
}
```

### 38.2 Follow a specific keyframe track for the final output frame

```json
{
  "source": {
    "type": "follow_timeline",
    "query": {
      "scope": {
        "type": "specific_track",
        "track_id": "11111111-2222-3333-4444-555555555555"
      },
      "prefer_touching": false
    }
  },
  "sample": {
    "type": "frame",
    "at": { "type": "output_end" }
  },
  "coverage": "strict"
}
```

### 38.3 Locked audio clip, still aligned to target

```json
{
  "source": {
    "type": "timeline_clip",
    "clip_id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
  },
  "sample": { "type": "aligned_range" },
  "coverage": "strict"
}
```

### 38.4 Locked generated video version sampled at its final source frame

```json
{
  "source": {
    "type": "timeline_clip",
    "clip_id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
    "version": "v3"
  },
  "sample": {
    "type": "frame",
    "at": { "type": "source_end" }
  },
  "coverage": "strict"
}
```

### 38.5 Explicit image asset without timeline context

```json
{
  "source": {
    "type": "project_asset",
    "asset_id": "99999999-8888-7777-6666-555555555555"
  },
  "sample": {
    "type": "frame",
    "at": { "type": "source_start" }
  },
  "coverage": "strict"
}
```

### 38.6 Frozen input

```json
{
  "source": {
    "type": "frozen_artifact",
    "path": "generated/video/target-id/inputs/frozen/start_image/start_image.png",
    "media_type": "image",
    "original_binding": {
      "source": {
        "type": "follow_timeline",
        "query": {
          "scope": { "type": "auto" },
          "prefer_touching": true
        }
      },
      "sample": {
        "type": "frame",
        "at": { "type": "output_start" }
      },
      "coverage": "strict"
    },
    "origin": {
      "source_asset_id": "11111111-1111-1111-1111-111111111111",
      "source_clip_id": "22222222-2222-2222-2222-222222222222",
      "source_version": "v4",
      "target_frame_time": 5.0,
      "source_frame_time": 4.9583333333
    }
  },
  "sample": {
    "type": "frame",
    "at": { "type": "output_start" }
  },
  "coverage": "strict"
}
```

## 39. Example Resolution Record

```json
{
  "media_type": "audio",
  "source_media_type": "audio",
  "stability": "lock_source",
  "relation": "explicit_clip",
  "sample": { "type": "aligned_range" },
  "source_asset_id": "11111111-1111-1111-1111-111111111111",
  "source_clip_id": "22222222-2222-2222-2222-222222222222",
  "source_version": null,
  "source_path": "audio/dialogue.wav",
  "materialized_path": ".cache/media_inputs/dialogue-aligned.wav",
  "target_range": {
    "start_seconds": 5.0,
    "end_seconds": 10.0
  },
  "source_range": {
    "start_seconds": 5.0,
    "end_seconds": 10.0
  },
  "target_frame_time": null,
  "source_frame_time": null
}
```

## 40. Final Architectural Statement

LatentSlate should not model image-to-video, first/last-frame video, audio conditioning, video conditioning, same-track continuation, and sparse keyframes as unrelated pinning features. They are all instances of provider media fields obtaining concrete content from project assets or timeline placements.

The durable abstraction is:

```text
Persisted intent:
    source selection + sampling rule + coverage policy

Queue-time fact:
    exact source asset/clip/version + exact source time/range + materialized file

User-controlled stability:
    Follow | Lock Source | Freeze Input
```

Everything else—provider role defaults, explicit source menus, same-track heuristics, frame/range materialization, timeline connectors, provenance, and future sparse condition collections—should be built around that separation.
