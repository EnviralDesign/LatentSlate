---
description: Best practices and rules for AI developers working on this project
---

# AI Developer Guidelines

## Build & Test Rules

### Cargo Build
- **DO NOT** run `cargo build` or `cargo run` — the user will do this manually
- There are CLI integration issues that prevent the AI from running these commands reliably

### Cargo Test
- **Optional** - run `cargo test` only when explicitly requested

### Cargo Check
- **Always run** `cargo check` before yielding back to the user

## Development Workflow

1. **Make changes** to source files
2. **Run `cargo check`** before yielding back to the user
3. **Run `cargo test`** only when explicitly requested
4. **Notify the user** that changes are ready for them to build/run

## Code Style

- Follow standard Rust conventions (rustfmt defaults)
- Use `snake_case` for functions and variables
- Use `PascalCase` for types and structs
- Keep functions focused and reasonably sized
- Add doc comments (`///`) for public APIs

## egui Specifics

- Keep UI rendering in `src/egui_app.rs` until a split is clearly needed
- Keep reusable editor operations in `src/editor.rs` so automation and UI actions share the same path
- Prefer native egui widgets and custom painting over hidden parallel UI logic
- State management goes in `src/state/`
- Core logic (non-UI) goes in `src/core/`

## File Organization

```
src/
├── main.rs              # Entry point only
├── egui_app.rs          # eframe/egui desktop shell
├── editor.rs            # Editor model/controller shared by UI and automation
├── state/               # State management
├── core/                # Non-UI logic
└── providers/           # Provider adapters
```

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
   - Update PROJECT.md with the root cause and fix

**This approach saved hours on the Provider Builder re-initialization bug—logs immediately revealed that the `initialized` flag was blocking seed processing on the second modal open.**

## Documentation

**IMPORTANT: Always update PROJECT.md after making changes/progress.** This includes:
- Marking completed features in the MVP checklist
- Adding new decisions to the Decision Log
- Updating the roadmap if milestones are reached
- Documenting any architectural changes or new patterns established

This keeps the project documentation as the living source of truth.
