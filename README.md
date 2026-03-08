# Kaizen MAX

Kaizen MAX is a cross-platform AI operations desktop for Windows and Linux. It gives one operator a native control surface for planning, delegating, and reviewing work across branches, missions, and workers.

`main` is the active release branch.

<p align="center">
  <img src="docs/assets/screenshots/office-2d.png" alt="Kaizen MAX office 2D view" width="100%" />
</p>

<p align="center"><strong>Office 2D</strong>: branch-level orchestration, worker status, and mission flow in one operator view.</p>

## Highlights

- Kaizen as the primary operator conversation
- `Branch -> Mission -> Worker` orchestration model
- detachable worker chats and a detachable 2D office board
- provider routing through Zeroclaw
- Crystal Ball event logging with optional Mattermost publishing
- local desktop release checks against `origin/main`

## Product Views

### Mission

![Kaizen MAX mission view](docs/assets/screenshots/mission-page.png)

### Integrations

![Kaizen MAX integrations view](docs/assets/screenshots/integrations-page.png)

## Platform Scope

Kaizen MAX is being built as a cross-platform desktop product for Windows and Linux.

Current repo ergonomics are still more mature on Windows because the checked-in launcher and updater scripts are PowerShell and batch based, but the core runtime and Tauri desktop architecture are not intended to stay Windows-only.

## Core Capabilities

### 1. Operator-led execution

Kaizen is the main executive-style agent the operator talks to for:

- planning and prioritization
- delegation to named workers
- branch and mission coordination
- review of event and gate state

### 2. Team orchestration

The runtime is structured around:

- `Branch`
- `Mission`
- `Worker`

Workers persist across restarts together with their conversation history and execution context.

### 3. Zeroclaw runtime

Zeroclaw is the runtime control plane for:

- active provider selection
- model routing
- provider readiness
- tool visibility

The default route today is `codex-cli`, which keeps the local system usable when Codex CLI is already authenticated.

### 4. OpenClaw fallback

When available on the machine, Kaizen MAX can fall back to selected OpenClaw tools instead of failing a missing local tool path outright.

Fallback coverage is explicit and limited. It is not full OpenClaw parity.

### 5. Local desktop updates

The installed desktop app can compare the local checkout to `origin/main`, notify the operator when a newer release is available, and apply updates when the worktree is clean.

## Runtime and Auth

### Supported runtime paths

- `codex-cli`
- `openai`
- `anthropic`
- `gemini`
- `nvidia`
- `gemini-cli`

### Current auth modes

- Codex CLI: local ChatGPT OAuth via `codex login`
- OpenAI / Anthropic / NVIDIA: API keys
- Gemini:
  - API key
  - app-managed Google OAuth
  - Google ADC fallback
- Gemini CLI: local CLI login

## Quick Start

### Launch

Use one of:

- `scripts\\launch-kaizen-max.ps1`
- `scripts\\start-max.ps1`
- the Desktop shortcut if you created one

### First-use setup

1. Launch the desktop app.
2. Open `Integrations`.
3. Connect or confirm the provider Zeroclaw should use.
4. Return to `Mission`.
5. Start working through Kaizen chat.

## Build and Validation

### Core

- `cd core`
- `cargo test`

### Desktop frontend

- `cd ui-rust-native`
- `cargo check --target wasm32-unknown-unknown --manifest-path frontend/Cargo.toml`
- `cargo tauri build`

## Repository Layout

- `core/` - gateway, orchestration, inference, persistence, events
- `ui-rust-native/` - Rust-native Mission Control desktop app
- `scripts/` - launcher, updater, validation helpers
- `config/` - settings defaults and schema
- `contexts/` - prompts and runtime policy templates
- `docs/` - focused technical and planning docs

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [STRUCTURE.md](STRUCTURE.md)
- [ROADMAP.md](ROADMAP.md)
- [docs/phase5_operator_readability_and_branches_plan.md](docs/phase5_operator_readability_and_branches_plan.md)
- [docs/subagent-forward-plan.md](docs/subagent-forward-plan.md)

## Current Boundaries

- OpenClaw fallback is selective, not full parity
- Gmail and lead tooling are not fully implemented yet
- image attachments flow through the chat transport, but true image understanding still depends on the active provider path

## License and Ownership

This repository is the active Kaizen MAX product line and the Rust-native Mission Control desktop is the primary frontend.
