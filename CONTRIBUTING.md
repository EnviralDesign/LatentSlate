# Contributing

NLA AI Video Creator is a functional alpha. Contributions are welcome, but please keep changes small, reviewable, and honest about the current state of the app.

## Development Setup

Primary development platform today is Windows 10/11.

Prerequisites:

- Rust stable from https://rustup.rs/
- FFmpeg development/runtime libraries compatible with `ffmpeg-next`
- `ffmpeg.exe` on `PATH` for export work
- Optional: local ComfyUI at `http://127.0.0.1:8188` for provider testing

Common commands:

```powershell
cargo fmt --check
cargo check
cargo test
cargo build --release
.\scripts\stage-runtime-dlls.ps1 -Profile release
```

Run the app from the built executable after staging FFmpeg DLLs:

```powershell
.\target\release\nla-ai-videocreator.exe
```

Useful docs:

- [Current status and roadmap](./docs/PROJECT.md)
- [Architecture overview](./docs/ARCHITECTURE.md)
- [Provider and ComfyUI guide](./docs/PROVIDERS.md)
- [Desktop test harness](./docs/DESKTOP_TEST_HARNESS.md)

## Verification Expectations

Before opening a PR:

- Run `cargo fmt --check`.
- Run `cargo check`.
- Run `cargo test` if your change touches state, core logic, providers, export, generation, or anything with existing tests.
- Attempt `cargo build --release` when Rust source/UI changes should be immediately testable.

`cargo clippy --all-targets -- -D warnings` is not a required gate yet. It currently fails on existing lint debt in UI, preview, audio, provider, and state modules. Clippy cleanup is welcome, but avoid mixing broad lint cleanup with unrelated feature work.

## Coding Conventions

- Follow standard Rust formatting and naming conventions.
- Keep public APIs documented with `///` where they are meant for reuse.
- Keep UI rendering in `src/egui_app.rs` and `src/egui_app/` unless a split is clearly justified.
- Keep reusable editor operations in `src/editor.rs` so UI and automation share the same behavior.
- Keep non-UI logic in `src/core/`.
- Keep project/state data model changes in `src/state/`.
- Prefer native egui widgets and shared `ui_kit` helpers over hidden parallel UI logic.
- Provider integrations should go through `src/providers/` and the shared generation/output-version path.
- Do not commit personal ComfyUI workflows, provider JSON files with secrets, generated media, or local project folders unless they are intentionally sanitized examples.
- Keep docs short and current. Do not add long planning docs, research dumps, or session logs.

## Filing Bugs

Please include:

- OS and Rust version.
- Build command used.
- Whether FFmpeg DLL staging was needed.
- App output or relevant console logs.
- Reproduction steps from a fresh launch when possible.
- Screenshots or a minimal project/workflow when safe to share.
- For ComfyUI issues: ComfyUI version, relevant custom nodes, provider manifest, workflow API JSON, output node type, and whether the workflow succeeds inside ComfyUI by itself.

Use the GitHub issue templates when possible.

## Areas Where Help Is Wanted

- ComfyUI workflow compatibility testing and clear failure reports.
- Provider adapters for local or hosted generation services.
- Export validation across source formats, durations, audio layouts, and codecs.
- macOS and Linux build investigation.
- Automated tests around project persistence, provider manifests, export, timeline operations, and media path resolution.
- CI hardening, including a future clippy cleanup plan.
- Public screenshots, demo projects, and docs that do not contain private media or credentials.
