# Repository Structure

## Top Level

- `core/` - Rust gateway, orchestration runtime, inference, persistence, events
- `ui-rust-native/` - primary desktop frontend and Tauri host
- `scripts/` - launcher, updater, validation helpers
- `config/` - settings defaults and schema
- `contexts/` - prompts and runtime policy templates
- `docs/` - technical notes and planning material
- `protocol/` - protocol-facing notes
- `compat/` - compatibility notes
- `tools/` - internal tool-area docs and external governance material

## Core Runtime

Important areas inside `core/`:

- `src/main.rs` - API surface and runtime wiring
- `src/inference.rs` - provider inference adapters and streaming
- `src/provider_auth.rs` - provider auth resolution
- `src/zeroclaw_runtime.rs` - Zeroclaw runtime contract
- `src/openclaw_bridge.rs` - OpenClaw fallback bridge
- `src/agents.rs` - branch, mission, and worker registry
- `src/settings.rs` - runtime settings model

## Desktop App

Important areas inside `ui-rust-native/`:

- `frontend/src/app.rs` - Mission Control UI shell and workflows
- `frontend/src/models/types.rs` - typed frontend models
- `frontend/src/styles.css` - main desktop styling
- `src-tauri/src/commands.rs` - desktop command bridge
- `src-tauri/src/lib.rs` - Tauri app bootstrap

## Runtime Data

Local runtime state is stored under `data/`.

This includes:

- worker registry snapshots
- conversation history
- OAuth token state
- event archive data

## Screenshots and Public Assets

- `docs/assets/screenshots/` - README and repo-front-page screenshots
- `ui-rust-native/frontend/assets/branding/` - product branding used by the desktop app

## Branching

- `main` is the active release line
- archived or experimental work should live on named side branches
- the old Solid/Tauri frontend is no longer part of `main`

## Local-Only Vendor Area

- `_vendor/` is currently a local assessment area and not part of the shipped runtime

If `_vendor/` is kept, it should remain intentionally separate from production code paths.
