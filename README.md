# Kaizen MAX

Kaizen MAX is a Windows-first engineering cockpit with a Rust runtime (`ZeroClaw`) and a settings-first control plane.

The system is built around one primary agent, `Kaizen`, with explicit user-controlled sub-agent orchestration, hard workflow gates, provider-based inference, and Crystal Ball event visibility.

## Current State

Kaizen MAX currently includes:

- Rust core API for chat, agent lifecycle, gate transitions, events, credentials, and settings.
- React and TypeScript UI for Kaizen chat, agent panel, Crystal Ball overlay, and settings.
- Real provider inference integration (Anthropic and OpenAI), including streaming responses.
- Encrypted secrets vault using AES-256-GCM.
- Crystal Ball event archive with hash-chain integrity and optional HMAC verification.
- Mattermost bridge support with UI-driven setup and diagnostics.

## Requirements

- Windows 10 or Windows 11
- Rust stable toolchain (`cargo`)
- Node.js 20+ and `npm` (UI build and local UI dev)
- Git
- Optional: Mattermost server for Crystal Ball bridge mode

## Quick Start

1. Start the stack:
   - `scripts/start-max.bat`
   - or `scripts/start-max.ps1`
2. Open the UI (default: `http://localhost:3000`).
3. Open `Settings -> Providers`.
4. Configure inference:
   - Select provider and model.
   - Store API key in `Provider Credentials`.
5. Optional Crystal Ball bridge setup:
   - Set Mattermost URL and channel ID in Providers.
   - Store Mattermost bot token in encrypted credentials (`Mattermost Bot`).
   - Run `Validate` and `Smoke` from Providers.

No manual vault key setup is required. If `ADMIN_VAULT_KEY` is not set, Kaizen MAX auto-generates and manages a vault key file.

## Security Model

- Secrets are encrypted at rest in the vault using AES-256-GCM.
- Plaintext secret values are not returned by API endpoints.
- Credentials UI shows masked metadata only.
- Crystal Ball events are redacted before archive/bridge publication.
- Hard gate workflow remains enforceable (`Plan -> Execute -> Review -> Human Smoke Test -> Deploy`).

## Core API Surface

Primary endpoints exposed by `core/src/main.rs`:

- `GET /health`
- `GET /api/settings`
- `PATCH /api/settings`
- `POST /api/chat`
- `POST /api/chat/stream`
- `GET /api/agents`
- `POST /api/agents`
- `PATCH /api/agents/{agent_id}`
- `PATCH /api/agents/{agent_id}/status`
- `GET /api/gates`
- `PATCH /api/gates/conditions`
- `POST /api/gates/advance`
- `GET /api/events`
- `GET /api/vault/status`
- `GET /api/secrets`
- `PUT /api/secrets/{provider}`
- `DELETE /api/secrets/{provider}`
- `POST /api/secrets/{provider}/test`
- `GET /api/crystal-ball/health`
- `GET /api/crystal-ball/audit`
- `GET /api/crystal-ball/validate`
- `POST /api/crystal-ball/smoke`

## Repository Layout

```text
KaizenMAX/
  core/                     Rust runtime gateway and policy engine
  ui/                       React + TypeScript operator dashboard
  protocol/                 Alignment assets and collision matrix docs
  contexts/                 Prompt templates and policy definitions
  scripts/                  Windows launch scripts
  config/                   Runtime defaults and schema
  implementation_plan.md    Implementation and rollout plan
```

## Verification Commands

- Core tests: `cargo test` (run in `core/`)
- UI build: `npm run build` (run in `ui/`)

## Branch and Delivery

- `main` is the active integration branch.
- Changes are committed and pushed directly to `main` for this repository workflow.
