# Repository Structure

## Top-Level Map
- `core/` - Rust backend runtime and API surface.
- `ui-tauri-solid/` - active desktop frontend (Tauri v2 + SolidJS).
- `scripts/` - launch, validation, and smoke automation.
- `config/` - defaults and schema for runtime settings.
- `contexts/` - policies and prompt templates.
- `tools/Nex_Alignment/` - external governance toolkit (submodule).
- `docs/` - focused technical docs.

## Frontend Structure (`ui-tauri-solid/`)
- `src/App.tsx` - Mission Control shell and tab workflows.
- `src/lib/types.ts` - typed API/domain models.
- `src/lib/tauri.ts` - command bridge request helper.
- `src/styles.css` - visual system tokens and layout styling.
- `src-tauri/src/commands.rs` - backend proxy command handlers.
- `src-tauri/src/lib.rs` - Tauri app bootstrap and state wiring.

## Branching and Legacy Policy
- Active implementation branch: `v2-mission-control`.
- Legacy archive branch: `legacy/ui-dioxus-v1-20260221`.
- Legacy archive tag: `legacy-ui-dioxus-v1-20260221`.
- Deprecated rewrite branch removed: `Kaizen-OpenCode-Rust-rewrite`.

## Artifact Hygiene
- Build outputs and logs are ignored via `.gitignore`.
- Legacy UI code is removed from active branch and preserved only via legacy branch/tag.
- Validation artifacts remain under `logs/` when generated locally.

## Docs Ownership
- `VISION.md` - product intent and success criteria.
- `ARCHITECTURE.md` - component boundaries and runtime contracts.
- `STRUCTURE.md` - repository map and conventions.
- `ROADMAP.md` - phased execution and acceptance gates.
