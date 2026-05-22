# Desktop Test Harness Plan

This app cannot be exercised like a browser tab. The current egui/eframe shell is native desktop UI, so the harness uses three layers: fast core checks that do not launch the UI, a loopback REST API for semantic/editor operations, and a UI-level registry that lets automation discover and invoke visible egui widgets.

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

Capture a broader reference set during UI migration work:

```powershell
.\scripts\automation-scenario.ps1 -Profile release -Monitor RightMost -CaptureReferenceSet
```

The scenario writes artifacts under `.tmp/desktop-smoke/`:

- `automation-*-timeline.png` - app-window-only screenshot after project/import/timeline/selection/save commands.
- `automation-*-providers.png` - app-window-only screenshot with the Providers modal open.
- `automation-*-state.json` - final semantic app state returned by the automation API.
- `legacy-ui-reference-*/*.png` - preserved pre-egui startup, timeline, selection variants, modals, queue, providers, collapsed panels, and preview stats reference screenshots.
- `ui-reference-*/*.png` - newly captured screenshots for the same scenario set.
- `egui-reference-*/*.png` - earlier egui migration screenshots for the same scenario set.

Latest reference capture:

```text
.tmp/desktop-smoke/legacy-ui-reference-20260519-173555/
.tmp/desktop-smoke/egui-reference-ready/
```

## Automation API

The executable accepts `--automation --automation-port <port>` or `NLA_AUTOMATION=1` plus optional `NLA_AUTOMATION_PORT`. The server binds only to `127.0.0.1`.

Endpoints:

- `GET /health` confirms the control plane is enabled.
- `GET /state` returns project, current time, selection, startup, and provider-modal state.
- `GET /ui` returns the visible widget registry from the last completed egui frame.
- `POST /ui/click` invokes a visible clickable widget by its current `/ui` ID.
- `POST /ui/text` replaces or appends text in a visible editable widget by its current `/ui` ID.
- `POST /screenshot` captures the current egui viewport and writes a PNG under `.tmp/automation-screenshots/`.
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

The UI-level endpoints are also accepted through `POST /command` as `get_ui`, `click_ui`, `text_ui`, and `screenshot`.

Example UI-level flow:

```powershell
$base = "http://127.0.0.1:47890"
$ui = Invoke-RestMethod "$base/ui"
$button = $ui.data.elements | Where-Object { $_.kind -eq "button" -and $_.label -eq "New Project..." } | Select-Object -First 1
Invoke-RestMethod "$base/ui/click" -Method Post -ContentType "application/json" -Body (@{ id = $button.id } | ConvertTo-Json)
$shot = Invoke-RestMethod "$base/screenshot" -Method Post -ContentType "application/json" -Body (@{ name = "new-project" } | ConvertTo-Json)
```

`/ui/click` returns `404` when the element ID is no longer present and `409` when the element is visible but disabled or not clickable. `/ui/text` uses the same visibility checks and additionally requires `editable: true`.

This is intentionally not a separate testing model. Semantic HTTP requests are consumed by the egui app loop and applied through `EditorState`, the same model/controller used by visible UI actions. UI-level HTTP requests are consumed by shared egui widget helpers during the normal render pass, so automation follows the real button/text-field path rather than a hidden parallel UI path.

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
   - `core::automation` queues semantic commands from loopback HTTP and the app runtime applies them through existing project/state operations.
   - Shared `ui_kit` widgets register current-frame egui responses and consume queued UI actions, which gives automation a self-assembling control surface as more UI is moved onto the kit.
   - egui should render the model and dispatch commands; tests should execute the same commands headlessly or through the loopback harness.

3. Treat screenshot tests as smoke checks, not exact goldens.
   - Use app-window screenshots to confirm startup, modal visibility, and gross layout state.
   - Prefer structural assertions and image sanity checks over brittle full-pixel comparisons while the UI is still moving.

4. For the egui refactor, prefer a native scenario runner.
   - egui is much easier to drive off a single model because the UI is immediate-mode.
   - The ideal loop is: build model state, run a scripted command sequence, render one or more frames, capture the window or an offscreen surface, and compare targeted visual invariants.
   - Prefer `POST /screenshot` over external window-capture scripts when automation mode is available; it captures the egui viewport directly from the renderer and writes deterministic files under `.tmp`.

## Near-Term Scenarios

- Startup smoke: app launches, main window appears, startup modal is visible.
- Project smoke: create a temporary project, verify default tracks and project file on disk.
- Timeline smoke: add generated fixture image/audio assets, place clips, seek, render preview frame.
- Modal smoke: open provider list and provider builder without suspending the preview surface incorrectly.
- Preview smoke: render a still-image clip through `PreviewRenderer` and assert non-empty preview bytes.
