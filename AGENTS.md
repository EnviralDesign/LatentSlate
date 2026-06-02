---
description: Best practices and rules for AI developers working on this project
---

# AI Developer Guidelines

## Repository Overview

LatentSlate is a Windows-first Rust desktop application by Enviral Design built with
`egui`/`eframe`. It is a local-first generative NLE for AI-generated media,
with project-local assets, FFmpeg-backed preview/export, audio playback and
waveforms, generative asset versioning, and bring-your-own ComfyUI provider
workflows.

Important directories:

```
src/
├── main.rs              # Entry point and automation startup
├── egui_app.rs          # eframe/egui desktop shell root
├── egui_app/            # UI panels, modals, preview, timeline, provider UI
├── editor.rs            # Editor model/controller shared by UI and automation
├── state/               # Project, asset, selection, provider, generative state
├── core/                # Non-UI logic, automation, FFmpeg, preview, export, audio
└── providers/           # ComfyUI, OpenAI, xAI, and future provider adapters
```

Other useful paths:

- `workflows/` contains intentionally tracked example ComfyUI workflow/manifest pairs.
- `.latentslate/` is the repo-local ignored runtime folder for provider JSONs, encrypted credentials, and caches; track only its `.gitkeep` placeholders.
- `scripts/desktop-smoke.ps1` and `scripts/automation-scenario.ps1` drive native desktop smoke checks.
- `docs/PROJECT.md` is the concise living source of truth for current status, roadmap, and decisions.
- `docs/ARCHITECTURE.md` summarizes the current system/data model.
- `docs/PROVIDERS.md` covers ComfyUI/provider setup and manifest behavior.
- `docs/DESKTOP_TEST_HARNESS.md` documents the loopback automation harness.

## Sub-agent defaults

- Use mini roles for exploration, triage, and mechanical edits.
- Use full Codex for final decisions, tricky reasoning, deep work, and review.
- Use fuzzfolioworker specifically for scoring profile exploration. The lead agent must keep that worker on track.

## Build & Test Rules

### Cargo Build
- After `cargo check` succeeds, attempt `cargo build --release` before yielding when Rust source/UI changes should be immediately testable.
- If `cargo build --release` fails because the app executable is open/locked, do **not** compile to a different target; report that the release build did not succeed because the executable appears to be running.
- Do not run `cargo run` or `dx serve` unless explicitly requested.

### Cargo Test
- **Optional** - run `cargo test` only when explicitly requested

### Cargo Check
- **Always run** `cargo check` before yielding back to the user

### CI Reality
- `cargo fmt --check`, `cargo check`, and `cargo test` currently pass locally.
- `cargo clippy --all-targets -- -D warnings` currently fails on existing lint debt and should not be added as a required gate until that cleanup is done.
- External provider behavior (ComfyUI/OpenAI/xAI) should be opt-in for tests and must not be required for routine CI.

## Development Workflow

1. **Make changes** to source files
2. **Run `cargo check`** before yielding back to the user
3. **Attempt `cargo build --release`** for code/UI changes that should be immediately testable
4. **Run `cargo test`** only when explicitly requested
5. **Notify the user** that changes are ready, including whether the release build succeeded or was blocked by a locked executable

## Code Style

- Follow standard Rust conventions (rustfmt defaults)
- Use `snake_case` for functions and variables
- Use `PascalCase` for types and structs
- Keep functions focused and reasonably sized
- Add doc comments (`///`) for public APIs

## egui Specifics

- Keep UI rendering in `src/egui_app.rs` until a split is clearly needed
- Keep reusable editor operations in `src/editor.rs` so automation and UI actions share the same path
- Keep the opt-in loopback automation surface Rust-native. UI-level automation should register and invoke real egui widget responses through shared kit helpers instead of external screenshot/click scripts or hidden duplicate UI logic.
- Prefer native egui widgets and custom painting over hidden parallel UI logic
- State management goes in `src/state/`
- Core logic (non-UI) goes in `src/core/`

## Communication

- Surface to the user frequently during iterative work
- Don't make sweeping changes without check-ins
- When making UI changes, describe what was changed so user knows what to look for when they build

## Debugging Strategy

### When to Pivot to Log-Driven Debugging

**Rule of thumb:** If you hit the same wall 2-3 times on a persistent bug, immediately pivot to a log-driven approach.

#### The Problem with Pure Code Analysis
When debugging complex state flows or asynchronous behavior, static code analysis often fails because:
- Signal/state updates may have timing issues
- Event propagation can be non-obvious
- Initialization order matters in ways not visible in code
- Effects may run (or not run) in unexpected ways

#### The Log-Driven Approach

1. **Add Comprehensive Logging**
   - Instrument every step of the suspected flow with `println!` debug statements
   - Log at entry/exit of functions, closures, and effect hooks
   - Log all relevant signal values (before and after updates)
   - Log decision points (if/else branches, match arms)
   - Be generous with logging—wall-of-text is fine

2. **Use the Human as the Executor**
   - Explicitly ask the user to:
     1. Run the app
     2. Execute the specific repro steps
     3. Copy the ENTIRE console output
     4. Paste it back to you
   - This leverages the human's ability to actually execute code in the real environment

3. **Analyze the Logs**
   - The logs will reveal:
     - Which code paths actually executed
     - What order things happened in
     - What the actual signal values were at each step
     - Where the flow diverged from expectations
   - This often leads to immediate "aha!" moments

4. **Example Pattern**
   ```rust
   println!("[DEBUG] FunctionName called");
   println!("[DEBUG]   param1: {:?}", param1);
   println!("[DEBUG]   signal_value: {:?}", my_signal());
   
   if condition {
       println!("[DEBUG]   Taking branch A");
       // ...
   } else {
       println!("[DEBUG]   Taking branch B");
       // ...
   }
   
   println!("[DEBUG]   FunctionName completed");
   ```

5. **Clean Up After**
   - Once bug is fixed and verified, remove or comment out debug logs
   - Or leave strategic ones if they might help future debugging
   - Update `docs/PROJECT.md` or another retained doc only when the result changes current status, decisions, or operational guidance.

**This approach saved hours on the Provider Builder re-initialization bug—logs immediately revealed that the `initialized` flag was blocking seed processing on the second modal open.**

## Documentation

**IMPORTANT: Keep docs lean and current.** Update `docs/PROJECT.md` for status, roadmap, or decision changes; `docs/ARCHITECTURE.md` for stable system/data model changes; `docs/PROVIDERS.md` for provider setup/manifest behavior; and `docs/DESKTOP_TEST_HARNESS.md` for harness changes.

Do not add long session logs, one-off implementation plans, or research dumps to `docs/`. Use git history and issues for that.
