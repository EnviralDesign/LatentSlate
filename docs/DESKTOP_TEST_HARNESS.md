# Desktop Test Harness

The app is a native egui/eframe desktop program, so smoke testing uses a local executable plus an opt-in loopback automation API.

## Smoke Scripts

Stage FFmpeg runtime DLLs beside the executable:

```powershell
.\scripts\stage-runtime-dlls.ps1 -Profile release
```

Launch and capture the app window:

```powershell
.\scripts\desktop-smoke.ps1 -Profile release -WaitSeconds 10
```

Run a scripted project/timeline scenario:

```powershell
.\scripts\automation-scenario.ps1 -Profile release -Monitor RightMost
```

Capture a broader reference set:

```powershell
.\scripts\automation-scenario.ps1 -Profile release -Monitor RightMost -CaptureReferenceSet
```

Artifacts are written under `.tmp/desktop-smoke/`.

## Automation Mode

Launch arguments:

```powershell
.\target\release\nla-ai-videocreator.exe --automation --automation-port 47890
```

Environment alternative:

```powershell
$env:NLA_AUTOMATION = "1"
$env:NLA_AUTOMATION_PORT = "47890"
```

The server binds only to `127.0.0.1`.

## Endpoints

- `GET /health`
- `GET /state`
- `GET /ui`
- `POST /ui/click`
- `POST /ui/text`
- `POST /screenshot`
- `POST /command`

UI commands are consumed by registered egui widgets during normal rendering. Semantic commands are applied through the editor state/controller path where possible.

## Useful Commands

Current command types include:

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
- `get_performance_diagnostics`
- `scrub_timeline_profile`

Example:

```powershell
$base = "http://127.0.0.1:47890"

Invoke-RestMethod "$base/command" -Method Post -ContentType "application/json" -Body (@{
    type = "get_performance_diagnostics"
} | ConvertTo-Json)

Invoke-RestMethod "$base/command" -Method Post -ContentType "application/json" -Depth 4 -Body (@{
    type = "scrub_timeline_profile"
    start_time = 0.0
    end_time = 5.0
    steps = 24
    repeats = 2
    scrub_audio = $true
} | ConvertTo-Json)
```

## Preview Diagnostics

When Preview Stats is enabled, the overlay and automation diagnostics expose:

- `async`: worker state and current busy time
- `worker`: background render worker time
- `delivery`: schedule-to-UI acceptance time
- `total`: total wall-clock time
- `scan`: track/asset/cache scan time
- `vdec`: video decode time
- `seek`, `pkt`, `xfer`, `scale`, `copy`: video decode sub-stages
- `hwdec`: percent of decoded frames using hardware acceleration
- `still`: still-image load time
- `comp`: CPU RGBA composition time; normally `0` for the interactive egui layer-texture path
- `upload`: preview layer texture preparation/upload time
- `hit`: frame-cache hit percentage
- `layers`: active visual layer count
- `stale`: discarded async render count

Timeline clips may also draw cache bucket strips when preview stats are enabled.

## DLL Staging

The executable needs FFmpeg runtime DLLs at launch:

- `avcodec-61.dll`
- `avformat-61.dll`
- `avutil-59.dll`
- `swresample-5.dll`
- `swscale-8.dll`

`scripts/stage-runtime-dlls.ps1` copies these from `VCPKG_ROOT`, `C:\vcpkg2`, `C:\vcpkg`, or an explicit `-SourceBin`.

## Test Strategy

- Prefer `cargo check` and focused Rust tests for pure logic.
- Use automation commands for desktop smoke checks.
- Use screenshots for gross layout and visibility validation, not brittle pixel-perfect goldens.
- Keep provider/network behavior opt-in. Routine CI should not require ComfyUI, OpenAI, xAI, or other external services.
