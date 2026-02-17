# Kaizen MAX

Kaizen MAX is a Windows first developer cockpit built around a Rust runtime.
The product focuses on one primary agent named Kaizen, explicit user controlled sub agent orchestration, strict review gates, and a settings first control plane.

## Project Scope

- Keep Kaizen as the primary planning and review assistant.
- Allow sub agents only when the user explicitly requests them.
- Enforce hard workflow gates before work is marked complete.
- Provide Crystal Ball event visibility with local archive integrity controls.
- Keep default runtime behavior native on Windows with ZeroClaw.

## Current Status

### Foundation implemented

- DevMaster runtime includes settings, chat, agent, gate, and event APIs.
- DevMaster runtime enforces gate state transitions and agent lifecycle checks.
- UI includes Kaizen chat, agent panel, per agent chat windows, and settings controls.
- Crystal Ball includes live feed behavior with drag and resize support.
- Local archive includes retention compaction, hash chain integrity, and optional HMAC signing.
- Optional Mattermost bridge includes validation and smoke endpoints.

### Remaining work

- Production grade Mattermost operational runbook and scheduled smoke checks.
- Persistent task and session store for broader runtime recovery.
- Infrastructure rollout track for Kubernetes and policy hardening.

## Repository Layout

```text
KaizenMAX/
  core/                     Rust runtime gateway and policy engine
  ui/                       React and TypeScript operator dashboard
  protocol/                 Nex alignment assets and parity audit docs
  compat/                   Optional compatibility adapters, disabled by default
  contexts/                 Prompt templates and policy context definitions
  scripts/                  Local start scripts
  config/                   Runtime defaults and schema
  implementation_plan.md    Product implementation plan
```

## Runtime APIs

These endpoints are currently wired in `core/src/main.rs` on the `DevMaster` branch.

- `GET /health`
- `GET /api/settings`
- `PATCH /api/settings`
- `POST /api/chat`
- `GET /api/agents`
- `POST /api/agents`
- `PATCH /api/agents/{agent_id}/status`
- `GET /api/gates`
- `PATCH /api/gates/conditions`
- `POST /api/gates/advance`
- `GET /api/events`
- `GET /api/crystal-ball/health`
- `GET /api/crystal-ball/audit`
- `GET /api/crystal-ball/validate`
- `POST /api/crystal-ball/smoke`

## Local Setup

1. Ensure `.env` exists at repository root, or run `scripts/start-max.ps1 -InitEnv` once to create it from `.env.example`.
2. Run `cargo test` in `core/`.
3. Run `npm install` in `ui/`.
4. Run `npm run build` in `ui/`.
5. Start services with `scripts/start-max.bat` or `scripts/start-max.ps1`.

The launcher binds child processes to a kill on close Job Object. If one process exits or the terminal closes, the remaining pipeline processes are stopped automatically.

## Branch Workflow

- `main` is the stable integration baseline.
- `DevMaster` is the active development branch.

## Documentation Index

- `implementation_plan.md`
- `protocol/collision_matrix/phase_b_initial_collision_matrix.md`
- `contexts/README.md`
- `contexts/rollout_checklist.md`
- `protocol/README.md`
- `compat/README.md`
