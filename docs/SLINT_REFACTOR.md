Refactor Specification: Migrating NLA‑AI Video Creator from Dioxus to Slint
1. Overview

This document defines a comprehensive plan to refactor the existing NLA AI Video Creator codebase from its current Dioxus + WebView + wgpu architecture to a Slint + native wgpu architecture. The goal is to provide a more native desktop feel, improve layout stability for both human developers and AI agents, and retain the high‑performance GPU‑accelerated preview and timeline rendering that the application already delivers. This refactor is intended for open‑source, non‑commercial use and will target Windows as the primary platform, with macOS as a secondary goal.

This spec is meant to be consumed by another ChatGPT agent (version 5.4) tasked with carrying out the refactor. It therefore describes both what to build and how to structure it in Rust and Slint terms, including file organization, module boundaries, Slint syntax, and guidelines for integrating custom wgpu panels with the Slint runtime.

2. Objectives
Replace Dioxus with Slint for the UI shell. Slint provides a declarative layout language and native widget set that better match desktop expectations. The new UI must reproduce the current app structure—title bar, side panels, timeline, preview window, attribute editor, providers modal, status bar, etc.—using Slint components.
Maintain and improve GPU‑accelerated rendering. The existing wgpu‑based compositing and preview pipeline (FFmpeg decode workers, frame cache, compositor) remains critical. The timeline itself will also require high‑performance, custom drawing. We must expose these custom render surfaces inside Slint via the set_rendering_notifier and Image::try_from APIs without relying on WebView overlays.
Preserve the data model and project features. All existing domain logic—project/clip/track structures, provider system, generation queue, job scheduling, state persistence—should remain in Rust and be reused where possible. The refactor should not throw away this logic but adapt it to the new UI event loop.
Improve layout robustness and AI agent ergonomics. The AI agent previously struggled with immediate‑mode layouts and CSS‑like styling in Dioxus. Slint’s declarative layout primitives (e.g., VerticalLayout, HorizontalLayout, GridLayout, property bindings) should make the UI easier to specify, verify, and extend by automated tools.
Enable modularity for custom panels. The timeline editor and preview viewport will be built as bespoke components (wrapping wgpu rendering code). They must accept input events (mouse, keyboard, scroll) and expose outputs (e.g., selection, cursor position, dirty rectangles) through clean Rust APIs. This makes it possible to slot in additional custom editors later (curve editor, node graph, waveform, etc.) with minimal Slint changes.
3. Current Architecture Summary

The existing codebase uses Dioxus 0.7 for UI, running inside a WebView. A wgpu compositor lives alongside the WebView to render the timeline and preview, overlayed via an offscreen surface. The project’s domain logic is organised into modules such as state (project/tracks/clips), timeline (UI controls), core::preview_gpu (wgpu compositor), core::audio (decode/playback), providers (ComfyUI adapters), and components (Dioxus UI components). The main.rs file sets up a dioxus::desktop window with a custom nla:// protocol handler to serve preview textures. The timeline is implemented in immediate mode and relies on manual CSS for layout, which has led to overlap and overflow issues.

Key features already implemented (see README.md and PROJECT.md) include:

Timeline with video/audio/marker tracks, clips with drag/resize, scroll/zoom, context menus, playhead, and ruler.
GPU‑accelerated preview using a wgpu compositor and frame cache to handle video decoding and layering.
Asset browser and attributes panel with modals for provider configuration and job queue management.
Project management (new/open/save) and an extensible provider system for ComfyUI (with planned adapters for other services).

The current pain points prompting the refactor are:

Layout frustrations: Dioxus’ immediate‑mode UI requires manual stacking of nested flex containers. AI agents waste time on CSS‑style overrides and pixel tweaks to avoid overlapping components.
Overlay limitations: The wgpu preview surface is layered on top of the WebView, meaning Dioxus UI cannot overlap or annotate the preview, and pointer events require ad‑hoc forwarding.
Non‑native feel: Running in a WebView results in non‑standard title bars and platform controls, diminishing the “first‑class desktop app” experience.
4. High‑Level Refactor Goals

To address these issues, we will refactor the UI layer to Slint, a declarative Rust‑native framework. The goals are:

