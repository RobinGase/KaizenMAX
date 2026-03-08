# Repository Structure

## Top Level

- `core/` - Rust gateway, orchestration runtime, inference, persistence, events, native tools
- `ui-rust-native/` - primary desktop frontend and Tauri host
- `scripts/` - launcher, updater, and local validation helpers
- `config/` - settings defaults and schema
- `contexts/` - prompts and runtime policy templates
- `docs/` - public screenshots and public technical notes
- `protocol/` - protocol-facing notes
- `compat/` - compatibility notes
- `tools/` - external or reference material kept separate from runtime code

## Core Runtime

Important areas inside `core/`:

- `src/main.rs` - API surface, orchestration runtime wiring, worker execution
- `src/inference.rs` - provider inference adapters and streaming
- `src/provider_auth.rs` - provider auth resolution
- `src/zeroclaw_runtime.rs` - Zeroclaw runtime contract
- `src/zeroclaw_tools.rs` - native tool registry and execution
- `src/openclaw_bridge.rs` - selective OpenClaw compatibility bridge
- `src/worker_runtime.rs` - worker jobs, heartbeats, tool steps, runtime persistence
- `src/agents.rs` - branch, mission, and worker registry
- `src/oauth_store.rs` - app-managed OAuth token storage
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
- worker runtime state
- OAuth token state
- event archive data
- exported worker artifacts

## Screenshots and Public Assets

- `docs/assets/screenshots/` - README and public repo screenshots
- `ui-rust-native/frontend/assets/branding/` - desktop branding assets

## Branching

- `main` is the public release line
- archived or experimental work should live on named side branches
- private planning notes should stay local and out of version control

## Local-Only Areas

- `_vendor/` is a local assessment area and not part of the shipped runtime
- local planning markdown and runtime state should stay outside the public branch surface
