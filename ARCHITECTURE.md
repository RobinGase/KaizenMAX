# Kaizen MAX Architecture

## Scope
This document defines the fresh-start architecture after retiring the legacy Dioxus and legacy web UI surfaces.

## System Layout
- `core/`: Rust Axum backend for orchestration, chat inference, agent lifecycle, gates, settings, secrets vault, and Crystal Ball events.
- `ui-tauri-solid/`: current transitional desktop app.
- `ui-rust-native/` (in progress): Tauri host with Rust-native frontend pipeline (no Node in default path).
- `tools/Nex_Alignment/`: external governance framework used for planning, audit discipline, and release checkpoints.

## Runtime Boundaries
- Mission Control frontend talks to Tauri command bridge.
- Tauri Rust layer proxies requests to `core/` HTTP API (`KAIZEN_CORE_URL`, default `http://127.0.0.1:9100`).
- Core remains the source of truth for domain state and transitions.
- Window orchestration for detachable chats is owned by Tauri Rust (`open/focus/close/restore` by `agent-{id}` labels).

## Frontend Domains
- Mission: chat, model/mode controls, agent operations.
- Branch Manager: company branch -> mission -> worker visualization and controls.
- Workflow Gates: conditions patching and transition attempts.
- Activity: event timeline and Crystal Ball validation/smoke/audit.
- Workspace: GitHub connectivity and repo selection state.
- Providers & Secrets: vault status, secrets CRUD/test/use, OAuth status/disconnect.
- System Settings: runtime and safety configuration patching.

## API Contract Strategy
- Reuse existing backend routes in `core/src/main.rs`.
- Keep typed frontend interfaces under `ui-tauri-solid/src/lib/types.ts`.
- Keep request transport centralized in `ui-tauri-solid/src/lib/tauri.ts`.

## Reliability and Security Baseline
- Health polling + explicit error toasts in UI.
- Admin token handling in UI with optional Bearer forwarding for protected endpoints.
- No secrets persisted in frontend local storage except explicitly entered admin token.
- Secret reveal action requires explicit user confirmation.

## Build and Launch
- Transitional UI dev: `npm run tauri:dev` in `ui-tauri-solid/`.
- Target UI dev path: cargo-only Rust-native frontend (no Node default dependency).
- Core dev: `cargo run` in `core/` or `scripts/start-max.ps1 -CoreOnly`.
- Combined pipeline: `scripts/start-max.ps1` starts core + Mission Control UI.

## Evolution Path
- Keep backend contract stable while iterating UI.
- Add streaming chat bridge as next increment after baseline tab coverage is stable.
- Add deeper governance hooks from Nex_Alignment into CI and release checklist.