Native windows: Use Slint’s Window API and native styling to match each platform’s look and feel (Fluent on Windows, Cupertino on macOS).
Declarative layouts: Define the UI hierarchy in .slint files, using VerticalLayout and HorizontalLayout nodes for panels and GridLayout for forms. Avoid absolute positioning except in custom render surfaces.
Custom wgpu panels: Implement the Timeline and Preview as separate Rust modules that render to wgpu::Textures. Use Slint’s set_rendering_notifier() (with unstable-wgpu features enabled) or Image::try_from to embed these textures as images in the scene graph. Input regions (TouchArea) layered on top of these panels will capture pointer events and forward them to the Rust modules.
State management: Centralise application state in an AppModel (e.g. Arc<Mutex<AppState>>) that includes project data, selection, playback state, job queue, and provider entries. Slint UI will bind to this model via properties and callbacks, while the timeline and preview modules will pull updates and push interactions back into the model.
Modular codebase: Reorganise the code into clearly separated crates/modules:
core — domain logic and multimedia engines (unchanged: decode workers, frame cache, provider adapters).
render — GPU rendering infrastructure for timeline and preview surfaces.
ui — Slint .slint files and Rust glue (widgets, layout definitions, modals, event handlers).
app — entry point that wires core, render, and ui together, sets up the event loop, and launches the window.
5. Proposed Architecture Using Slint
5.1 File/Module Layout (proposed)
src/
├── main.rs            # Launches the Slint application
├── app.rs             # Wires together the Slint UI and core logic
├── model.rs           # Definition of AppState and message types
├── ui/
│   ├── main.slint     # Top-level UI layout (window, panels, modals)
│   ├── panels.slint   # Definitions for side panels, status bar, title bar
│   ├── modals.slint   # Modal dialogs (new project, provider builder, queue)
│   └── components.slint # Smaller reusable components (buttons, forms)
├── render/
│   ├── timeline.rs    # Wgpu timeline renderer (custom surface + hit testing)
│   └── preview.rs     # Wgpu preview compositor (reuse existing code)
├── core/              # Existing modules (reuse with minimal changes)
│   ├── preview/       # FFmpeg workers, frame cache
│   ├── preview_gpu/   # Wgpu compositor, to be adapted for Slint integration
│   ├── audio/
│   ├── generation/
│   └── ...
└── providers/         # Provider adapters (ComfyUI, etc.)
5.2 Slint UI Structure

The UI will be authored in .slint files. Below is a high‑level outline of the top‑level layout (main.slint). It uses a VerticalLayout to arrange the title bar, content area, and status bar. Within the content area, a HorizontalLayout divides the side panels, the central area (timeline + preview), and the attributes panel. This structure mirrors the current app while making sizing explicit.

import { VerticalLayout, HorizontalLayout, GridLayout, Text, Image, TouchArea } from "std";
component MainWindow {
    in property <bool> showStartupModal;
    callback openProject(path: string);
    // Additional callbacks: newProject(), saveProject(), generateAsset(), etc.

    VerticalLayout {
        TitleBar { /* defines menu buttons, window controls, queue status */ }
        HorizontalLayout {
            // Left: Asset browser
            SidePanel {
                // Contains search bar and asset list
            }
            // Center: Timeline + Preview stacked vertically
            VerticalLayout {
                // Timeline panel placeholder
                TimelineView {
                    // Child of type Image to show GPU texture
                    id: timelineImage;
                    // Overlaid TouchArea to capture pointer events
                    TouchArea { id: timelineTouch; }
                }
                // Preview panel placeholder
                PreviewView {
                    id: previewImage;
                    TouchArea { id: previewTouch; }
                }
            }
            // Right: Attributes panel
            SidePanel {
                /* property editors bound to selected clip/asset */
            }
        }
        StatusBar { /* playback controls, timecode, queue progress */ }
    }
}

Each custom panel (TimelineView, PreviewView) must expose a resource property of type image bound to a wgpu texture. The .slint file will import the Rust backend via @resources (or through code in app.rs) to update these properties whenever the GPU renders a new frame.

5.3 Custom wgpu Panel Integration

To embed a GPU surface in Slint, there are two primary options:

Using Window::set_rendering_notifier (preferred). Enable the unstable-wgpu-XX features for the Slint crate (e.g., slint = { version = "1.4", features = ["renderer-winit", "backend-wgpu", "backend-wgpu-renderer"] }). In main.rs, after creating the window, call window.set_rendering_notifier() with a callback that obtains a wgpu Surface and performs custom rendering before or after Slint draws its scene. This allows the timeline and preview renderers to draw directly onto the swap chain each frame. Note that only one notifier can be set per window, so multiplex the timeline and preview draws within the callback.
Using Image::try_from(wgpu::Texture). Each frame, render the timeline and preview into separate textures. Convert these textures into slint::Image objects via Image::try_from(texture.clone()) and assign them to the resource property of the corresponding Image element in the UI. This is simpler but may involve an additional copy on some platforms.

