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
- **Secrets-first security:** API keys and OAuth tokens must be encrypted at rest and never returned in plaintext by API, UI, logs, or events.
- **No-public-secret exposure:** secrets must never be transmitted over public networks in plaintext. Local mode binds to loopback only; remote mode requires private networking + TLS.

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

### Agent Personalization
- User assigns agent names at spawn time.
- User can rename agents after spawn via UI or API.
- Renamed identities are reflected everywhere: panel, chat windows, Crystal Ball feed.

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
- `credentials_ui_enabled`: `true`.
- `oauth_ui_enabled`: `true`.
- `agent_name_editable_after_spawn`: `true`.
- `secrets_storage_mode`: `encrypted_vault`.
- `write_plaintext_secrets_to_env`: `false`.
- `show_only_masked_secrets_in_ui`: `true`.

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
- **OAuth and API key lifecycle must be fully manageable in UI** (create/update/revoke/test), with encrypted backend handling.

## 11) Security and Compliance Baseline
- **CompTIA Cloud+-aligned architecture posture** (operationally applied).
- **Zero-Trust** service-to-service model.
- **Secure at rest:** all provider keys and OAuth tokens encrypted before persistence (AES-256-GCM envelope encryption).
- **Secure in transit:** TLS required for remote mode; local mode loopback-only by default.
- **Secure by design** defaults.
- `.env` governance keys use **`ADMIN_`** prefix convention.
- `.env` stores non-secret config and secret references only; raw secret values are never written to `.env`.
- No API endpoint, UI panel, log, archive, or event feed may expose plaintext secrets.

### Credential and OAuth Hard Requirements
- API keys and OAuth credentials are entered via UI settings only, never manual `.env` text editing.
- Values are stored in an encrypted secret vault (write-only API), not plaintext config files.
- UI only displays masked metadata: `configured` status, `last_updated` timestamp, optional `last4` characters.
- OAuth access/refresh tokens are encrypted and never rendered back to users.
- Secret leak checks are **release-gate tests** that must pass before any version is declared production-ready.

### Network Security Controls
- Local mode: core binds to `127.0.0.1` only, no public listen.
- Remote mode: private network + TLS/mTLS + authenticated connections.
- CORS allowlist required (permissive mode removed before production).
- Global request/response redaction middleware for secret patterns.

## 12) Kubernetes/Cloud Deployment Blueprint

A deployment blueprint is the production contract for how Kaizen MAX runs, scales, and stays secure.

### Blueprint Scope
- `zeroclaw-gateway` Deployment + Service.
- `ui` Deployment + Service/Ingress.
- Persistent volume for Crystal Ball archive and audit data.
- Secret management via Kubernetes Secrets + external secret manager/KMS integration.
- NetworkPolicy (default deny + explicit allow paths only).
- RBAC least-privilege roles for runtime, operators, and admin functions.
- Pod security context, resource requests/limits, health probes.
- Observability stack: logs, metrics, traces, audit events.

### Why This Matters
- Repeatable deployments across environments.
- Stronger secret isolation and auditability.
- Safer scale-up for multi-agent workloads.
- Faster incident response and rollback.

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

### Phase I - Credentials, OAuth, and Secret Vault
- Add encrypted secret vault service in core (AES-256-GCM, OS keystore/KMS backed).
- Add write-only secret API endpoints (set/revoke/test; masked GET responses only).
- Add UI settings forms for provider API keys (enter, update, revoke, test connection).
- Add OAuth connect/callback/refresh/disconnect flows.
- Ensure `.env` receives only secret references, never raw values.
- Auto-reload runtime clients after credential save (no manual restart required).

### Phase J - Agent Personalization
- Add post-spawn agent rename capability (`PATCH /api/agents/{agent_id}` with `{ name }`).
- Enforce validation: length, charset, uniqueness, reserved name checks.
- Reflect renamed identities in agent panel, chat windows, and Crystal Ball feed instantly.

### Phase K - Security Validation Gate (Release Blocker)
- **At-rest test:** inspect persisted store; only ciphertext present.
- **API test:** fetch settings/secrets metadata; zero plaintext values in any response.
- **Network test:** packet capture during key save and OAuth flow; zero plaintext key hits.
- **Log test:** scan app logs and Crystal Ball archive for secret patterns; zero hits.
- **Access test:** unauthorized user cannot view or modify secrets.
- **Regression test:** existing runtime flows still work with decrypted in-memory use only.
- **CORS test:** confirm permissive mode is removed and allowlist is enforced.
- All tests must pass before any version is declared production-ready.

## 14) Success Criteria
- Kaizen MAX opens with Kaizen as primary chat.
- Sub-agents are manual and capped, with per-agent toggle chat windows.
- User can rename spawned agents and see updated names everywhere.
- Crystal Ball feed shows AI:AI:HUMAN interactions live.
- Hard-gate state machine blocks unauthorized finalization.
- ZeroClaw settings/capabilities are manageable via UI.
- OAuth and provider keys are fully managed in UI with encrypted storage.
- No plaintext secrets are exposed in API, UI, logs, events, or network paths.
- Governance conventions (`ADMIN_`, alignment checks) are active.

## 15) Execution Status Snapshot

All implementation phases are complete. Application is fully functional for local use.

- Phase A bootstrap is complete.
- Phase B parity audit is complete with collision matrix documentation.
- Phases C and D UI foundation and orchestration controls are implemented.
- Phase E Crystal Ball local stream, Mattermost bridge, validation, and smoke tooling are implemented.
- Phase F hard gate controls are implemented in runtime state machine and API paths.
- Phase G startup workflow is implemented and hardened (Job Object lifecycle, orphan protection).
- Phase H security controls include masking, archive retention, integrity audit, and CORS allowlist enforcement.
- Phase I complete: encrypted vault (AES-256-GCM), secret API endpoints (store/revoke/test/list with masked responses), OAuth flow endpoints, and credential management UI in settings.
- Phase J complete: post-spawn agent rename via PATCH API with validation (length/charset/uniqueness/reserved names), inline rename UI in agent panel, reflected in chat windows and Crystal Ball feed.
- Phase K complete: vault at-rest encryption tests (5 tests), API masked-response tests, CORS allowlist enforced, secret redaction in logs/events, regression tests passing (26 total Rust tests, TypeScript build clean).
