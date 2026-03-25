# Slint Migration Todo

This checklist is the working migration spine for replacing the Dioxus shell with a modular Slint application. It is intentionally detailed and should be edited as the refactor reveals better seams.

## Current Audit

- `src/app.rs` is the primary monolith and currently mixes app bootstrap, state ownership, timeline interactions, playback control, generation orchestration, preview integration, and most layout decisions.
- `src/components/**/*.rs` and `src/timeline/**/*.rs` are Dioxus view code and should be treated as temporary reference material, not long-term architecture.
- `src/core/preview_gpu/` contains useful GPU work, but its window/surface integration is still tied to Dioxus/Tao and needs a dedicated Slint-facing adapter layer.
- `src/core/media.rs` and `src/hotkeys/mod.rs` still expose Dioxus-shaped APIs and should be normalized behind framework-neutral service interfaces.
- `src/state/`, most of `src/core/preview/`, `src/core/audio/`, `src/core/provider_store.rs`, and `src/providers/` are the best candidates to preserve and build around.

## Guardrails

- Prefer small modules with single ownership boundaries over large controller files.
- Keep legacy Dioxus code only while it is actively serving as migration reference.
- Move toward explicit app services: shell, model, command bus, preview adapter, timeline adapter, provider workflows.
- Do not let Slint UI files become giant dump sites. Split by shell area and reusable widgets as soon as each area has real behavior.

## Phase 0: Groundwork

- [x] Audit current Dioxus coupling points and migration hazards.
- [x] Create this migration todo document.
- [x] Switch the executable entrypoint to a Slint shell on the compile path.
- [x] Keep legacy Dioxus files on disk but off the primary executable path.
- [ ] Add a dedicated `legacy_dioxus` module boundary or archive folder once reference usage drops.

## Phase 1: Shell Bootstrap

- [x] Add Slint + `slint-build` dependencies with an explicit winit backend selection.
- [x] Create a minimal `build.rs` and external `.slint` window file.
- [x] Create `src/slint_app/` for shell controller/model code.
- [x] Create `src/ui/` for Slint module loading and UI markup.
- [ ] Split the current single `main_window.slint` into smaller shell files once behavior starts landing:
- [ ] `src/ui/title_bar.slint`
- [ ] `src/ui/workspace_shell.slint`
- [ ] `src/ui/status_bar.slint`
- [ ] `src/ui/panels/` for assets, preview, timeline, attributes

## Phase 2: Shared App Model

- [ ] Introduce a framework-neutral `AppModel` that owns:
- [ ] active project
- [ ] selection
- [ ] panel layout state
- [ ] playback state
- [ ] queue state
- [ ] provider registry snapshot
- [ ] preview/timeline viewport state
- [ ] Define a command/event boundary instead of directly mutating shared state from UI callbacks.
- [ ] Replace Dioxus signal-heavy ownership in `src/app.rs` with model/services that Slint can bind to.
- [ ] Add formatting/presenter helpers so `.slint` files receive view-ready values instead of ad hoc formatting logic.

## Phase 3: Service Extraction

- [ ] Move duration probing out of `src/core/media.rs` into framework-neutral service functions plus async adapters.
- [ ] Replace `src/hotkeys/mod.rs` Dioxus key types with an internal key abstraction or command mapper.
- [ ] Introduce a preview service boundary that separates:
- [ ] frame scheduling / cache invalidation
- [ ] compositing
- [ ] native surface or texture presentation
- [ ] Introduce a timeline service boundary that separates:
- [ ] layout metrics
- [ ] hit testing
- [ ] editing commands
- [ ] draw list generation

## Phase 4: Panel Migration Order

- [ ] Title bar and status bar
- [ ] Startup / new project flow
- [ ] Asset browser
- [ ] Attributes inspector
- [ ] Queue surface
- [ ] Provider management
- [ ] Timeline viewport
- [ ] Preview viewport

## Phase 5: Timeline Rewrite

- [ ] Stop treating the old Dioxus timeline widgets as reusable UI. Preserve only domain logic and interaction rules.
- [ ] Define a timeline scene model with explicit visible tracks, clips, markers, selections, and snap targets.
- [ ] Implement timeline coordinate conversion and hit testing in pure Rust.
- [ ] Implement a Slint-facing timeline adapter for pointer, wheel, and keyboard events.
- [ ] Decide whether the first working timeline render is:
- [ ] CPU software draw list rendered into an image
- [ ] WGPU texture rendered into a Slint image
- [ ] full custom rendering notifier path
- [ ] Migrate clip dragging, resizing, snapping, selection, marker edits, and track operations in that order.

## Phase 6: Preview Rewrite

- [ ] Preserve the existing preview decode/composite stack where it still makes sense.
- [ ] Replace the Dioxus/Tao child-window overlay path with a Slint-native presentation path.
- [ ] Evaluate the first stable integration target:
- [ ] `Image::try_from(...)` texture/image handoff
- [ ] Slint rendering notifier with a dedicated compositor bridge
- [ ] Restore preview stats, hardware decode toggle, and playback invalidation only after the base surface works.

## Phase 7: Provider + Queue UX

- [ ] Port the queue into model-backed Slint state instead of overlaying it over a webview shell.
- [ ] Port provider management flows without carrying over old modal state hacks.
- [ ] Split provider builder logic into:
- [ ] workflow parsing
- [ ] editable provider draft model
- [ ] save/validation actions
- [ ] view bindings

## Phase 8: Deletion Pass

- [ ] Remove `src/app.rs` from the repo once no longer needed for reference.
- [ ] Remove `src/components/` legacy Dioxus UI modules once each Slint replacement lands.
- [ ] Remove `src/timeline/` Dioxus UI modules after timeline logic is preserved elsewhere.
- [ ] Remove `Dioxus.toml`.
- [ ] Remove Dioxus custom protocol / webview assumptions from remaining services.
- [ ] Remove the Dioxus dependency from `Cargo.toml`.

## Immediate Next Slice

- [ ] Replace the placeholder shell model with a real shared `AppModel`.
- [ ] Land a command bus for top-bar actions.
- [ ] Port the startup/new-project flow first so the Slint shell can load real projects before deeper panel migration.