Either way, pointer handling is achieved via TouchArea layered over each panel. In Slint, a TouchArea emits pressed(x, y), moved, released, mouse_wheel, and key events. These events should be forwarded to the timeline/preview modules (see Section 6) along with the panel’s size and DPI scaling factor to convert from Slint coordinates to world coordinates. For instance, when the user drags a clip on the timeline, the TimelineView receives pressed events at pixel coordinates; these are translated to timeline time units and passed into the timeline engine to update clip start/duration. Similarly, mouse_wheel events change zoom/scroll.

5.4 State Management and Event Flow

Implement a central AppModel (in model.rs) to coordinate data between the UI and core modules. A recommended structure:

#[derive(Default)]
pub struct AppModel {
    pub project: Project,
    pub selection: Selection,
    pub timeline_zoom: f64,
    pub timeline_scroll: f64,
    pub playback_state: PlaybackState,
    pub queue: GenerationQueue,
    pub provider_entries: Vec<ProviderEntry>,
    // Channels for inter‑thread communication
    pub ui_tx: Sender<UiMessage>,
    pub core_rx: Receiver<CoreMessage>,
}

// Messages emitted by the UI (e.g. from button clicks or touch events)
pub enum UiMessage {
    ImportAsset(PathBuf),
    NewProject,
    SaveProject,
    TimelineEvent(TimelineEvent),
    PreviewEvent(PreviewEvent),
    StartGeneration(GenerationRequest),
    // ...
}

// Messages emitted by core subsystems (e.g. decode completion, job progress)
pub enum CoreMessage {
    NewFrameAvailable { target: PanelKind, texture: TextureId },
    QueueUpdated,
    AssetLoaded,
    // ...
}

The app.rs file will spawn background threads/tasks for audio decoding, video decoding, generation jobs, and frame caching (reusing existing core code). Each of these tasks will communicate back to the main UI thread through channels. Slint’s invoke_from_event_loop() method can be used to schedule property updates on the UI thread when CoreMessages arrive. Similarly, UiMessages can be handled either synchronously (e.g. toggling selection) or asynchronously (e.g. starting a decode job).

6. Detailed Component Migration
6.1 Title Bar and Status Bar

Recreate the custom title bar in Slint with a HorizontalLayout containing:

The application icon and name.
Menus/buttons for New Project, Open Project, Save, Preferences.
A generation queue indicator (e.g. a button showing the number of pending jobs that opens the queue modal when clicked).
On the far right, window controls (close, minimize, maximize) using Slint’s built‑in WindowControls component.

The status bar at the bottom should show the current playhead timecode, timeline zoom level, and playback controls (play/pause, stop, loop). Use Slint properties bound to AppModel.playback_state to update icons and tooltips.

6.2 Side Panels (Assets and Attributes)

Implement the left and right panels as reusable Slint components (SidePanel) with a property controlling width and collapsed state. Within each panel, use VerticalLayout and ScrollView to present lists of assets and editable fields. For example, the Assets panel will display a list of imported files and generative assets; clicking an asset emits a UiMessage::SelectAsset to update the selection. The Attributes panel will expose fields for the selected clip or asset (e.g. name, duration, provider parameters), bound to AppModel via two‑way bindings. Because Slint supports property bindings, you can declare input elements like:

TextInput {
    text: root.model.currentAsset.name;
    on_edit: { send UiMessage::RenameAsset(self.text); }
}
6.3 Timeline Renderer

The timeline will be moved into its own Rust module (render/timeline.rs) that maintains a viewport state (scroll position, zoom level, panel size) and a representation of visible tracks and clips. It should reuse existing timeline logic from timeline and core modules where possible. The renderer will:

Build a list of primitives (rectangles, lines, text labels) for all visible clips, tracks, markers, and the playhead at the current zoom/scroll.
Use wgpu to render these primitives to a texture each frame. A 2D renderer such as wgpu_glyph
 or piet_gpu
 can be used to handle text; otherwise, reuse any existing epaint or wgpu code.
Expose methods to accept input events: press, drag, release, scroll, key, etc. Each event updates the timeline model (e.g. selecting a clip, moving/resizing a clip, zooming) and marks the renderer as dirty so that a new frame will be produced.
Communicate changes back to the AppModel via messages (e.g. UiMessage::ClipMoved with new start/duration).

