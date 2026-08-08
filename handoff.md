# LatentSlate provider identity UI handoff

## Objective

Finish the provider-source identity and LatentSlate Engine observability pass in the Rust/egui frontend.

The Engine integration itself is already first-class and working. Engine tools are discovered dynamically from `/v1/catalog`, normalized into `ProviderEntry`, cached for offline project readability, and executed through the same generation queue as ComfyUI and cloud providers. The remaining problem is UI ambiguity: several providers can share categorical names such as `Text to Video`, and dynamic Engine tools are not adequately represented in Settings > AI Providers.

Work directly on `main`. Do not open a pull request unless explicitly requested. Keep the implementation native to egui; do not add raster logo assets unless there is a compelling reason discovered during implementation.

## Required source identities

Use compact source badges that are visually consistent with the existing LatentSlate UI:

| Source | Badge |
| --- | --- |
| LatentSlate Engine | `LS` |
| ComfyUI | `CU` |
| OpenAI | `OA` |
| xAI | `xAI` |
| Custom HTTP | `<>` |

Derive source identity from `ProviderConnection`, not provider-name prefixes.

Relevant variants are in `src/state/providers.rs`:

- `ProviderConnection::ComfyUi`
- `ProviderConnection::LatentSlateEngine`
- `ProviderConnection::OpenAiImage`
- `ProviderConnection::XaiImage`
- `ProviderConnection::XaiVideo`
- `ProviderConnection::CustomHttp`

For Engine providers, also expose state from the existing `available` and `unavailable_reason` fields. Distinguish:

- `LIVE`
- `CACHED OFFLINE`
- `UNAVAILABLE`
- `NOT DISCOVERED`
- `DISABLED`

A cached-offline reason currently contains wording similar to: `LatentSlate Engine is offline; this tool was loaded from the cached catalog.`

## Required UI coverage

Create one reusable source-identity rendering module rather than duplicating badge logic.

Apply it to:

1. The provider selector in the Attributes panel.
2. The provider selector in Asset Lab.
3. Provider-choice context menus such as keyframe/bridge generation menus.
4. Project Settings > provider scope rows.
5. Settings > AI Providers local-provider rows.
6. Generation Queue rows, because queued jobs already retain their full `ProviderEntry`.

Keep provider names unchanged. The badge should disambiguate the source without changing the tool’s categorical name.

Offline/unavailable Engine entries should remain visible for project readability but should be visibly marked and non-selectable for new generation.

## Settings > AI Providers Engine section

`src/egui_app/provider_modal.rs` currently focuses on editable local provider files. Add a separate read-only `LatentSlate Engine` section above or beside the local-provider list.

It should show:

- source badge;
- endpoint from `crate::providers::latentslate_engine::load_connection_settings()`;
- state badge;
- discovered tool count;
- available tool count;
- expandable read-only list of Engine tools, their workflow category, output type, and availability;
- first useful unavailable reason when applicable;
- `Refresh` action that calls the existing provider refresh path.

Expected state logic:

- connection disabled => `DISABLED`;
- enabled and no live or cached Engine providers => `NOT DISCOVERED`;
- at least one available Engine tool => `LIVE`;
- Engine tools exist and all are cached-offline => `CACHED OFFLINE`;
- Engine tools exist but are unavailable for another reason => `UNAVAILABLE`.

Do not make Engine tools editable through Provider Builder. Their schemas are owned by the Engine catalog.

## Suggested file locations

- New reusable module: `src/egui_app/provider_identity.rs`
- Module declaration/import: `src/egui_app.rs`
- Existing local-provider source classification: `src/egui_app/provider_builder.rs`
- Settings modal: `src/egui_app/provider_modal.rs`
- Attributes selector and context menus: `src/egui_app/attributes_panel.rs`
- Asset Lab selector: `src/egui_app/asset_lab.rs`
- Project scope: `src/egui_app/project_modals.rs`
- Generation Queue: `src/egui_app/queue_panel.rs`

There is an older `ProviderSourceKind` implementation in `provider_builder.rs`. Consolidate it rather than leaving competing enums.

## Constraints

- Preserve existing provider UUIDs, schemas, project scope behavior, and generation execution behavior.
- Do not infer source from provider labels.
- Do not add `Engine -`, `Comfy -`, or similar name prefixes.
- Keep this a presentation/observability change; do not redesign provider persistence.
- Do not add a general plugin system or model-management UI.
- Use existing `ui_kit` colors/components where possible.
- Ensure long provider names truncate cleanly in narrow panels.
- Tooltips should provide the full provider name, source, category, description, and unavailable reason where useful.
- Preserve automation instrumentation on clickable provider rows.

## Cleanup context

A previous remote attempt used temporary migration scripts and a one-shot workflow. Those files have been removed intentionally. Do not recreate a source-generating migration workflow. Edit the Rust source directly and use the local compiler for tight feedback.

## Validation

At minimum run:

```powershell
cargo fmt --all -- --check
cargo check --locked
cargo test provider_identity
```

Then build and visually inspect on Windows:

```powershell
.\scripts\build-and-stage.ps1 -Profile release
.\target\release\latentslate.exe
```

Test these states:

1. Engine running before LatentSlate starts: tools show `LS` and `LIVE`.
2. Engine stopped after a successful catalog fetch, then LatentSlate restarted: cached tools remain visible and show offline/unavailable state.
3. No prior catalog and Engine offline: Settings shows `NOT DISCOVERED`; no phantom Engine tools appear.
4. Engine disabled in `engine.json`: Settings shows `DISABLED`.
5. Same categorical name from Engine and ComfyUI: selectors clearly distinguish `LS` from `CU` without name prefixes.
6. Offline Engine tools cannot be selected for a new generation.
7. Existing project provider scope and existing generative assets still resolve their provider UUIDs correctly.

## Already completed; do not duplicate

Adaptive Engine job polling is implemented in `src/providers/latentslate_engine.rs`: polling starts responsively, backs off during long periods without status/progress/message changes, and resets when the job changes. The goal is to reduce the repeated `/v1/jobs/{id}` traffic seen during long generations without changing the API contract.

Engine-side prompt/reference conditioning caches and `/v1/runtime` observability are in the separate `LatentSlate-Engine` repository on `main`.