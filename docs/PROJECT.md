# NLA AI Video Creator

> **A local-first, AI-native Non-Linear Animation editor for generative video production.**

---

## 🎯 Vision

**NLA AI Video Creator** is a desktop application designed to bridge the gap between creative intent and AI-powered video generation. It provides filmmakers, animators, and content creators with an intuitive timeline-based environment to orchestrate AI-generated content—keyframe images, video segments, and audio—into cohesive short films.

The tool embraces a **"bring your own AI"** philosophy. Rather than locking users into a single provider or workflow, it offers a modular adapter architecture that lets creators plug in their preferred tools—whether that's commercial APIs like Veo3 and fal.ai, or custom ComfyUI workflows they've painstakingly crafted.

### The Problem

Creating AI-generated short films today is *tedious*:
- Switching between generation tools and video editors
- Manually downloading, renaming, and importing assets
- Losing creative flow while waiting for generations
- No unified timeline view of audio + keyframes + generated segments
- Difficulty coordinating keyframe images with beat markers in music

### The Solution

A purpose-built NLA editor that:
1. **Unifies the workflow** — Audio, keyframes, and video segments live in one timeline
2. **Integrates AI natively** — Generate images and videos directly from the editor
3. **Stays flexible** — Swap providers per-project or per-shot via adapters
4. **Works locally** — Your projects, your machine, your data (with optional cloud features later)

---

## 🏗️ Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                         NLA AI Video Creator                        │
├─────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌────────────┐  │
│  │   Timeline  │  │   Preview   │  │   Assets    │  │ Attribute  │  │
│  │    Editor   │  │    Window   │  │   Browser   │  │   Editor   │  │
│  └─────────────┘  └─────────────┘  └─────────────┘  └────────────┘  │
├─────────────────────────────────────────────────────────────────────┤
│                         Core Engine (Rust)                          │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  App State  │  Selection  │  Asset Manager  │  Job Queue     │   │
│  └──────────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────┤
│                      Provider Adapter Layer                         │
│  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌─────────────────┐   │
│  │  ComfyUI   │ │   fal.ai   │ │   Veo3     │ │  Custom HTTP    │   │
│  │  Adapter   │ │  Adapter   │ │  Adapter   │ │    Adapter      │   │
│  └────────────┘ └────────────┘ └────────────┘ └─────────────────┘   │
│  └────────────┘ └────────────┘ └────────────┘ └─────────────────┘   │
├─────────────────────────────────────────────────────────────────────┤
│                       Rendering Engine                              │
│  ┌───────────────┐   ┌────────────────┐   ┌─────────────────────┐   │
│  │ Thumbnailer   │   │  Compositor    │   │   Frame Server      │   │
│  └───────────────┘   └────────────────┘   └─────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

### Technology Stack