The timeline’s drawing code should be separated from business logic so that the same hit‑testing and coordinate conversion used for rendering can also be used for input handling. This ensures that modifications to the timeline’s look (colors, fonts) do not break functionality.

6.4 Preview Renderer

The preview renderer can largely reuse the existing core::preview_gpu and core::preview modules. Integrate it into Slint via one of the two methods described above. Key tasks:

Expose a PreviewRenderer struct with methods new(surface: &Surface), resize(size), render_frame(project_time: f64) -> wgpu::Texture, and on_mouse_event(...) (for interactive 3D viewports if necessary).
Use the existing frame cache and decode workers to request frames at the current playhead position and composite them into the final output texture.
Support toggling of overlays (e.g. preview stats) by exposing a Slint property bound to a button in the UI.
6.5 Provider Modals and Job Queue

The provider builder modal currently uses Dioxus components. Reimplement it in Slint using forms, lists, and dynamic bindings. It should allow users to:

Select a provider type (ComfyUI, fal.ai, etc.).
Enter connection information (e.g. base URL, API key).
Upload a workflow manifest and select which inputs to expose.
Bind input parameters to assets or constants.
Save or cancel the configuration.

The generation queue should be a modal or side popover listing queued/completed jobs with progress bars and status icons. Use a ListModel in Slint bound to the queue field of AppModel. Provide actions for canceling jobs, deleting completed jobs, and clearing the queue.

7. Migration Plan
Set up a new Slint app scaffolding. Add slint as a dependency in Cargo.toml and enable the backend-winit and renderer-wgpu features. Generate a minimal main.slint file and confirm that a window opens on Windows.
Extract core logic into independent modules. Move timeline models, project state, providers, and generation logic into core/ and model.rs. Ensure that these modules do not depend on Dioxus.
Implement AppModel and message passing. Define the state and message enums. Set up channels for communication between the UI and background tasks.
Build the Slint UI. Incrementally recreate the existing panels in .slint files. Start with the title bar and status bar, then implement the asset and attribute panels, and finally wire up the provider modals and job queue.
Integrate the timeline and preview renderers. Port the existing wgpu compositor into render/preview.rs. Write a new timeline renderer in render/timeline.rs that uses the same coordinate system as the existing timeline logic. Use the chosen integration method (rendering notifier or texture upload) to display these panels in Slint.
Wire event handling. Attach TouchArea callbacks in .slint to send UiMessages for pointer events, button clicks, key presses, and scroll events. Translate coordinates and propagate to the appropriate renderer or core module.
Remove Dioxus dependencies. Once the Slint UI covers all functionality, delete Dioxus components and the WebView configuration. Remove the nla:// protocol handler and unify all asset loading through the Rust file system.
Test across Windows and macOS. Ensure that window controls, input event handling, high‑DPI scaling, and GPU backends work correctly on both platforms. Pay special attention to the set_rendering_notifier implementation, as backend support may vary.
Document and polish. Update README.md and other docs to reflect the new architecture. Provide guidance for extending the UI, writing new providers, and customizing renderers.
8. Considerations and Caveats
Unstable Slint features: The wgpu integration APIs are still marked unstable in Slint. Use the latest Slint release and be prepared to adjust to API changes. Keep the integration layer thin so that updates are isolated.
Performance tuning: Measure timeline and preview rendering separately. Keep the number of Slint UI updates minimal—update properties only when the underlying data changes or when a frame is ready. Consider using double‑buffering or ring buffers for textures to avoid blocking the UI thread.
Threading: Slint requires that UI updates occur on its event loop. Use slint::slint_runtime::run_on_main_thread() or invoke_from_event_loop() to update properties from background threads safely.
Accessibility: Slint supports keyboard navigation and focus handling. Ensure that common actions (e.g. play/pause, scrub, zoom) are available via keyboard shortcuts bound through Slint’s event system.
Licensing: Slint is dual‑licensed (GPL for open source and proprietary license for commercial use). Since this project is open source under MIT, ensure that Slint is used under the appropriate terms and that the repository reflects this choice in the LICENSE and documentation.
9. Conclusion

Migrating the NLA‑AI Video Creator to Slint promises a more robust, native, and maintainable UI foundation. By separating the high‑level layout (Slint) from the low‑level rendering (wgpu), we preserve the existing performance and flexibility of the timeline and preview pipelines while gaining predictable layouts and better AI‑assisted development. Following this spec will result in a cleaner architecture with clear boundaries between the UI shell, custom GPU renderers, and core logic, making future enhancements (audio waveform editor, curve editor, multi‑provider support) more straightforward.