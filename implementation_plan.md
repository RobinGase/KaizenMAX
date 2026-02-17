# Kaizen MAX - Implementation Plan

## 1) Product Identity
- **Brand:** Kaizen
- **Product:** MAX
- **Display name in UI/docs:** **Kaizen MAX**
- **Primary AI name:** **Kaizen**

## 2) Product Goal
Build a Windows-first developer cockpit that integrates ZeroClaw (Rust runtime) + Nex_Alignment so you can:
- Talk mainly to **Kaizen** (reason/review/plan/code).
- Spawn sub-agents **only when you request it**.
- Watch each agent in its own toggleable chat panel.
- Enforce strict review gates before completion/deploy.
- Keep terminal-level workflow functionality available from UI settings.

## 3) Core Operating Principles
- **Single-main-agent default:** Kaizen is primary.
- **User-controlled orchestration:** no auto-spawn; sub-agents are created only on explicit instruction.
- **Hardware-aware mode:** optimized for normal use, with optional scale-up for swarms when you explicitly request it.
- **Hard integrity gates:** no task can be finalized without required approvals.
- **No hardcoded personas in code:** rely on settings/config surfaces.
- **Settings-first control plane:** all major features are toggleable in the UI settings menu.
- **Node-free default stack:** Node.js runtime is removed from the baseline architecture.

## 4) Repository Layout
```text
KaizenMAX/
  core/                  # ZeroClaw runtime/gateway (Rust binary-first)
  ui/                    # Kaizen MAX dashboard (React + TypeScript)
  protocol/              # Nex_Alignment fork and MCP assets
  compat/                # Optional compatibility adapters (disabled by default)
  scripts/
    start-max.ps1        # Start workflow (native UI + ZeroClaw core)
  implementation_plan.md
```

## 5) Runtime Strategy (ZeroClaw-First, No WSL)
Given local memory constraints and your efficiency target, Kaizen MAX uses a ZeroClaw-first baseline:
- **Recommended default:** native Windows UI + native ZeroClaw core binary.
- **Why:** removes Node.js baseline overhead and avoids WSL translation overhead.
- **Inference mode:** provider-hosted inference APIs only (no local open-weight model hosting).
- **Desktop requirement:** Kaizen MAX app runs natively on Windows.

Optional mode:
- Remote Linux ZeroClaw core + native Windows UI (for offloading CPU/RAM while keeping the same runtime).

Compatibility mode (off by default):
- Optional OpenClaw/Node bridge can be added later if a required feature is missing.
- Node.js is not part of the default runtime stack.

Avoid by default:
- WSL2 runtime for core services.
- Docker Desktop + WSL2 for day-to-day operation.

### Hardware Guidance (Provider Inference Only)
- **GPU requirement:** none.
- **Local desktop minimum:** 16 GB RAM, modern 6-core CPU, NVMe SSD.
- **Preferred local desktop:** 32 GB RAM, 8+ core CPU for heavier orchestration.
- **Remote core minimum:** 2 vCPU, 4 GB RAM, 50 GB SSD.
- **Remote core recommended:** 4 vCPU, 8 GB RAM, 100 GB SSD.
- **Remote heavy mode (10-20 agents):** 8 vCPU, 16 GB RAM, 200 GB SSD.

## 6) UX and Interaction Model

### Main Workspace
- Kaizen chat is always visible.
- Agent list/panel shows current agents and status.
- Clicking an agent toggles that agent chat open/closed.

### Multi-Chat Behavior
- Every spawned agent gets its own chat context.
- New agent chats are **created closed by default**.
- User can talk directly to any agent or route all requests through Kaizen.

### Crystal Ball Feed
- Mattermost-backed feed for **AI:AI:HUMAN** communications.
- Twitch-style scroll feed, draggable/resizable overlay.
- Agent usernames match user-assigned agent names.

## 7) Settings-First Feature Toggles (Personal Defaults)

All major features are toggleable in the settings menu. Default profile reflects your personal workflow:

- Every new feature ships behind a settings toggle before becoming default-on.

- `runtime_engine`: `zeroclaw` (default), `openclaw_compat` (disabled/off).
- `auto_spawn_subagents`: `false`.
- `max_subagents`: `5`.
- `main_chat_pinned`: `true`.
- `new_agent_chat_default_state`: `closed`.
- `allow_direct_user_to_subagent_chat`: `true`.
- `crystal_ball_enabled`: `true`.
- `crystal_ball_default_open`: `false`.
- `hard_gates_enabled`: `true`.
- `require_human_smoke_test_before_deploy`: `true`.
- `provider_inference_only`: `true`.

