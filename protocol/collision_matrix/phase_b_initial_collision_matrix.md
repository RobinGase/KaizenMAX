# Phase B - Initial ZeroClaw vs Nex_Alignment Collision Matrix

This matrix is the first-pass audit artifact for `Kaizen MAX`.
It is intentionally conservative: any item not yet verified in live integration is marked **Pending Validation**.

## Scope

- ZeroClaw side: `core/src/main.rs`, `core/src/gate_engine.rs`, `core/src/agents.rs`, `core/src/settings.rs`
- Nex side: `contexts/policies/*.yaml`, `contexts/templates/*.md`

## Collision Matrix

| Capability | ZeroClaw Native Status | Nex MCP/Policy Status | Collision | Decision | Notes |
| --- | --- | --- | --- | --- | --- |
| Main agent default (`Kaizen`) | Implemented in chat flow defaults | Defined in templates/policy | Low | Keep ZeroClaw native + retain policy docs | Runtime uses Kaizen as primary responder |
| Manual sub-agent spawn only | Implemented in `/api/agents` with explicit request gate | Defined in `agent_control_policy.yaml` | Medium | Keep native enforcement, policy remains source of intent | Native API denies spawn when explicit request flag is missing |
| Max sub-agent cap | Implemented in `AgentRegistry` + settings | Defined (`max_subagents: 5`) | Low | Keep native cap + settings binding | Native cap updates when settings are patched |
| Agent lifecycle transitions | Implemented with guarded transitions | Defined lifecycle states/transitions | Medium | Keep native enforcement; add policy parity checks in later phase | `review_pending -> done` requires Kaizen approval |
| Review gate state machine | Implemented in `gate_engine` | Defined in `review_gate_policy.yaml` | Low | Keep native state machine as runtime authority | Sequence matches Plan -> Execute -> Review -> Human Smoke Test -> Deploy |
| Block finalize without reasoners test | Implemented as hard block in transition logic | Defined as hard block policy | Low | Keep native hard block | Runtime blocks transition until condition is true |
| Block deploy without human smoke test | Implemented as hard block in transition logic | Defined as hard block policy | Low | Keep native hard block | Runtime blocks transition until condition is true |
| Settings-first toggles | Implemented via `/api/settings` + `config/defaults.json` | Defined in implementation plan | Low | Keep native settings route | Supports runtime patch + env override behavior |
| Crystal Ball event stream | Implemented as in-memory event API + optional Mattermost publish/fetch bridge | Policy expects Mattermost transport | Medium | Bridge implemented (env-driven), validation pending | Uses `MATTERMOST_URL`, `MATTERMOST_TOKEN`, `MATTERMOST_CHANNEL_ID`; falls back to local feed if unset |
| Mattermost bridge validation/smoke | Implemented runtime validate + smoke APIs (`/api/crystal-ball/validate`, `/api/crystal-ball/smoke`) | Policy needs operational reliability checks | Medium | Keep native validation; add operator runbook + scheduling | Validation checks reachability/auth/channel; smoke checks send+fetch roundtrip |
| Secret redaction in comms feed | Implemented message masking before local storage/publish | Policy defines deny patterns and mask behavior | Medium | Keep native redaction + expand test coverage | Masks `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `AWS_SECRET_ACCESS_KEY`, and `ADMIN_` prefixes |
| Audit retention/archive | Implemented local JSONL archive with TTL compaction + hash-chain integrity + optional HMAC | Policy requires 72h window + 30d archive | Medium | Keep native archive + validate operationally | Uses `CRYSTAL_BALL_ARCHIVE_PATH` + `CRYSTAL_BALL_ARCHIVE_TTL_DAYS` + optional `CRYSTAL_BALL_ARCHIVE_HMAC_KEY`; startup compaction + rolling append + audit verification |
| Audit visibility surface | Implemented runtime health + integrity APIs (`/api/crystal-ball/health`, `/api/crystal-ball/audit`) | Policy requires auditability | Medium | Keep native APIs and add operator runbook checks | UI settings panel consumes these endpoints for quick operator review |
| Provider inference only | Implemented as settings/env policy | Defined in plan and prompts | Low | Keep native setting and expose in UI | Inference backend integration still pending |

## Initial Deprecate-vs-Bridge Decisions

1. **Deprecate overlap in runtime**
   - Keep ZeroClaw native for gate transitions, spawn limits, and settings toggles.
   - Do not duplicate these as separate local MCP tools unless integration demands remote execution.

2. **Bridge required**
   - Crystal Ball transport to Mattermost.
   - Feed redaction/masking and retention/audit controls.

3. **Compatibility adapters (optional, off by default)**
   - Reserve `compat/` for any MCP parity gaps that cannot be solved in native gateway modules.

## Next Phase B Actions

- Validate each matrix row against a running ZeroClaw + UI session.
- Add parity tests for every hard-block rule.
- Validate Mattermost adapter in a live self-hosted workspace (publish + fetch roundtrip).
- Validate hash-chain integrity behavior under tamper/restore scenarios.
- Promote this matrix from initial audit to signed-off parity report.