| Component | Technology | Rationale |
|-----------|------------|-----------|
| **UI Framework** | [egui/eframe 0.34](https://www.egui.rs/) | Native immediate-mode desktop UI with shared preview/dialog composition |
| **Language** | Rust | Safety, performance, excellent FFI |
| **Video Processing** | FFmpeg (external) | Industry standard, battle-tested |
| **Async Runtime** | Tokio | De facto Rust async runtime |
| **Serialization** | Serde (JSON) | Provider configs, project files |
| **HTTP Client** | reqwest | Async HTTP for API providers |

### Target Platforms

| Platform | Priority | Status |
|----------|----------|--------|
| Windows 10/11 | **Primary** | Active development |
| macOS | Secondary | Future |
| Linux | Secondary | Future |

---

## 📐 Core Concepts

### 1. Project

A **Project** is the top-level container. It has:
- A name and save location (folder = project, KISS)
- Global settings (resolution, frame rate, export preferences)
- One or more **Tracks**
- Provider configuration (global for MVP)

### 2. App Settings

Global application settings (not per-project):
- **Projects folder location** — Where new projects are created by default
- UI preferences (theme, layout)
- Default providers / presets
- FFmpeg path override (if not on PATH)

### 3. Tracks

The timeline consists of layered tracks:

| Track Type | Purpose | Duplicatable |
|------------|---------|--------------|
| **Video Track** | Holds video clips, image clips (stills with duration), and generative visual content | Yes |
| **Audio Track** | Holds audio clips and generative audio content | Yes |
| **Marker Track** | Holds point-in-time markers (beat markers, scene breaks, notes) | No |

> **Note:** Images are placed on Video tracks as stills with duration, following standard NLE conventions. There is no separate "Keyframe" track—reference images for generation are simply clips that overlap generative clips in time.
> 
> See [CONTENT_ARCHITECTURE.md](./CONTENT_ARCHITECTURE.md) for the full content and generation architecture.

### 4. Markers / Keypoints

Markers are timestamp annotations that can:
- Be placed manually (MVP)
- Be auto-generated from audio analysis (beat detection, transients)
- Carry metadata (labels, colors, types)
- Trigger or guide generation tasks

### 5. Generation Tasks

A **Generation Task** is a request to an AI provider:
- **Image Generation** — Create a keyframe image from a prompt
- **Image-to-Video (I2V)** — Animate a keyframe into a video segment
- **Video-to-Video (V2V)** — Transform/stylize an existing video
- **Video Extension** — Extend a video segment forward or backward
- **Audio Generation** — *(Future)* Generate audio from video, or music from prompts
- **Audio Analysis** — *(Future)* Extract beats, segments, transcription

> Note: Audio isn't just an anchor—it might itself be generated content.

### 6. Provider Entries

Provider entries are the pluggable backends that execute generation tasks. Key principles:
- **Single-purpose** — Each entry does ONE thing (image gen, I2V, etc.). If a service supports multiple capabilities, the user adds separate entries for each.
- Configured via simple JSON/config
- Can be a commercial API, local ComfyUI instance, or custom HTTP endpoint
- ComfyUI entries reference an API workflow JSON via `workflow_path` (relative to repo/app or absolute path; MVP default: `workflows/sdxl_simple_example_API.json`)
- Details of the adapter interface will be discovered during implementation—we're keeping this intentionally vague until we experiment with real ComfyUI workflows.

---

> **Intentionally vague for now.** We'll discover the right abstractions when we actually integrate with ComfyUI and experiment with real workflows. Premature abstraction is the enemy of good design.

---

## 🎥 Rendering & Preview Strategy

### 1. Robust Compositor (egui texture-based)
The current preview path renders composited RGBA frames into an egui texture inside the same native UI pass as panels, modals, and timeline controls. This keeps the desktop shell coherent while preserving a path toward a deeper GPU compositor later.

#### Architecture
1.  **Frame Server (Rust)**
    - Managed by a background thread (Tokio).
    - Responsible for fetching or decoding frame data for the current timestamp.
    - **Caching Strategy**: To ensure smooth scrubbing, we will employ a hybrid strategy:
        - **Images**: Loaded fast from disk/memory.
        - **Video**: decoded on demand or pre-cached as low-res proxy image sequences for performance.
2.  **Compositor (Rust)**
    - Takes raw frame buffers from the Frame Server.
    - Applies a "Render Graph" of operations:
        - **Transform**: Scale, Rotate, Translate (Project Canvas coordinates).
        - **Composite**: Layering using standard blending modes (Source Over).
    - Outputs a single raw RGBA buffer for the viewport.
3.  **Display (egui/eframe)**
    - The egui preview panel uploads the composited RGBA buffer to an egui texture.
    - Modals, panels, preview, and timeline now share one native window composition path.
    - This removes the previous split-surface layering issue where UI overlays could not appear above the preview.

### 2. Thumbnail Generation
Visual feedback on the timeline.
- **Mechanism**: Background FFmpeg task.
- **Output**: Cached JPEGs stored in `.project/cache/thumbnails/`.
- **UI**: egui image/paint primitives for timeline feedback.

---

## 🔌 Provider System

### Design Goals

1. **Simplicity** — Adding a provider should be straightforward, not overwhelming
2. **Single-purpose entries** — Each provider entry does ONE thing. Want image gen AND I2V from the same service? Add two entries.
3. **Configurability** — JSON-based configuration (API URL, auth key, workflow path, etc.)
4. **Flexibility** — A project can mix providers freely (Provider A for images, Provider B for I2V)

### Entry Types

The types of things a provider entry can do:
- **Image Generation** — Text/prompt → Image
- **Image-to-Video (I2V)** — Image → Video segment
- **Video-to-Video (V2V)** — Video → Transformed video
- **Video Extension** — Video → Longer video
- *(Future: Audio analysis, beat detection, etc.)*

### Implementation Notes

> **Intentionally vague for now.** We'll discover the right abstractions when we actually integrate with ComfyUI and experiment with real workflows. Premature abstraction is the enemy of good design.

### Example Providers (Ideas)

| Provider | Type | Notes |
|----------|------|-------|
| ComfyUI workflow #1 | Image Gen | User's custom SDXL style workflow |
| ComfyUI workflow #2 | I2V | User's AnimateDiff or similar |
| fal.ai Kling | I2V | Commercial API |
| Veo3 | I2V | Commercial API |
| Replicate Model X | Image Gen | Commercial API |

Users can add as many entries as they need. The same underlying service (like ComfyUI) might have multiple entries for different workflows/purposes.

### Dynamic Provider UI (Sockets)

Providers have **bespoke input requirements**. Some need just text, others need text + image, etc. The UI should have a framework that:
- Allows providers to declare their input schema
- Dynamically renders appropriate input fields (text boxes, image pickers, sliders, etc.)
- Acts as "sockets" that can be plugged into from the predefined tools

> **Intentionally vague on implementation.** The goal is to avoid hardcoding provider UIs—instead, a harness that adapts based on what each provider needs.

---

## 🎬 User Workflow (MVP)

### Phase 1: Setup
### Phase 1: Setup
1. User opens the app → **Startup Modal** appears (New Project / Open Project)
2. **New Project**: User selects a folder/name. System immediately creates the folder structure on disk.
3. **Open Project**: User selects an existing `project.json` or folder.
4. App loads with the project active.
2. Sets project dimensions (1080p, 4K, etc.) and frame rate
3. Configures one or more providers in the Provider panel

### Phase 2: Audio & Planning
1. Imports an audio file (MP3/WAV) → appears on Audio Track
2. Plays through audio, manually places markers at key moments
3. Optionally labels markers ("intro", "beat drop", "climax")

### Phase 3: Keyframes
1. At each marker (or selected markers), user creates a keyframe slot
2. Either:
   - **Imports** an existing image
   - **Generates** via a configured image provider (types prompt, clicks "Generate")
3. Keyframe appears in the Keyframe Track at that timestamp

### Phase 4: Video Generation
1. User selects two adjacent keyframes
2. Chooses an I2V provider and parameters
3. Clicks "Generate Video Segment"
4. Generated video appears in the Video Track, spanning between keyframes

### Phase 5: Export
1. User arranges tracks as desired
2. Clicks "Export" → FFmpeg composites audio + video → final output
3. *(Future)* Option to "Export Parts" — individual clips with nice filenames for external editing

---

## 🎨 UI Principles

### Fluidity & Polish

The UI should feel **alive and responsive**:
- **Hover effects** on all interactive elements (buttons, clips, markers)
- **Smooth transitions** on state changes (selections, panel toggles)
- **Timeline smoothness** — scrubbing, zooming, and panning should feel buttery
- No jarring state jumps; prefer animated transitions where practical

### Attribute Editor

A context-sensitive panel that displays properties of the current selection:
- If **one item** is selected → show all editable properties
- If **multiple items of the same type** are selected → show common properties, edits apply to all
- If **mixed types** are selected → show only universally applicable actions (delete, etc.)

This panel adapts based on what's selected in the **timeline** or **asset browser**.

### Labels vs. Filenames

Every asset (clip, keyframe, audio file) has:
- **Filename** — The actual file on disk (auto-generated or imported)
- **Label** — A user-facing display name (optional, can be different from filename)

This supports:
- Friendly display in the UI ("Intro Scene" instead of `seg_001_002.mp4`)
- Future "Export Parts" feature where clips get nice descriptive names

---

## 🗂️ Project File Structure

```
my-project/
├── project.json          # Main project file
├── audio/                # Imported audio files
│   └── soundtrack.mp3
├── keyframes/            # Generated or imported images
│   ├── kf_001_intro.png
│   └── kf_002_beatdrop.png
├── video_segments/       # Generated video clips
│   ├── seg_001_002.mp4
│   └── seg_002_003.mp4
├── exports/              # Final rendered outputs
│   └── final_v1.mp4
```

Global providers (MVP, Windows):
```
%LOCALAPPDATA%\NLA-AI-VideoCreator\providers\
├── <provider-id>.json
└── ...
```

Workflow templates (repo):
```
workflows/
├── sdxl_simple_example_API.json
└── sdxl_simple_example_manifest.json
```

### `project.json` Schema (Simplified)

```json
{
  "version": "1.0",
  "name": "My Short Film",
  "settings": {
    "width": 1920,
    "height": 1080,
    "fps": 24,
    "duration_seconds": 60.0,
    "preview_max_width": 960,
    "preview_max_height": 540
  },
  "tracks": {
    "audio": [...],
    "markers": [...],
    "keyframes": [...],
    "video": [...]
  },
  "provider_assignments": {
    "image_generation": "my_comfy_workflow",
    "image_to_video": "fal_kling"
  }
}
```

---

## 📦 MVP Feature Set

### Must Have (v0.1)

- [x] **UI Shell** ✓
  - Main application layout (title bar, panels, timeline, status bar)
  - Charcoal monochrome color scheme with functional accent colors
  - Consistent borders and typography
  - Panel headers with matching heights

- [x] **Panel System** ✓
  - Resizable side panels (drag edge, instant feedback)
  - Collapsible side panels (icon button → thin rail)
  - Collapsible timeline (icon button → bottom rail with controls visible)
  - Smooth animated collapse/expand transitions
  - Hover feedback on collapsed rails
  - Click anywhere on collapsed rail/header to expand
  - Drag state persists if mouse leaves window and returns

- [x] **Data Model & Project Management** (Phase 1) ✓
  - [x] Core data structures (Project, Track, Clip, Asset, Marker)
  - [x] Project save/load (JSON serialization)
  - [x] Project creation workflow (new project → folder)
  - [x] Project settings (resolution, fps, duration, preview downsample)
  - [x] Project settings edit flow (reuse startup modal UI)
  - [x] In-project asset storage (audio/, images/, video/, generated/)

- [x] **Timeline Editor** (Foundation) ✓
  - [x] Horizontal scrolling timeline (robust hierarchical structure)
  - [x] Zoom in/out (pixel-based scaling)
  - [x] Multiple track lanes (synced w/ headers)
  - [x] Frame-snapped playhead (project fps alignment)
  - [x] Click-to-scrub interaction (click/drag anywhere on ruler to seek)
  - [x] Playback/Seek controls (Play, Pause, Step Frame)
  - [x] Frame ticks on ruler (subtle, at high zoom levels)
  - [x] Timecode display (HH:MM:SS:FF format)
  - [x] Dynamic track list (from project data, not hardcoded)
  - [x] Add/remove tracks UI
  - [x] Audio playback integration (cpal playback + audio-clock-driven playhead)

- [x] **Track System** (Revised Architecture) ✓
  - [x] Video tracks — hold video clips, image clips (stills), generative clips
  - [x] Audio tracks — hold audio clips, generative audio clips
  - [x] Marker track — point-in-time markers (single, non-duplicatable)
  - [x] Default new project: Video 1, Audio 1, Markers
  - [x] User can add additional Video/Audio tracks
  - [x] Track selection now drives Attributes panel for track-level controls

- [x] **Clip System**
  - [x] Render clips on timeline tracks (positioned by start_time, sized by duration)
  - [x] Visual distinction: standard clips vs generative clips (dashed border, ✨ prefix)
  - [x] Clip Interactions:
    - [x] Move clips (drag body to reposition, frame-snapped 60fps)
    - [x] Resize clips (drag left/right edges, min duration 0.1s)
    - [x] Delete clips (right-click custom context menu, native menu suppressed)
    - [x] Move clips between compatible tracks (context menu up/down)
    - [x] Clip snapping (drag/move/trim to edges, markers, playhead)
  - [x] Clip Creation:
    - [x] "Add to Timeline" (context menu) — renders at playhead
    - [x] Drag & Drop from Asset Panel — renders at drop position
  - [x] Clip labels (per-instance display name) editable in Attributes panel
  - [x] Clip volume control (audio + video clips) in Attributes panel
  - [ ] Clip thumbnail/waveform preview
    - [x] **Thumbnailer Service**: Background FFmpeg task to generate cache images
    - [x] **Timeline Rendering**: UI logic to display cached thumbnails on clips

- [x] **Asset System** (Phase 2A) ✓
  - [x] Assets panel shows project assets (imported + generative)
  - [x] Import files via native file dialog
  - [x] Visual distinction: standard assets vs generative assets (⚙️ badge, dashed border)
  - [x] Drag assets to timeline to create clips (with compatibility checks)
    - [x] Copy imported files to project folder
    - [x] **Import Logic**: Create `Project::import_file` to copy external files to `audio/`, `video/`, etc.
    - [x] **Path Normalization**: Ensure `Asset` stores relative paths for portability
    - [x] **Collision Handling**: Auto-rename files if they already exist in project folder

- [ ] **Generative Assets** (Core Innovation) — In Progress
  - [x] "+ New Generative Video/Image/Audio" buttons in Assets panel
  - [x] Generative asset folder structure (generated/{type}/{id}/)
  - [x] Placeholder display for un-generated assets (dashed border, ⚙️ icon)
  - [x] Generative config file (config.json)
    - [x] Create config.json on generative asset creation
    - [x] Persist provider id selection
    - [x] Persist input bindings + version history
  - [x] Version management (v1, v2, ... in asset folder)
  - [x] Active version selection (stored in config.json)
  - [x] Delete active version with inline confirmation
  - [x] Active version load/save on project open
  - [x] Thumbnail updates after generation completes

- [x] **Markers**
  - [x] Right-click to add marker at playhead position
  - [x] Drag markers to reposition
  - [x] Delete markers
  - [x] Marker labels (optional)
  - [x] Marker descriptions (optional)
  - [x] Marker colors (optional)

- [x] **Audio Track**
  - [x] Import MP3/WAV
  - [x] **Waveform visualization** (essential)
  - [x] Basic playback controls (play, pause, seek)
  - [x] **Audio scrubbing** - hear audio while dragging playhead (critical for usability)

- [ ] **Selection & Attribute Editor**
  - [x] Clip selection state (single)
  - [x] Attribute panel for clip transforms (position/scale/rotation/opacity)
  - [x] Track selection state
  - [ ] Asset selection state
  - [ ] Multi-select support for same-type items
  - [x] For generative clips: provider picker
  - [x] For generative clips: generate button
  - [x] For generative clips: dynamic input fields (schema-driven)
  - [x] For generative clips: version selector (active version)
  - [x] For generative clips: status/progress line (queued/running/done)

- [ ] **Smart Input Suggestions** (Timeline as Implicit Wiring)
  - [ ] When configuring generative clip inputs, auto-surface overlapping assets
  - [ ] "In Time Range" section at top of asset picker
  - [ ] "Other Assets" section below
  - [ ] Duration defaults to clip duration on timeline

- [x] **Provider System**
  - [x] Provider entry data model (output type, input schema, connection info)
  - [x] Global provider config storage under `%LOCALAPPDATA%\NLA-AI-VideoCreator\providers\`
  - [x] Provider configuration UI (JSON editor modal)
  - [x] Provider builder UI (ComfyUI workflow picker, node browser, exposed inputs, output selector)
  - [x] Dynamic input schema rendering (text, number, boolean, enum)
  - [x] Health check / connection test
  - [x] ComfyUI adapter (first provider)
    - [x] Minimal T2I workflow (prompt + seed)
    - [x] Image output download/save into generated/{type}/{id}/

- [ ] **Generation Pipeline**
  - [ ] Queue generation jobs (async, non-blocking)
  - [ ] Job state tracking (queued/running/succeeded/failed)
  - [x] Progress/status feedback in UI
  - [x] Save generated files to asset folder (v1.png / v1.mp4 / v1.wav)
  - [x] Update config.json + asset active version on completion
  - [x] Trigger thumbnail refresh after generation
  - [ ] Cascading: regenerating dependent uses active version of inputs

- [ ] **FFmpeg Integration**
  - [ ] Export final timeline to video file
  - [ ] Assume FFmpeg on PATH
  - [ ] Basic export settings

- [ ] **Preview Window** (Priority: High)
  - [x] Clip transform data model (position/scale/rotation/opacity)
  - [x] Preview render loop (playhead-driven frame requests)
  - [x] Frame server v0: load stills + in-process FFmpeg decode worker
  - [x] Compositor v0: layer stack with opacity + basic scale/translate
  - [x] Preview panel renders composited frame via direct RGBA canvas upload
  - [x] Transform pipeline v1: rotation, scale, translation
  - [x] Canvas compositor + direct buffer upload (replace PNG cache)
  - [x] Native preview surface (wgpu) integration
  - [x] Frame caching/prefetch for smooth scrubbing
  - [ ] Transform pipeline v2: anchor/pivot support

### Nice to Have (v0.2+)

- [ ] I2V generation (image-to-video providers)
- [ ] V2V transformation (video-to-video providers)
- [ ] Video extension
- [ ] Audio generation (music/sfx providers)
- [ ] Batch variations ("Generate 5 variations with different seeds")
- [ ] Beat detection / auto-marker placement
- [ ] Undo/redo
- [ ] Provider presets library
- [ ] fal.ai provider
- [ ] Replicate provider
- [ ] Multiple audio tracks with mute/solo
- [ ] Multiple video tracks with visibility toggle
- [ ] Audio generation providers
- [x] Rename/relabel clips and assets
- [ ] Export Parts (individual clips with descriptive filenames)
- [ ] Keyboard shortcuts

### Future Vision (v1.0+)

- [ ] Bundled FFmpeg (no external dependency)
- [ ] macOS and Linux builds
- [ ] Cloud sync for projects
- [ ] Hosted provider hub (premium)
- [ ] Collaborative editing
- [ ] Plugin system for custom adapters
- [ ] LUT/color grading
- [ ] Transitions and effects
- [ ] Basic video transforms (translate, rotate, scale)
- [ ] External asset references (outside project folder)

> **Philosophy:** This is NOT meant to replace a full video editor. If users need fine-grained control, they export their timed/sequenced clips (nicely named!) and bring them into their editor of choice. We stay focused on the AI generation workflow.

---

## 💼 Business Model (Long-term Vision)

### Open Source Core (MIT License)

The desktop application is **open source under MIT**:
- Maximum adoption and community contributions
- Establishes trust with technical users
- Benefits from security and quality auditing

### Monetization Avenues

1. **Premium Hosted Providers**
   - Curated, optimized workflows as a service
   - Users pay for API credits or subscription
   - Zero config—just works

2. **Pro Features (Freemium Model)**
   - Base app free
   - Pro license unlocks: cloud sync, priority support, advanced export codecs

3. **Marketplace**
   - User-contributed workflows and presets
   - Revenue share for creators

4. **Enterprise**
   - Team features, SSO, audit logs
   - Custom provider development

---

## 🛠️ Development Setup

### Prerequisites

| Dependency | Version | Installation |
|------------|---------|--------------|
| Rust | 1.75+ | [rustup.rs](https://rustup.rs) |
| FFmpeg | 6.0+ | [ffmpeg.org](https://ffmpeg.org/download.html) or `winget install ffmpeg` |

### Secrets / API Keys

Provider API keys and secrets are stored in a `.env` file at the project root (git-ignored). Users running locally manage their own `.env`.

```env
# Example .env
FAL_API_KEY=your_fal_key_here
REPLICATE_API_TOKEN=your_replicate_token_here
```

### Getting Started

```bash
# Clone the repo
git clone https://github.com/yourusername/nla-ai-videocreator-rust.git
cd nla-ai-videocreator-rust

# Check in development mode
cargo check

# Build for release
cargo build --release
```

### Project Structure (Proposed)

```
nla-ai-videocreator-rust/
├── Cargo.toml
├── src/
│   ├── main.rs              # App entry point and automation startup
│   ├── egui_app.rs          # eframe/egui desktop shell
│   ├── editor.rs            # Editor model/controller shared by UI and automation
│   ├── state/               # App state management
│   │   ├── mod.rs           # State module root
│   │   ├── app_state.rs     # Global app state
│   │   ├── project.rs       # Project state
│   │   ├── selection.rs     # Selection state (shared across views)
│   │   └── providers.rs     # Provider state
│   ├── providers/           # Provider adapter implementations
│   │   ├── mod.rs           # Provider traits and types
│   │   ├── comfyui.rs       # ComfyUI adapter
│   │   └── fal.rs           # fal.ai adapter
│   ├── core/                # Core logic (non-UI)
│   │   ├── ffmpeg.rs        # FFmpeg wrapper
│   │   ├── audio.rs         # Audio processing
│   │   ├── project_io.rs    # Project save/load
│   │   └── job_queue.rs     # Background task queue
│   └── schema/              # JSON schemas for providers, project files
├── assets/                  # Static assets (icons, fonts)
├── workflows/               # Example ComfyUI workflows
└── docs/                    # Additional documentation
    ├── CONTENT_ARCHITECTURE.md    # Content & generation architecture
    ├── PROVIDER_SETUP_GUIDE.md    # End-user provider setup guide
    ├── PROVIDER_MANIFEST_SCHEMA.md # Provider manifest schema (draft)
    └── PROVIDER_BUILDER_SPEC.md   # Provider builder UX spec (draft)
```

### State Architecture

Inspired by **Blender's multi-view model**:
- **Shared core data** — The project, assets, timeline clips exist once in memory
- **View-specific state** — Each panel (asset browser, timeline, attribute editor) may have its own selection, scroll position, etc.
- **Selection is centralized** — A single selection state that multiple views can observe and modify
- **Modular and flat** — Avoid deep nesting; prefer distinct state slices that can be composed

This allows:
- Asset browser showing the same asset that's on the timeline
- Attribute editor responding to selections from either view
- Multiple views staying in sync without tight coupling

---

## 📋 Decision Log

| Decision | Rationale | Status |
|----------|-----------|--------|
| Use egui/eframe native desktop UI | Immediate-mode UI, native composition, and a single preview/dialog composition path | ✅ Decided |
| FFmpeg on PATH for MVP | Simplifies initial development; bundling is later optimization | ✅ Decided |
| JSON for provider configs | Machine-readable, toolable, familiar | ✅ Decided |
| Project-local asset folders | Portable, self-contained projects | ✅ Decided |
| Folder = Project (KISS) | Simple mental model, easy to backup/share | ✅ Decided |
| Async job queue for generations | Non-blocking UI while waiting for slow API calls | ✅ Decided |
| Single-purpose provider entries | Simpler mental model; add service twice if it does multiple things | ✅ Decided |
| MIT License | Maximum adoption, permissive, standard for tools | ✅ Decided |
| Secrets via .env | Simple, familiar, users manage their own keys | ✅ Decided |
| Lean development philosophy | Build custom, avoid dependency bloat, iterate with user | ✅ Decided |
| Modular state (Blender-inspired) | Multiple views can share/observe same data with their own view state | ✅ Decided |
| Labels separate from filenames | Enables friendly display names + future "Export Parts" feature | ✅ Decided |
| Audio scrubbing is essential | Without hearing audio while scrubbing, the tool is unusable for music-synced work | ✅ Decided |
| UI fluidity is non-negotiable | Hover effects, smooth transitions, polished feel from day one | ✅ Decided |
| Remove the previous UI runtime | eframe now owns the desktop UI; old source modules, config, and dependency were removed | ✅ Decided |
| Transitions disabled during drag | Instant resize feedback; transitions only for collapse/expand | ✅ Decided |
| Use native egui input widgets | Keeps input behavior inside the active desktop UI toolkit | ✅ Decided |
| App-local Timeline State (Temp) | Keep state simple in `editor.rs` until data model requirements mature | ✅ Decided |
| Hierarchical Track Labels | Fixed left column + scrollable timeline area; no browser event synchronization needed | ✅ Decided |
| Draggable Playhead | Real-time updating during drag for immediate feedback | ✅ Decided |
| 1-Second Step Buttons | Frame-stepping felt too slow; 1s steps preferred for navigating | ✅ Decided |
| Frame-snapped Playhead | All seeking snaps to project fps frame boundaries for accurate positioning | ✅ Decided |
| Timeline tick spacing | Major ticks use a "nice" seconds list based on target pixel spacing | ? Decided |
| Playhead-anchored zoom | Zoom in/out keeps the playhead anchored by adjusting timeline scroll | ? Decided |
| Timeline zoom scroll sync | Sync scrollLeft via data attribute + MutationObserver to avoid jitter | ? Decided |
| Timeline scroll jitter guard | Ignore programmatic scrollLeft while user is actively scrolling | ? Decided |
| Marker interactions | Right-click track adds at playhead; context menu delete; drag to move; label/description/color in Attributes | ? Decided |
| Timeline snapping targets | Snap to clip edges first, then playhead, then markers; snap in frame units to avoid gaps | ? Decided |
| Snap preview indicator | Show a 50% opacity snap line at the active target while dragging | ? Decided |
| Snap disable modifier | Hold Alt while dragging to temporarily disable snapping | ? Decided |
| Playhead snapping | Playhead drags snap to clip edges and markers; Alt disables | ? Decided |
| Ctrl+S save hotkey | Ctrl/Cmd+S triggers a project save | ? Decided |
| Spacebar play/pause hotkey | Space toggles timeline playback | ? Decided |
| Timeline-focused play/pause | Spacebar only toggles playback when the timeline has focus | ? Decided |
| Audio logging reduction | Removed audio/perf debug logs; keep warnings/errors only | ? Decided |
| Preview cache LRU compaction | Rebuild LRU queue when it grows too large to avoid unbounded memory | ? Decided |
| Click-to-scrub Interaction | Click anywhere on ruler to seek; playhead follows cursor, not grabbed | ✅ Decided |
| Hierarchical Timeline Layout | Fixed left column + scrollable right column; no JS scroll sync needed | ✅ Decided |
| Playhead as Visual Indicator | Triangle handle is purely visual; interaction is on ruler bar | ✅ Decided |
| Generative Assets as Explicit Creation | Users explicitly create generative assets via UI; they start "hollow" and get populated through generation | ✅ Decided |
| Generative Assets Have Versions | Each generation creates a new version; user picks active version; dependent assets use active version | ✅ Decided |
| Timeline as Implicit Wiring | Overlapping assets auto-surface as input suggestions; no explicit linking required | ✅ Decided |
| Audio stack v1 | Use ffmpeg-next for decode/resample, cpal for playback, and project-local peak cache for waveforms | ✅ Decided |
| Providers Grouped by Output Type | Video/Image/Audio; input requirements vary per provider via dynamic schema | ✅ Decided |
| Provider Bindings via Selectors | Bind workflow inputs by selector/tag instead of node IDs | ✅ Decided |
| Provider Builder UI | Workflow picker + node browser for exposed inputs | ✅ Decided |
| Adapter-Agnostic Manifests | Use a versioned manifest per adapter type for provider bindings | ✅ Decided |
| No Separate Keyframe Track | Images are clips on Video tracks; "keyframes" are just overlapping reference images | ✅ Decided |
| In-Project Assets Only (MVP) | All assets must be in project folder; external refs are future enhancement | ✅ Decided |
| Preview Compositor Strategy | Build robust pixel-buffer compositing for transforms/blending and present through egui textures | ✅ Decided |

---

## 🤝 Contributing

*(To be expanded)*

This project welcomes contributions! Areas where help is especially appreciated:
- Provider adapters for new services
- UI/UX improvements
- Cross-platform testing (macOS, Linux)
- Documentation and tutorials

---

## 📜 License

**MIT License**

This project is open source under the MIT License. See [LICENSE](./LICENSE) for details.

---

## 🗺️ Roadmap

```
v0.1 - Foundation
├── Basic timeline UI
├── Audio import & playback
├── Manual marker placement
├── Keyframe import
├── ComfyUI image generation adapter
└── FFmpeg export

v0.2 - Generation Flow
├── I2V generation pipeline
├── Job queue with progress UI
├── Provider health checks
└── fal.ai adapter

v0.3 - Polish
├── Undo/redo
├── Improved waveform
├── Keyboard shortcuts
└── Marker auto-generation from beats

v0.4 - Multi-platform
├── macOS builds
├── Linux builds
└── Bundled FFmpeg

v1.0 - Public Release
├── Stable API for adapters
├── Documentation
├── Premium hosted providers (beta)
└── Community workflow library
```

---

## 📞 Contact

*(To be filled in)*

---

## 📊 Current Status (2026-05-19)

### Completed ✅
| Area | Status | Notes |
|------|--------|-------|
| **UI Shell** | ✅ Complete | Title bar, panels, timeline, status bar |
| **Panel System** | ✅ Complete | Resizable, collapsible, hover effects |
| **Data Model** | ✅ Complete | Project, Track, Clip, Asset, Marker structs |
| **Project Management** | ✅ Complete | New project dialog, create folder, save/load JSON |
| **Timeline Foundation** | ✅ Complete | Scroll, zoom, playhead, ruler, timecode |
| **Track System** | ✅ Complete | Video/Audio/Marker tracks, add/remove/reorder |
| **Context Menus** | ✅ Complete | Custom right-click menus (delete, move up/down) |
| **Window Config** | ✅ Complete | Custom title, no default menu bar |
| **Asset Panel** | ✅ Complete | Display assets, import files via native dialog |
| **File Copy** | ✅ Complete | Imported assets are copied into the project folder |
| **Video Export** | ✅ First Pass | Modal settings/progress/cancel workflow with MP4 encode and timeline audio mixdown |

### In Progress 🔄
| Area | Status | Next Steps |
|------|--------|------------|
| **Clip System** | � In Progress | Placing clips works, previews next |
| **Thumbnails** | ✅ Complete | Background generation & `nla://` protocol |
| **Preview Engine** | 🟨 In Progress | v0 frame server + compositor wired; next: canvas buffer + caching |
| **Audio Playback** | 🔲 Not Started | Waveform visualization, sync with timeline |

### Code Structure
```
src/
├── main.rs          # Entry point and automation startup
├── egui_app.rs      # eframe/egui desktop shell, panels, modals, preview, timeline
├── ui_kit.rs        # Product-specific egui tokens, frames, buttons, rows, modal helpers
├── editor.rs        # Editor model/controller used by UI and automation
├── constants.rs     # Shared editor constants
├── core/            # Core logic (preview renderer, automation, media helpers)
│   ├── automation.rs # Loopback automation API for desktop scenarios
│   ├── export.rs     # Export-video render, audio mixdown, and FFmpeg encode path
│   ├── preview/     # Preview renderer/cache/layer/util split
│   └── media.rs     # Import/probe helpers
├── state/
│   ├── mod.rs       # Module exports
│   ├── asset.rs     # Asset, AssetKind (file & generative)
│   └── project/     # Project, Track, Clip, Marker, save/load split into modules
└── scripts/         # Runtime DLL staging, desktop smoke, automation harness
```

### Recent Changes (Session Log)
- **2026-05-22:** Added a shared weighted field-grid row helper to `ui_kit` and moved the export modal's grouped fields onto it. Export video fields now use equal thirds, codec/quality uses a one-third/two-thirds split, and range uses an even split instead of modal-local row sizing.
- **2026-05-22:** Fixed the export settings card layout so bounded field rows no longer consume the remaining card height. Codec, perceptual quality, range, audio, and timestamp options now live inside the card's clipped scroll body instead of being pushed below the visible modal area.
- **2026-05-22:** Made modal outside-click dismissal a reusable scrim behavior. Any modal with an enabled top-right close button now treats clicking the dimmed background as the same close/cancel action, the scrim is exposed through `/ui` as a real clickable element for automation, and startup remains non-dismissible until a project is created or opened.
- **2026-05-22:** Tightened the shared combo-field automation registration so dropdown widgets expose their real egui combo response through `/ui`, which keeps automation IDs closer to the native widget handling path.
- **2026-05-22:** Extended the export-video modal with H.264/H.265 codec selection, perceptual quality presets mapped to FFmpeg CRF values per codec, and an optional burned-in timestamp overlay on a clean black plate at the top or bottom center of rendered frames.
- **2026-05-22:** Added a standard File -> Quit menu item that closes the eframe viewport through the native app command path and is registered with the UI automation layer.
- **2026-05-21:** Added the first export-video workflow. File → Export Video opens a polished blocking modal with output path, resolution, FPS, range, quality, include-audio, progress bar, rendered preview thumbnail, cancel handling, and completion/error states. The core export path reuses the existing preview compositor for raw RGBA frames, mixes timeline audio/video clip audio into a temporary WAV, and invokes FFmpeg to write MP4/H.264 + AAC output.
- **2026-05-21:** Extended the automation/UI-kit surface for export validation. Labeled shared text fields now expose their labels through `/ui`, the export modal can be opened by automation, and smoke validation rendered `projects/test` to a 2.000s 640×360 MP4 with video and audio streams plus a separate cancel-path check that leaves no partial output.
- **2026-05-21:** Added a Rust-native UI-level automation layer on top of the existing loopback harness. Shared egui kit widgets now register a current-frame `/ui` control registry, `/ui/click` and `/ui/text` queue actions for real widgets to consume during the normal render pass, and `/screenshot` captures the application viewport directly to `.tmp/automation-screenshots/` through eframe's native screenshot command.
- **2026-05-21:** Restored the Add Assets/New Generative card height budget so the media-type buttons have a normal bottom gutter and no longer clip against the card edge in the Assets panel.
- **2026-05-21:** Rebuilt the egui Generation Queue as a custom anchored popover instead of a stock window. The queue now closes from its scrim, scales vertically with job count up to the available app height, keeps Clear All/Close controls in a compact header, shows Dioxus-style status pills plus Workflow/Node progress bars, and uses an active-count QUE badge with a slow orange pulse without auto-opening the popover when jobs are enqueued.
- **2026-05-21:** Updated the SDXL Vanilla API workflow checkpoint reference to ComfyUI's current nested checkpoint key `sdxl\juggernautXL_juggXIByRundiffusion.safetensors`. ComfyUI validation was rejecting the old bare filename after the checkpoint moved under the `sdxl` folder.
- **2026-05-21:** Hardened the provider JSON editor modal so the code editor is an exact clipped scroll viewport with a pinned footer. The JSON text now scrolls inside its field surface instead of painting or selecting underneath the Cancel/Save row.
- **2026-05-21:** Reworked the Generation Queue surface as a blocking anchored popover. The Queue button records its current screen rect every frame, the queue window is fixed just below/right-aligned to that rect after resizes, and the shared modal scrim now catches input so background panels cannot be interacted with while modal-style overlays are open.
- **2026-05-21:** Hardened the shared egui vertical scroll helper so inspector/provider/modal scroll regions allocate a fixed clipped viewport, cap horizontal width, disable scroll-edge fades in editor panes, and clamp inner content width. Replaced child `set_clip_rect` calls with inherited `shrink_clip_rect` clipping so scrolled-off inspector fields cannot paint above or below the Attributes viewport.
- **2026-05-21:** Reconnected the egui generative-image inspector for selected timeline clips. Version/provider selection, batch seed controls, schema-driven provider inputs, reusable combo-field styling, and the ComfyUI image generation queue now update generative config/version state through the shared project model.
- **2026-05-21:** Added asset-to-timeline drag/drop for the egui editor. Asset rows now act as drag sources, compatible timeline tracks highlight as drop targets, dropping creates a new clip instance on the target track at the frame-snapped drop time, and timeline clip titles now prefer the per-instance clip label when present before falling back to the source asset name.
- **2026-05-21:** Removed the baked one-pixel plate border from egui preview frame compositing. The preview texture now presents only rendered canvas pixels; any future preview outline should be drawn in UI space consistently on all four sides.
- **2026-05-21:** Simplified the Preview body surface by removing the extra stroked intermediary plate around the rendered texture. The preview panel now uses the body fill as the stage and clips the scaled preview canvas directly inside the padded body rect.
- **2026-05-21:** Normalized top-level shell borders so adjacent app panels no longer double up one-pixel strokes. Chrome, dock, collapsed dock, and timeline frames are now fill/padding surfaces; the app paints explicit single-edge separators for top bar, status bar, side docks, and timeline boundaries.
- **2026-05-21:** Normalized AI Provider and Provider Builder node tiles onto the same painter-based button-row behavior used by asset rows. Provider rows no longer embed selectable labels, the whole row now exposes one pointing-hand click target, and row text is clipped/truncated inside the tile instead of changing cursor behavior over the text.
- **2026-05-21:** Hardened scroll-pane containment for modal/provider layouts by making the shared `scroll_body` helper allocate a clipped exact viewport before rendering `ScrollArea` contents. The Provider Builder columns now sit inside a bounded body rect, the right settings/input list no longer paints behind the footer, and egui's default scroll-edge fade gradients are disabled globally for cleaner recessed editor panes.
- **2026-05-21:** Normalized timeline header transport/collapse controls with the rest of the timeline toolbar. Transport buttons now use the same 20px vertical metric as Fit/Frames and render centered geometric play/pause/step/caret icons instead of font glyphs, eliminating baseline drift and bottom clipping.
- **2026-05-21:** Consolidated duplicate shell readouts: the top bar no longer repeats the loaded project name beside QUE, the Preview header now shows only preview resolution, the timeline remains the single timecode surface, and the bottom status bar now carries status plus project name on the left and FPS on the right.
- **2026-05-21:** Moved the app top-bar menu triggers and queue chip onto the shared subtle button substrate used by the timeline transport controls. File/Edit/View/Settings/Help and QUE now stay transparent until hover/open/active states while preserving the existing popup menu contents.
- **2026-05-21:** Added a reusable field-pair layout primitive for narrow inspector/form rows. Paired fields can now choose whether to enforce a minimum column width; the Attributes inspector opts into shrink-to-fit behavior so field boxes stay inside the card and only the inner label/value text clips when the right pane gets very narrow.
- **2026-05-21:** Moved timeline playback-audio decode warmup earlier in the project-open path. Opening a project now schedules decode/cache work for timeline audio/video sources immediately and services that background work with short repaint ticks, so the first scrub should not be the operation that pays the initial 1-2 second audio decode cost.
- **2026-05-21:** Refined the shared timeline header layout so collapsed mode no longer draws a body separator, transport controls are vertically centered in an exact button-height region, and the transport group centers between the left/right control groups that are actually visible in the current expanded/collapsed state.
- **2026-05-21:** Rebuilt the egui AI Providers flow toward the Dioxus V2 reference: the provider list now uses equal-width New/Reload controls plus a delete footer, provider selection opens an editor-choice hub instead of a cramped JSON preview, Edit as JSON launches a wide dedicated JSON editor with validation, and Edit in Builder launches a wide ComfyUI builder that can pick workflow/manifest JSON, browse workflow nodes, expose inputs, choose an output node, and save provider/manifest files.
- **2026-05-21:** Cleared the current build warning set: migrated egui shell panels to the 0.34 `Panel::{top,bottom,left,right}.show_inside(...)` API, made the collapsed timeline reuse the expanded timeline header chrome with zoom-only controls hidden, and documented intentionally dormant provider/preview/thumbnail paths with narrow `dead_code` allowances instead of leaving noisy warnings.
- **2026-05-20:** Refined timeline and pane chrome toward the Dioxus reference: installed Segoe UI/Symbol as the egui font fallback, added reusable subtle timeline toolbar/transport buttons that stay transparent until hover/active states, padded the Preview header metadata, and replaced collapsed side-panel word rails with full-height click targets plus inverse arrow controls.
- **2026-05-20:** Upgraded marker inspector fields: marker color now uses a reusable field-height swatch with egui's native popup color picker while continuing to persist project colors as hex strings, and marker descriptions now use a reusable configurable multi-line text-field primitive instead of a single-line field.
- **2026-05-20:** Normalized timeline snapping and item semantics: Alt now bypasses clip, marker, and playhead boundary snapping; ruler scrubbing snaps to clip/marker boundaries by default; marker hit targets now include the visible label/handle area for immediate drag; and video/image accents moved away from selection green while unselected timeline items use neutral fills with subdued type outlines.
- **2026-05-20:** Fixed timeline clip/marker move and resize persistence by switching drag math from per-frame `drag_delta()` to cumulative `total_drag_delta()` when applying movement against the drag-start baseline.
- **2026-05-20:** Rebalanced the egui timeline header into explicit left/center/right regions so transport controls are centered between zoom tools and the right-side timecode/collapse controls instead of flowing directly after the zoom buttons.
- **2026-05-20:** Corrected the egui root panel hierarchy so the status bar claims the full application width before side panels and the timeline are registered; this matches the legacy Dioxus shell where side panels sit above the app-wide bottom bar.
- **2026-05-20:** Corrected Shift+wheel timeline scrolling direction so a downward wheel gesture advances the timeline viewport to the right/later time instead of moving left.
- **2026-05-20:** Stopped playback audio decode retry spam for silent video assets by adding a per-project playback decode failure cache. Video files without audio streams are now logged once and skipped for playback audio until the project cache is reset, while valid audio clips continue to decode and play normally.
- **2026-05-20:** Tightened the egui timeline structure after visual review: the fixed track-label rail and scrollable ruler/track canvas now paint through separate clipped regions so clips, ticks, markers, waveforms, thumbnails, and playhead overlays cannot bleed over labels; visual clip strips now sample cached thumbnails at each tile's clip time instead of repeating the first frame; Shift+wheel accepts both raw shifted wheel and horizontal trackpad deltas; and playback/scrubbing now routes through the cpal audio engine with UI-thread-owned engine updates and background decode into the shared sample cache.
- **2026-05-20:** Began the egui timeline parity pass against the Dioxus snapshot: timeline geometry now uses pixels-per-second plus horizontal scroll instead of stretching duration to viewport width, the header restores zoom/fit/frames/playback/timecode controls, ruler ticks include high-zoom frame ticks, clips can be selected/moved/resized with edge hit-testing, marker/playhead styling is closer to the legacy timeline, visual clips render thumbnail strips, and audio clips load/build peak caches for waveform drawing.
- **2026-05-20:** Corrected Add Assets card height by rendering it as a manual stack with implicit vertical item spacing disabled; the card height now comes from shared label/button/gap tokens instead of a magic number.
- **2026-05-20:** Added a reusable exact-rect equal media-pill row so the Add Assets Video/Image/Audio controls compute responsive widths from the available card space instead of overflowing narrow side panels.
- **2026-05-20:** Fixed resizable side-panel creep by routing Assets and Attributes contents through an exact-size `fixed_panel_body` viewport and moving the Add Assets section to an exact-height card allocation, preventing child content width from feeding back into egui's stored panel size.
- **2026-05-20:** Updated `AGENTS.md` build workflow so future code/UI handoffs run `cargo check` and then attempt `cargo build --release`, while reporting a locked running executable instead of switching build targets.
- **2026-05-20:** Removed side-panel width feedback that could make resizable panes creep wider over time, and added an empty-space click target in the Assets list so clicking outside asset rows clears the current selection.
- **2026-05-20:** Stabilized resizable side-panel content by constraining full-width card children; thumbnail image painting now uses an internal inset so square images do not poke through rounded thumbnail frames.
- **2026-05-20:** Consolidated the Assets rail creation controls into one `Add Assets` card, grouping file import and generative asset creation above the scrollable project-asset list with normal shared spacing.
- **2026-05-20:** Refined the left Assets panel: New Generative now fills the panel width with equal-width media buttons, import-to-card spacing uses the shared form gap, asset rows use larger vertically centered thumbnail boxes, and row text is left-aligned with top/bottom anchoring against the thumbnail.
- **2026-05-20:** Began the main-window polish pass after the project-modal work: asset rows now use thumbnail-aware reusable painting, selected asset/clip/marker/track inspectors use shared cards and field grids, marker metadata is editable in the inspector, and project-open/create paths clear stale preview/thumbnail caches.
- **2026-05-20:** Updated the egui best-practices guide with the latest UI-kit lessons: tokenized control families, field/button/browse behavior rules, modal corner painting rules, faux-backdrop guidance, and a stronger visual QA checklist.
- **2026-05-20:** Fixed modal corner artifacts globally by giving modal headers top-only radii and modal bodies bottom-only radii, matching the rounded outer frame instead of relying on egui parent clipping.
- **2026-05-20:** Added reusable faux-blur modal backdrop styling: layered tinted scrim, subtle edge vignette, and tokenized modal drop shadow so dialogs feel separated from the editor without a true offscreen blur pass.
- **2026-05-20:** Rebuilt modal close buttons as square tokenized controls with a larger hit area, coherent top/right header insets, hover/focus states, and a stroked X icon instead of tiny text.
- **2026-05-20:** Forced editable single-line text fields onto the same exact `FIELD_H` allocation and shared field frame tokens as value fields and browse rows, removing egui natural-height drift from Project Name and other text fields.
- **2026-05-20:** Normalized standalone button metrics in `ui_kit` so primary/action, secondary, and danger buttons share one height, radius, and text-size token family; green actions now differ by color skin rather than shape.
- **2026-05-20:** Added reusable native file/folder browse helpers with shared field styling, optional extension filters for file dialogs, stable widget IDs, and reset-vs-remember starting-directory behavior; moved project open/save and asset import dialogs onto the shared helpers.
- **2026-05-20:** Added shared first-focus select-all behavior for single-line text fields and made read-only value fields selectable/focusable with the same field edge highlighting.
- **2026-05-20:** Aligned numeric `DragValue` fields with text-field surfaces by styling egui button `weak_bg_fill` as well as `bg_fill`, and moved regular text fields onto the same focused green outline path.
- **2026-05-20:** Softened the shared field surface palette away from near-black, added hover/active field fill tokens plus `TEXT_ON_ACCENT`, and documented the no pure black/white UI primitive rule in the egui best-practices notes.
- **2026-05-20:** Tightened field compound layout so browse rows reserve exactly one field-height row, use a single field-to-button gap token, and share one global field text-alignment token across editable and read-only fields.
- **2026-05-20:** Normalized New Project and inspector field styling through shared `ui_kit` tokens: text fields, read-only browse fields, field-height Browse buttons, and inline numeric drag prefixes now use the same field metrics, colors, and radius.
- **2026-05-20:** Added `egui_extras` and moved the New Project modal columns onto a shared StripBuilder body/footer template, including reusable labeled text-field and browse-value row helpers whose field flexes while the Browse button keeps a fixed width.
- **2026-05-20:** Refined the New Project footer template so bottom-up layout pins a single fixed footer region, while the Save Location row and Create Project action are laid out top-down inside that region for reliable spacing.
- **2026-05-20:** Added reusable egui scroll-body-above-footer and bottom-pinned-region layout helpers and moved the New Project create form onto them so the form area flexes while the Save Location/Create Project footer stays bottom-pinned in normal egui flow.
- **2026-05-20:** Made the project wizard size itself to the available viewport and split the New Project card into a scrollable form region plus a pinned footer so the Save Location row and Create Project button cannot overflow the card.
- **2026-05-20:** Replaced the project wizard left footer's bottom-up layout with an explicit top-down footer block so the Save Location row and Create Project button have consistent row spacing.
- **2026-05-20:** Matched the project wizard footer gap between Save Location and Create Project to the modal's normal 8px row spacing.
- **2026-05-20:** Removed the fixed-height scroll block from the project wizard form and bottom-aligned the Save Location/Create Project footer so the primary action stays on the same bottom flow as Browse for Project.
- **2026-05-20:** Made resolution preset badges derive their green selected state from the current width/height values, and changed the 1:1 preset to set 512x512.
- **2026-05-20:** Enlarged the project wizard by 40px vertically and changed its content split to roughly 2/3 create-project form and 1/3 recent-projects list so the primary form and Create Project footer have more room.
- **2026-05-20:** Applied the shared modal-header close affordance across Project Settings, Generative Video, AI Providers, and Generation Queue; startup remains intentionally non-closeable until a project is opened or created.
- **2026-05-20:** Fixed reopened New Project modal behavior so selecting a recent project or browsing to a project closes the modal after a successful open.
- **2026-05-20:** Added a reusable modal-header close affordance and gated the New Project close button so it only appears when an existing project is already open; startup still requires creating or opening a project.
- **2026-05-20:** Reworked the reusable numeric field row template to allocate exact paired column rectangles, eliminating the subtle right-column vertical offset in project settings and inspector grids.
- **2026-05-20:** Added a shared vertically centered single-line text-field helper and routed project, asset, track, and inspector text inputs through it for consistent field typography.
- **2026-05-20:** Moved create-dialog polish into reusable egui templates: shared button painting now provides visible hover/press/focus states, and a read-only value box keeps path/action rows from clipping in the project wizard footer.
- **2026-05-20:** Tightened the egui polish pass: project wizard cards now share a fixed height, modal headers span the full modal width, inspector fields use stable labeled grids, asset rows truncate long names, timeline scrubbing supports drag, and the timeline panel is vertically resizable.
- **2026-05-19:** Added the reviewed egui UI/UX rebuild checklist to `docs/EGUI_UI_UX_REBUILD_PLAN.md`, introduced a reusable app UI kit, restyled the shell panels/modals/assets/inspector/timeline/preview/provider surfaces, and captured the final app-window reference set under `.tmp/desktop-smoke/egui-ui-rebuild-final/`.
- **2026-05-19:** Scrubbed remaining legacy UI framework residue from tracked docs/tooling, removed obsolete status/refactor docs and stale desktop config, and preserved old screenshots under `.tmp/desktop-smoke/legacy-ui-reference-20260519-173555/`.
- **2026-05-19:** Replaced the desktop runtime shell with an egui/eframe implementation (`src/egui_app.rs`) backed by a shared editor controller (`src/editor.rs`).
- **2026-05-19:** Removed the old UI runtime modules, stale desktop config, abandoned child-window preview path, and obsolete implementation-status docs; `Cargo.toml` now uses `eframe`.
- **2026-05-19:** Captured the egui review reference set under `.tmp/desktop-smoke/egui-reference-ready/` with startup, timeline selections, modals, queue, providers, collapsed panels, and preview stats.
- **2026-05-19:** Added an egui automation repaint heartbeat so loopback API commands are processed even when the immediate-mode UI is otherwise idle.
- **2026-05-19:** Expanded automation/reference capture to cover selection variants, project/new/generative modals, queue, providers, collapsed panels, preview stats, and tighter app-window capture bounds.
- **2026-05-19:** Captured the legacy UI reference set under `.tmp/desktop-smoke/legacy-ui-reference-20260519-173555/` before beginning the egui migration.
- **2026-05-19:** Added loopback desktop automation mode (`--automation`) with semantic commands for create/open project, import asset, add clip, seek, select, marker, save, and providers modal open/close.
- **2026-05-19:** Added `scripts/automation-scenario.ps1` to drive a project/import/timeline/modal slice on the right-most monitor and save app-window-only screenshots plus state JSON.
- **2026-05-19:** Added desktop smoke harness documentation plus scripts for FFmpeg runtime DLL staging and launch/screenshot verification.
- **2026-05-19:** Diagnosed direct-launch DLL popups as missing staged vcpkg FFmpeg runtime DLLs (`avcodec-61`, `avformat-61`, `avutil-59`, `swresample-5`, `swscale-8`).
- **2026-01-13:** Added Asset Config controls in the Attributes panel for editing generative video FPS + frame count.
- **2026-01-13:** Suspended the native preview while the generative video creation modal is open.
- **2026-01-13:** Generative video assets now require FPS + frame count on creation, and preview playback retimes to fill the declared asset duration.
- **2026-01-13:** Added ComfyUI queue support for video outputs, including output selection fallback by file type.
- **2026-01-12:** Removed Stats/HW quick toggles from the title bar (menus remain).
- **2026-01-12:** Strengthened dimming backdrops and added subtle blur for queue popover and title-bar menus.
- **2026-01-12:** Queue popover now closes on outside click.
- **2026-01-12:** Queue job labels no longer include batch index suffixes.
- **2026-01-12:** Queue job labels now omit active-version suffixes to avoid confusion while generating.
- **2026-01-12:** Removed version labels from the queue to avoid misleading planned versions.
- **2026-01-12:** Added a Manage Versions menu with delete current/others/all actions and confirmations.
- **2026-01-12:** Added a Clear All action in the generation queue to purge queued/completed jobs quickly.
- **2026-01-12:** Wired ComfyUI WebSocket progress events into the generation queue with workflow + node progress bars.
- **2026-01-12:** Added batch generation controls (count + seed strategy/field) with seed auto-detection and multi-job enqueueing.
- **2026-01-12:** Improved ComfyUI missing-output messaging to point at cached results and seed offsets.
- **2026-01-12:** Provider Builder V2 now resets to defaults on new-provider opens so stale edit state doesn't linger.
- **2026-01-12:** Provider Builder V2 exposed inputs can now be reordered with Up/Down controls.
- **2026-01-12:** Provider Builder V2 exposed inputs use stable IDs so reorder keeps field values aligned.
- **2026-01-11:** Updated AI guidelines to run `cargo check` (not `cargo test`) before yielding back.
- **2026-01-11:** Provider Builder V2 restores input type, default, required, multiline, and enum option controls for exposed inputs.
- **2026-01-11:** Removed unused v1 provider builder module wiring and an unused manifest helper to keep builds warning-free.
- **2026-01-11:** Fixed StableTextArea handler type inference and cleaned up unused v1 component re-exports/variables after the cursor-safe input refactor.
- **2026-01-11:** Refactored remaining text/number/textarea inputs to use Stable* cursor-safe components and extended Stable* inputs with blur/focus/keydown hooks plus optional rows/autofocus.
- **2026-01-09:** Provider Build now flushes the JSON editor draft into app state before opening the builder (prevents empty/new mode when text is visible).
- **2026-01-09:** Provider Builder uses editor JSON as a fallback to enter edit mode when selection path is missing.
- **2026-01-09:** Provider Builder now reads the selected provider file from disk when opening (avoids stale/empty editor state).
- **2026-01-09:** Providers modal now opens with no selection to force explicit picks (avoids stale editor state).
- **2026-01-09:** Provider list selection now updates editor state directly to avoid empty JSON/editor desync after restart.
- **2026-01-09:** Provider list selection now explicitly loads JSON into the editor; Reload clears selection to force a fresh pick.
- **2026-01-09:** Provider JSON editor keeps text synced on input; Provider Builder preserves existing provider IDs when editing.
- **2026-01-09:** Fixed textarea draft buffering ownership issues after test failures (compile clean again).
- **2026-01-09:** Provider Builder now derives a manifest path from the workflow when missing (legacy provider fallback).
- **2026-01-09:** Textarea inputs now track local draft text to avoid caret jumps and stop clearing on focus (provider inputs + providers JSON editor).
- **2026-01-09:** Asset rows now start drag from the entire row (text included), not just the icon.
- **2026-01-09:** Multiline textareas stop resetting the caret to the end while typing (provider inputs + providers editor).
- **2026-01-09:** Generative configs now load once into project memory and UI edits write through the project state (no disk reads on selection); config files persist on save/generate.
- **2026-01-09:** Removed generation-record backcompat defaults (provider IDs now required in version records) and moved workflow/cache roots out of `projects/`.
- **2026-01-09:** Removed asset-level provider fields and centralized provider/version metadata in generative configs.
- **2026-01-09:** Removed unused preview default constants and the unused renderer constructor to keep the build warning-free.
- **2026-01-09:** Added `PartialEq` to ProjectSettings so the project settings modal can accept optional settings props.
- **2026-01-09:** Added a project settings edit flow (reusing the startup modal UI) with a File → Project Settings entry.
- **2026-01-09:** Preview renderer now honors per-project preview downsample limits (configurable in project settings).
- **2026-01-09:** Queue badge stays orange when active and shifts to gray only when paused.
- **2026-01-09:** Queue badge now uses a neutral gray and the QUE toggle shimmers while jobs run.
- **2026-01-09:** Generative config saves are atomic to prevent versions/provider state from being clobbered.
- **2026-01-09:** Queue now retries offline providers once after 5s, then pauses with a resume action.
- **2026-01-09:** Timeline zoom no longer auto-clamps on window resize (zoom stays stateful).
- **2026-01-09:** Preview GPU device now requests adapter max texture limits (with downlevel fallback).
- **2026-01-09:** Preview GPU size guard now uses device limits (prevents oversize Surface::configure panics).
- **2026-01-09:** Native preview now falls back to the canvas when the GPU surface exceeds device texture limits.
- **2026-01-09:** Queue panel now shows newest jobs at the top.
- **2026-01-09:** Added a ComfyUI health check before enqueueing generation jobs (blocks offline providers).
- **2026-01-09:** Added a queue-item context menu to remove queued/completed jobs.
- **2026-01-09:** Generative config saves now reload from disk to avoid overwriting versions/provider state.
- **2026-01-09:** Queue processing now reacts to enqueue events (fixes jobs stuck in queued state).
- **2026-01-09:** Queue panel now suspends the native preview overlay to avoid being obscured.
- **2026-01-09:** Added a generative job queue with sequential execution and per-job status tracking.
- **2026-01-09:** Added a top-bar QUE toggle with badge counts and a queue panel overlay for pending/completed jobs.
- **2026-01-09:** Generation now queues jobs and refreshes generative configs on completion (no blocking).
- **2026-01-08:** Marked file copy into project folders as complete (import pipeline already copies assets).
- **2026-01-08:** Provider input fields now remount on version switch to reflect saved input snapshots immediately.
- **2026-01-08:** Switching generative versions now restores the saved inputs (and provider) from that version’s snapshot.
- **2026-01-08:** Added multiline text inputs (builder toggle + textarea rendering) for provider inputs.
- **2026-01-08:** Output key placeholder now follows the selected output type in the Provider Builder.
- **2026-01-08:** Added a dynamic output key hint in the Provider Builder Output tab.
- **2026-01-08:** Split Provider Builder into Inputs/Output tabs with a three-column layout (browser, inspector, config).
- **2026-01-08:** Clicking the provider list background now clears selection (same as clicking the selected item).
- **2026-01-08:** Providers modal now supports deselection and updates the Build button label based on selection.
- **2026-01-08:** Hid tag fields in the Provider Builder UI; tagging is now a documented TODO.
- **2026-01-08:** Provider Builder now opens in edit mode for selected ComfyUI providers and preloads manifests/workflows.
- **2026-01-08:** Wired the ComfyUI adapter to consume provider manifests (selector-based input/output binding with optional tags).
- **2026-01-08:** Updated provider docs and content architecture to reflect manifest-based ComfyUI binding and output selection rules.
- **2026-01-08:** Added Provider Builder modal scaffold (workflow picker, node browser, exposed inputs, save).
- **2026-01-08:** Added provider manifest types in state and wired builder saves to manifest + provider entries.
- **2026-01-08:** Added ComfyUI workflow parser to power the builder node browser.
- **2026-01-08:** Added provider manifest schema + provider builder spec docs; refreshed setup guide for multi-adapter roadmap.
- **2026-01-08:** Added example ComfyUI manifest `workflows/sdxl_simple_example_manifest.json`.
- **2026-01-08:** Expanded provider architecture doc with ComfyUI workflow picker + node binding UI details.
- **2026-01-08:** Revised provider architecture doc to use selector/tag bindings and a provider builder UI (no node ID reliance).
- **2026-01-08:** Added end-user provider setup guide `docs/PROVIDER_SETUP_GUIDE.md` covering ComfyUI workflow setup and provider JSON.
- **2026-01-08:** Extracted Providers modal, New Project modal, and track context menu into smaller UI modules.
- **2026-01-08:** Split Attributes panel UI into `generative_controls` and `provider_inputs` helpers.
- **2026-01-08:** Fixed native preview Y-flip by inverting V coordinates in the GPU preview shader.
- **2026-01-08:** Split GPU preview into `src/core/preview_gpu/` (surface, shaders, types, layers) and cleaned up module boundaries.
- **2026-01-08:** Split preview renderer into `src/core/preview/` (renderer, cache, layers, types, utils) with explicit re-exports.
- **2026-01-08:** Silenced the unused `Project::save_project_as` warning via an explicit allow annotation.
- **2026-01-08:** Split project state into `src/state/project/` (project, track, clip, marker, settings, persistence) with `mod.rs` re-exports.
- **2026-01-08:** Split the timeline module into `src/timeline/` (panel, ruler, playback controls, track label/row, clip element) with `mod.rs` re-exports and shared constants.
- **2026-01-08:** Relocated helper functions into `core/` and `state/` modules during the earlier UI modularization pass.
- **2026-01-08:** Split Attributes and Assets panels into smaller modules; relocated provider/media/generative helpers into `core/` and `state/`, and moved timeline zoom bounds into the timeline module.
- **2026-01-08:** Began the earlier modularization pass by extracting shared UI constants, startup modal, panel modules, and shared input fields.
- **2026-01-08:** Generative thumbnail cache now clears when no active version exists (prevents stale thumbnails after deleting all versions)
- **2026-01-08:** Preview cache now invalidates generative asset folders on generate/delete so regenerated versions update immediately
- **2026-01-08:** Version dropdown now lists newest first; added inline delete confirmation that keeps selection position
- **2026-01-08:** Split Generative controls into two cards: Generative (version/provider/generate) and Provider Inputs (dynamic fields)
- **2026-01-08:** Asset context menus now clamp to the Assets panel width so they don't get hidden by the native preview overlay
- **2026-01-08:** Attributes panel now remounts on clip selection to refresh fields when switching clips
- **2026-01-12:** Locked the audio timeline plan (cpal playback, ffmpeg decode, project-local peaks, background cache rebuild).
- **2026-01-12:** Added audio core scaffolding under `src/core/audio/` and exported the module.
- **2026-01-12:** Added ffmpeg-based audio decode + resample helpers (f32 stereo output).
- **2026-01-12:** Added cpal-based playback engine scaffolding with a basic mixer + audio clock.
- **2026-01-12:** Added waveform peak cache format + background builder for audio (project-local).
- **2026-01-12:** Added timeline waveform rendering for audio clips with on-demand cache build + refresh.
- **2026-01-12:** Added waveform mount/cache debug logs and explicit SVG width/height for waveform coordinate alignment.
- **2026-01-12:** Added one-time ClipElement render debug logging to confirm audio clip wiring when waveform is missing.
- **2026-01-12:** Switched waveform cache loading to a synchronous render-time path keyed off the cache buster to bypass missing effect callbacks.
- **2026-01-12:** Replaced per-line SVG waveform rendering with a cached single-path build and added perf timing logs for waveform path generation.
- **2026-01-12:** Waveform generation now computes per-column min/max from base peaks and logs SVG vs bitmap build timings for perf comparison.
- **2026-01-12:** Waveforms now render from cached BMP strips under `.cache/audio/waveform_strips/` (disk-backed, uncompressed).
- **2026-01-12:** Added a waveform strip width cap and softened waveform opacity/brightness for readability.
- **2026-01-12:** Wired cpal playback into timeline controls with audio-clock playhead sync and on-demand decode caching.
- **2026-01-12:** Added cpal output format fallback (mixes in f32, converts to device sample format).
- **2026-01-12:** Preferred stereo output configs when available and allowed multi-channel decode buffers.
- **2026-01-12:** Fixed packed f32 audio extraction to use full interleaved buffer (avoids accelerated playback).
- **2026-01-12:** Scrubbing now pauses playback while dragging and resumes on release, with audio preview during scrub.
- **2026-01-12:** Audio decode now prewarms on project load/clip add and avoids blocking UI on first play.
- **2026-01-12:** Scrubbing now holds the audio playhead until the cursor moves (prevents runaway audio while dragging).
- **2026-01-12:** Video clips now contribute their embedded audio during playback.
- **2026-01-12:** Fixed packed f32 decode slicing to avoid padding noise in multi-channel output.
- **2026-01-12:** Added clip + track volume controls (Attributes panel) and applied gains in the mixer.
- **2026-01-12:** Volume fields now update live on change so playback reflects edits without pausing.
- **2026-01-12:** Defaulted clip/track volumes to 1.0 on load for older projects.
- **2026-01-12:** Removed obsolete Provider UI v1 modal/builder components and scrubbed stale debug logging
- **2026-01-08:** Added generative version selector in Attributes panel; changing active version refreshes thumbnails and preview
- **2026-01-08:** Added per-clip labels in Attributes panel; timeline labels now respect clip names and show active generative version
- **2026-01-08:** Generative assets now default to sequential names (Gen Image 1, Gen Video 2) and asset list titles include active version
- **2026-01-07:** Implemented centralized hotkey system (`src/hotkeys/`) with action-based architecture and context awareness
- **2026-01-07:** Added global hotkeys for Timeline Zoom (+/- on Numpad and standard keys)
- **2026-01-07:** Attributes panel now shows provider picker for generative clips and saves provider selection to config.json
- **2026-01-07:** Provider entries now load from the global providers folder and display their input schema
- **2026-01-07:** Added a global Providers JSON editor modal (top bar) for quick provider edits
- **2026-01-07:** Providers modal now renders at the app root and messaging reflects global-only config
- **2026-01-07:** Native preview overlay now hides while modal dialogs are open (prevents WGPU surface covering UI)
- **2026-01-07:** Added ComfyUI workflow template `workflows/sdxl_simple_example_API.json` and optional `workflow_path` on ComfyUI providers
- **2026-01-07:** ComfyUI adapter now submits API workflows, polls history, and downloads image outputs
- **2026-01-07:** Generative attributes now render dynamic input fields, generate button, and status line
- **2026-01-07:** Generative output now writes versioned files, updates config + active version, and refreshes thumbnails
- **2026-01-07:** Thumbnailer now supports generative image/video assets by resolving active versions
- **2026-01-07:** Still image thumbnails now render via the image crate (covers regular + generative images)
- **2026-01-07:** Project load now syncs generative asset config.json and active version state
- **2026-01-07:** Generative asset creation now writes a default config.json in the asset folder
- **2026-01-07:** Added provider + generative config data models and storage helpers (config.json + global providers)
- **2026-01-07:** Expanded the Generative/Provider/Generation TODOs into an atomic ComfyUI MVP plan
- **2026-01-07:** Defaulted timeline zoom to Fit on project open/create once the viewport width is known
- **2026-01-07:** Added timeline zoom bounds based on visible width, plus Fit/Frames buttons and adaptive minimum clip width
- **2026-01-07:** Distinguished preview plate vs background by clearing the GPU surface to the UI background, adding a black plate fill, and drawing a 1px plate border
- **2026-01-07:** Fixed native-size preview scaling by tracking source dimensions alongside cached frames
- **2026-01-07:** Preview now renders clips at native size (no auto-fit scaling to canvas)
- **2026-01-07:** Fixed GPU rotation skew by accounting for preview aspect ratio in the shader
- **2026-01-07:** Added CPU fallback rotation support using imageproc (GPU + CPU paths now respect clip rotation)
- **2026-01-07:** Added idle-time prefetch (800ms delay, 5s ahead + 1s behind) to warm the preview cache when not playing
- **2026-01-07:** GPU preview now applies clip rotation (rotation degrees respected during compositing)
- **2026-01-07:** Added a title-bar "HW Dec" toggle to force CPU decode for A/B comparisons
- **2026-01-07:** Defaulted preview stats toggle to off (still available via the header toggle)
- **2026-01-07:** Moved preview stats into a docked right-side column so they stay visible above the native wgpu surface
- **2026-01-07:** Expanded preview stats into a vertical overlay with detailed video decode breakdown (seek, packet, transfer, scale, copy)
- **2026-01-07:** Expanded playback prefetch window to 3 seconds to improve sustained playback responsiveness
- **2026-01-07:** Increased preview frame cache budget to 8GB for smoother scrubbing in larger regions
- **2026-01-07:** Added parallel decode scheduling (worker pool keyed by track lanes) to allow per-layer decoding concurrently
- **2026-01-07:** Added hardware-accelerated decode support (Windows D3D11VA/DXVA2) with automatic CPU fallback
- **2026-01-07:** Preview stats now include a `hwdec` percentage to indicate hardware decode usage
- **2026-01-07:** Added a `SHOW_CACHE_TICKS` toggle to enable/disable the timeline cache bucket overlay
- **2026-01-07:** Cache tick overlay now uses a per-asset frame index to mark buckets based on any cached frame in the clip range (stills fill all buckets once cached)
- **2026-01-07:** Added per-clip cache tick overlay to visualize cached frame buckets on the timeline
- **2026-01-07:** Capped in-process FFmpeg decoders with an LRU eviction policy (8 max) and added sequential playback decode mode
- **2026-01-07:** Increased playback prefetch window to 1s and forced a preview render after GPU init; native overlay stays hidden until first upload
- **2026-01-07:** Fixed WGPU preview shader uniform layout to prevent pipeline validation crashes
- **2026-01-07:** Switched native preview to upload per-layer textures and composite them in WGPU using per-layer transforms and opacity
- **2026-01-07:** Preview render loop now emits layer stacks for the GPU path and triggers native redraws when layers update
- **2026-01-07:** Reworked preview stats labeling to show scan time (excluding decode/still) for clearer left-to-right stage timing
- **2026-01-07:** Allowed the preview panel to shrink vertically to avoid the native surface overlapping the timeline in short windows
- **2026-01-07:** Added a small Windows-only native preview offset to compensate for the then-current host client-area inset
- **2026-01-07:** Kept preview header layout stable when stats are hidden and removed preview padding to align the native surface
- **2026-01-07:** Added a title-bar toggle for preview stats and anchored native preview bounds to a dedicated host rectangle
- **2026-01-07:** Aligned native preview bounds to the canvas element, moved stats into the preview header, and switched native letterbox bars to black
- **2026-01-07:** Fixed native preview positioning to use parent-relative coordinates (avoids double offset when moving the app window)
- **2026-01-07:** Adjusted native preview window positioning to use window-origin coordinates and raised the child window to the top of the z-order
- **2026-01-07:** Added wgpu upload timing to the preview performance overlay
- **2026-01-07:** WGPU preview now uploads RGBA frames to a texture and renders via a quad (canvas uploads suppressed once native preview is active)
- **2026-01-07:** Restored preview canvas visibility while the native host is active and fixed preview overlay stacking so stats stay visible
- **2026-01-06:** Added preview performance overlay (cache hit rate + per-stage timing) to guide optimization work
- **2026-01-06:** Served preview frames from in-memory PNG store to remove per-frame disk writes
- **2026-01-06:** Switched preview output to raw RGBA canvas uploads (removed PNG encode from the loop)
- **2026-01-06:** Added preview stats reference doc (overlay field definitions)
- **2026-01-06:** Added wgpu native preview surface spike (child window + bounds sync)
- **2026-01-06:** Throttled native preview init/update to avoid UI stalls (bounds change + redraw gating)
- **2026-01-06:** Updated ffmpeg-next to v8.0.0 to align with FFmpeg 7.x headers from vcpkg
- **2026-01-06:** Added in-process FFmpeg decode worker for preview frame extraction
- **2026-01-06:** Removed ffmpeg scale filter from preview decode to avoid empty frames; scaling happens in Rust after decode
- **2026-01-06:** Fixed preview latest-wins gating so in-flight renders don't get discarded when the render gate is busy
- **2026-01-09:** Started Provider UI V2 rebuild to fix persistent state management issues—created clean modals with no draft buffers, no effects for init, direct file I/O
- **2026-01-09:** Fixed Provider Builder not re-initializing on subsequent opens—`initialized` flag was preventing seed processing after first modal open
- **2026-01-06:** Added preview frame cache (2GB budget), latest-wins scheduling, and prefetch window for smoother scrubbing
- **2026-01-06:** Clip context menu now supports moving clips up/down to compatible tracks
- **2026-01-06:** Attribute editor numeric fields commit on blur/Enter to avoid input jitter
- **2026-01-06:** Added preview renderer v0 (ffmpeg frame extraction + compositing) and playhead-driven preview updates
- **2026-01-06:** Added clip transforms + single-clip selection with transform editing in Attributes panel
- **2026-01-06:** Startup modal now captures project resolution, FPS, and duration; location field moved to bottom with separator
- **2026-01-06:** Added project duration to settings and extended timeline ruler ticks across full duration
- **2026-01-06:** Fixed left-edge trim drift by anchoring to drag-start end time
- **2026-01-06:** Removed unused `mut` warnings from thumbnail tick signals and clip resize logic
- **2026-01-06:** Added clip trim-in state for left-edge trimming; timeline thumbnails now offset by trim-in and clip filmstrip is clipped to bounds
- **2026-01-06:** Fixed thumbnail refresh wiring for asset/timeline panels and duration probe helpers
- **2026-01-06:** Asset durations now cached via ffprobe for audio/video; clips use asset duration on drop/add and resizing is clamped to source length
- **2026-01-06:** Thumbnail URLs now cache-bust on refresh and missing files no longer render broken images
- **2026-01-06:** Asset panel shows first-frame thumbnails for visual assets; timeline thumbnails distribute across clips using 1s sampling with repeat-fill on zoom
- **2026-01-06:** Implemented robust custom protocol (`http://nla.localhost`) for serving local thumbnails
- **2026-01-06:** Added "Rendering & Preview Strategy" to docs
- **2026-01-06:** Promoted Preview Window and Thumbnails to MVP status based on user feedback
- **2026-01-06:** Added right-click context menu to delete projects from startup modal
- **2026-01-06:** Fixed project list layout (compact items, proper overflow handling, scrollable)
- **2026-01-06:** Improved Startup Modal: existing projects now listed automatically, file dialogs start from projects folder
- **2026-01-04:** Implemented custom context menus for track management
- **2026-01-04:** Added "Move Up/Down" track reordering via context menu
- **2026-01-04:** Fixed window title and removed default Win/Edit/Help menu bar
- **2026-01-04:** Added viewport-constrained context menu positioning
- **2026-01-04:** Implemented New Project modal dialog with folder creation
- **2026-01-04:** Added track add/remove functionality with UI buttons
- **2026-01-04:** Integrated Project data model with timeline (dynamic tracks)
- **2026-01-04:** Created core data structures (Project, Track, Clip, Asset, Marker)
- **2026-01-04:** Implemented timeline clip interactions (Move, Resize, Delete, Drag & Drop)
- **2026-01-04:** Refined resize handles and fixed context menus

---

## 🧭 Development Philosophy

> **"Tight, lean, focused."**

This project intentionally:
- **Avoids premature abstraction** — We discover the right patterns during implementation, not before
- **Minimizes external dependencies** — If we can build it simply, we do
- **Iterates with the user** — Frequent check-ins, test early, refine as we go
- **Stays in its lane** — AI video generation workflow, not a full-featured video editor
- **Values feel over features** — Every component should feel intentional and polished
- **Prioritizes fluidity** — Smooth hover effects, transitions, and scrubbing from the start

We start with the UI shell, dial in the look and feel, then layer in functionality. Style and UX decisions are made early to avoid refactoring across the codebase later.

---

*Last updated: 2026-05-21*