## 8) Orchestration and Gate Logic

### Master Planner
- Kaizen acts as the master planner/reasoner.
- Kaizen can orchestrate up to a configurable limit (default max: 5).
- Kaizen assigns task slices and monitors progress.

### Enforced State Machine (Hard Gates)
```text
Plan -> Execute (Sub-Agents) -> Review (Nex_Alignment) -> Human Smoke Test -> Deploy
```

Hard-gate rules:
- No agent may finalize output without Kaizen approval.
- Required review checkpoint: **Passed Reasoners Test**.
- If review fails, flow returns to Execute/Review until passed.

## 9) Nex_Alignment Integration Plan

### Alignment Audit First
1. Inventory ZeroClaw native review/approval tools.
2. Inventory Nex_Alignment MCP tools.
3. Build collision matrix.
4. Identify only critical parity gaps that require a compatibility adapter.

### Collision Handling
- If ZeroClaw already has equivalent native gates, deprecate overlapping local protocol tools.
- If missing, bridge Nex MCP tools into gateway/tool layer.
- If still missing, implement optional adapters in `compat/` without changing the default runtime.

### Prompt/Template Injection
- Inject core Nex tenets into ZeroClaw system prompt and `AGENTS.md` templates.
- Ensure all spawned agents inherit the same governance baseline.

## 10) ZeroClaw Feature Parity Requirement
- UI settings must expose terminal-level functionality where feasible.
- Personas/behavior are controlled through settings/config, not hardcoded app logic.
- Keep gateway/tool controls available in UI for power use.
- If parity gaps are found, isolate them behind optional compatibility adapters in `compat/`.

## 11) Security and Compliance Baseline
- **Comptia Cloud+-aligned architecture posture** (operationally applied).
- **Zero-Trust** service-to-service model.
- **Secure at rest** (encrypted storage/secrets).
- **Secure by design** defaults.
- `.env` governance keys use **`ADMIN_`** prefix convention.

## 12) Kubernetes/Cloud Architecture Track
Design target includes:
- Gateway/API + UI workloads with clear trust boundaries.
- RBAC, network policies, secret management, auditability.
- Separate control-plane concerns from chat/session-plane workloads.
- Scalable but capped agent execution policies.

## 13) Implementation Phases

### Phase A - Bootstrap
- Create repo structure and clone `core` (ZeroClaw), `ui`, and `protocol` sources.
- Install dependencies and verify baseline health.

### Phase B - Alignment Audit
- Complete MCP/native gate comparison for ZeroClaw + Nex_Alignment.
- Document deprecate-vs-bridge decisions and any compatibility gaps.

### Phase C - UI Foundation
- Build Kaizen main chat + agent panel.
- Implement toggleable per-agent chat windows.
- Add drag/resize support for chats and overlays.

### Phase D - Orchestration Layer
- Implement Kaizen orchestration commands.
- Add explicit sub-agent spawn/close controls.
- Add configurable max-subagent limit.

### Phase E - Crystal Ball Comms
- Integrate Mattermost (self-hosted).
- Build live interaction feed mapping channels/users to agents.

### Phase F - Hard Gate Engine
- Implement enforced state transitions and lock conditions.
- Block finalization until review + smoke-test pass.

### Phase G - Windows Operations
- Add `scripts/start-max.ps1` to start native Windows UI and native/remote ZeroClaw core.
- Configure multi-workspace access model.

### Phase H - Security + Infra
- Apply env/admin controls, secrets handling, and policy defaults.
- Define K8s deployment blueprint and rollout strategy.

## 14) Success Criteria
- Kaizen MAX opens with Kaizen as primary chat.
- Sub-agents are manual and capped, with per-agent toggle chat windows.
- Crystal Ball feed shows AI:AI:HUMAN interactions live.
- Hard-gate state machine blocks unauthorized finalization.
- ZeroClaw settings/capabilities are manageable via UI.
- Governance conventions (`ADMIN_`, alignment checks) are active.

## 15) Execution Status Snapshot

Current branch status reflects implementation progress beyond initial bootstrap.

- Phase A bootstrap is complete.
- Phase B parity audit is in progress with an active collision matrix document.
- Phase C and D foundational UI and orchestration controls are implemented in the development branch.
- Phase E Crystal Ball local stream, Mattermost bridge, validation, and smoke tooling are implemented with operational validation still in progress.
- Phase F hard gate controls are implemented in runtime state machine and API paths.
- Phase G startup script supports packaged UI preference with development fallback.
- Phase H security controls include masking, archive retention controls, and integrity audit reporting.
