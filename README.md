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

- Rust gateway service with settings, chat, agent, gate, and event APIs.
- Enforced gate state machine and agent lifecycle transition checks.
- React and TypeScript dashboard with Kaizen chat, agent panel, per agent chat windows, and settings drawer.
- Crystal Ball live feed UI with drag and resize support.
- Local event archive with retention compaction, hash chain integrity, and optional HMAC signing.
- Optional Mattermost bridge with validation and smoke test endpoints.

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

1. Copy `.env.example` to `.env` and fill required keys.
2. Run `cargo test` in `core/`.
3. Run `npm install` in `ui/`.
4. Run `npm run build` in `ui/`.
5. Start services with `scripts/start-max.ps1`.

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
