---
description: Best practices and rules for AI developers working on this project
---

# AI Developer Guidelines

## Repository overview

LatentSlate is a Windows-first Rust desktop app built with `egui`/`eframe`: a local-first generative NLE with project-local media, FFmpeg preview/export, audio/waveforms, generative asset versioning, and provider integrations.

Key paths:

- `src/main.rs` — entry point and automation startup
- `src/egui_app.rs` / `src/egui_app/` — desktop shell and UI
- `src/editor.rs` — editor operations shared by UI and automation
- `src/state/` — project/asset/selection/provider/generative state
- `src/core/` — non-UI logic, automation, FFmpeg, preview/export/audio
- `src/providers/` — ComfyUI/OpenAI/xAI provider adapters
- `workflows/` — intentionally tracked example ComfyUI workflow/manifest pairs
- `.latentslate/` — ignored runtime provider JSON, encrypted credentials, and caches; track only `.gitkeep` placeholders
- `scripts/desktop-smoke.ps1`, `scripts/automation-scenario.ps1` — native smoke/automation checks

Living docs:

- `docs/PROJECT.md` — current status, roadmap, decisions
- `docs/ARCHITECTURE.md` — stable system/data model
- `docs/PROVIDERS.md` — provider setup and manifest behavior
- `docs/DESKTOP_TEST_HARNESS.md` — loopback automation harness

## Managed local stack

Use the Local Process Manager for building, running, and testing the managed LatentSlate/Engine stack. The authoritative lifecycle/discovery/reload rules live in `../LatentSlate-Engine/AGENTS.md` under **Local stack and process control**; follow them before runtime work.

Do not launch separate unmanaged test instances when the manager is available. Use its configured UI build entry for release builds. If the manager is unavailable, report that rather than silently starting an unmanaged replacement.

## Build and test gates

- **Always run `cargo check` before yielding after Rust changes.**
- After `cargo check`, attempt a release build through the Process Manager when source/UI changes should be immediately testable.
- `scripts/build-and-stage.ps1` remains available but is not required; the managed `cargo build --release` path is supported.
- If the release build is blocked because the executable is open/locked, do not evade it with a different target directory; report that the managed release build did not succeed.
- Do not run `cargo run` or `dx serve` unless explicitly requested.
- Run `cargo test` only when explicitly requested.
- External provider behavior must remain opt-in for tests and must not be required by routine CI.
- Do not make `cargo clippy --all-targets -- -D warnings` a required gate while the repository still has existing lint debt.

When yielding, state whether `cargo check` passed and whether the managed release build succeeded or was blocked.

## UI and code structure

Follow normal Rust/rustfmt conventions. Keep code in its existing ownership boundary unless there is a clear reason to move it:

- reusable editor operations belong in `src/editor.rs` so UI and automation use the same behavior;
- state belongs in `src/state/`;
- non-UI core logic belongs in `src/core/`;
- prefer native egui widgets/custom painting over hidden parallel UI logic;
- keep the opt-in automation surface Rust-native and invoke real egui widget responses through shared kit helpers instead of screenshot/click automation or duplicated hidden behavior.

**egui width trap:** do not calculate remainder widths with `ui.available_width()` from inside `ui.horizontal(...)` or another horizontal layout. The main axis may be unbounded and repeated rows can progressively widen a scroll body. Capture the bounded parent width first or use `kit::bounded_horizontal_row` and its finite `row_width` argument.

## Debugging

After 2–3 unsuccessful attempts at a persistent runtime/state bug, stop repeating static guesses and switch to evidence-driven debugging. Instrument the actual flow, reproduce it in the managed app, inspect execution order and relevant state values, then remove temporary debug logging after the behavior is verified. Prefer observed runtime behavior over increasingly elaborate speculation.

Ask the user to perform a manual repro only when the managed automation surface cannot exercise the required interaction or observation.

## Documentation and communication

Keep docs lean and current. Update the existing owner document rather than adding session logs or one-off research dumps:

- status/roadmap/decisions → `docs/PROJECT.md`
- stable architecture/data model → `docs/ARCHITECTURE.md`
- provider behavior/setup → `docs/PROVIDERS.md`
- harness behavior → `docs/DESKTOP_TEST_HARNESS.md`

Use git history/issues for transient implementation history. During iterative UI work, tell the user what materially changed and what to inspect, but do not make sweeping unrelated changes without a check-in.
