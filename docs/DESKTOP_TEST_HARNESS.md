# Desktop Test Harness Plan

This app cannot be exercised like a browser tab because Dioxus Desktop runs in WebView2 and the preview is a separate native `wgpu` child window. The harness needs two layers: fast core checks that do not launch the UI, and desktop smoke scenarios that launch the executable and capture visual evidence.

## Current Smoke Scripts

- `scripts/stage-runtime-dlls.ps1` copies the vcpkg FFmpeg runtime DLLs required by the executable into `target/<profile>`.
- `scripts/desktop-smoke.ps1` optionally builds, stages DLLs, launches the app, waits for the largest visible top-level window owned by the app process, verifies loaded FFmpeg modules, captures that application window under `.tmp/desktop-smoke/`, then stops the process unless `-KeepRunning` is used.
- `scripts/automation-scenario.ps1` launches the app with the loopback automation API enabled, generates a fixture PNG, creates a temporary project, imports the fixture, adds it to the timeline, seeks, adds a marker, selects the clip, saves, captures the app window, opens the Providers modal, captures again, then writes final state JSON.

Example:

```powershell
.\scripts\desktop-smoke.ps1 -Profile release -WaitSeconds 10
```

Launch on the right-most monitor before capturing:

```powershell
.\scripts\desktop-smoke.ps1 -Profile release -Monitor RightMost
```

Use a specific Windows monitor index:

```powershell
.\scripts\desktop-smoke.ps1 -Profile release -Monitor Index -MonitorIndex 1
```

Run the narrow native-like automation scenario on the right-most monitor:

```powershell
.\scripts\automation-scenario.ps1 -Profile release -Monitor RightMost
```

Capture a broader Dioxus reference set before a UI migration:

```powershell
.\scripts\automation-scenario.ps1 -Profile release -Monitor RightMost -CaptureReferenceSet
```

The scenario writes artifacts under `.tmp/desktop-smoke/`:

- `automation-*-timeline.png` - app-window-only screenshot after project/import/timeline/selection/save commands.
- `automation-*-providers.png` - app-window-only screenshot with the Providers modal open.
- `automation-*-state.json` - final semantic app state returned by the automation API.
- `dioxus-reference-*/*.png` - startup, timeline, selection variants, modals, queue, providers, collapsed panels, and preview stats reference screenshots.

Latest reference capture:

```text
.tmp/desktop-smoke/dioxus-reference-20260519-173555/
```

## Automation API

The executable accepts `--automation --automation-port <port>` or `NLA_AUTOMATION=1` plus optional `NLA_AUTOMATION_PORT`. The server binds only to `127.0.0.1`.

Endpoints:

- `GET /health` confirms the control plane is enabled.
- `GET /state` returns project, current time, selection, startup, and provider-modal state.
- `POST /command` accepts JSON commands tagged with `type`.

Current commands:

- `create_project`
- `open_project`
- `import_asset`
- `add_asset_to_timeline`
- `seek`
- `select_clip`
- `select_asset`
- `select_track`
- `select_marker`
- `add_marker`
- `save_project`
- `open_providers`
- `close_providers`
- `open_project_settings`
- `close_project_settings`
- `open_new_project`
- `close_new_project`
- `open_queue`
- `close_queue`
- `open_generative_video`
- `close_generative_video`
- `set_layout`
- `close_all_overlays`

This is intentionally not a separate testing model. HTTP requests are converted into semantic editor commands, then consumed on the Dioxus app runtime and applied through the same project/state methods used by visible UI actions. That keeps the harness close to native interaction without relying on pixel clicks.

## Why DLL Staging Exists

The executable imports these FFmpeg DLLs at process startup:

- `avcodec-61.dll`
- `avformat-61.dll`
- `avutil-59.dll`
- `swresample-5.dll`
- `swscale-8.dll`

They are available in the local vcpkg install, but launching `target\release\nla-ai-videocreator.exe` directly does not reliably put `C:\vcpkg2\installed\x64-windows\bin` on the loader path. Staging them beside the exe makes direct launches and smoke tests deterministic.

## Target Harness Shape

1. Keep pure logic testable without a desktop window.
   - Project mutation, provider manifest handling, snapping, media path resolution, and preview frame collection should be callable from tests.
   - A later `src/lib.rs` split would let integration tests import `state` and `core` directly instead of only relying on unit tests inside modules.

2. Put UI work behind commands.
   - The current `core::automation` module is a first slice: it queues semantic commands from loopback HTTP and the app runtime applies them through existing project/state operations.
   - The next step is to extract these operations into an editor model/controller layer that owns create project, import asset, add clip, select clip, seek, generate, and save.
   - Dioxus or egui should render the model and dispatch commands; tests should execute the same commands headlessly or through the loopback harness.

3. Treat screenshot tests as smoke checks, not exact goldens.
   - Use app-window screenshots to confirm startup, modal visibility, and gross layout state.
   - Prefer structural assertions and image sanity checks over brittle full-pixel comparisons while the UI is still moving.

4. For the egui refactor, prefer a native scenario runner.
   - egui is much easier to drive off a single model because the UI is immediate-mode.
   - The ideal loop is: build model state, run a scripted command sequence, render one or more frames, capture the window or an offscreen surface, and compare targeted visual invariants.

## Near-Term Scenarios

- Startup smoke: app launches, main window appears, startup modal is visible.
- Project smoke: create a temporary project, verify default tracks and project file on disk.
- Timeline smoke: add generated fixture image/audio assets, place clips, seek, render preview frame.
- Modal smoke: open provider list and provider builder without suspending the preview surface incorrectly.
- Preview smoke: render a still-image clip through `PreviewRenderer` and assert non-empty preview bytes.
