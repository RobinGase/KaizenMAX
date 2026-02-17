# Phase B Initial ZeroClaw and Nex Alignment Collision Matrix

This document captures the current parity snapshot between native ZeroClaw runtime behavior and Nex alignment policy expectations.

## Scope

- Runtime source: `core/src/main.rs`, `core/src/gate_engine.rs`, `core/src/agents.rs`, `core/src/settings.rs`, `core/src/crystal_ball.rs`, `core/src/event_archive.rs`
- Policy source: `contexts/policies/*.yaml`, `contexts/templates/*.md`

## Capability Review

### 1. Main agent default

- Native status: Implemented.
- Policy status: Defined.
- Decision: Keep native behavior and keep policy templates.
- Notes: Kaizen remains the primary responder.

### 2. Manual sub agent spawn only

- Native status: Implemented through `/api/agents` explicit request checks.
- Policy status: Defined in `agent_control_policy.yaml`.
- Decision: Keep native enforcement.
- Notes: Spawn is blocked unless explicit user request conditions are met.

### 3. Max sub agent cap

- Native status: Implemented in `AgentRegistry` and runtime settings updates.
- Policy status: Defined with default cap value.
- Decision: Keep native cap and settings binding.
- Notes: Cap updates immediately when settings change.

### 4. Agent lifecycle transitions

- Native status: Implemented with guarded transitions and approval checks.
- Policy status: Defined.
- Decision: Keep native enforcement and expand parity tests.
- Notes: Transition to `done` requires Kaizen approval and gate checks.

### 5. Review gate state machine

- Native status: Implemented in gate engine runtime state.
- Policy status: Defined in `review_gate_policy.yaml`.
- Decision: Keep native state machine as authority.
- Notes: Flow remains `Plan -> Execute -> Review -> Human Smoke Test -> Deploy`.

### 6. Reasoners test gate

- Native status: Implemented as a blocking condition.
- Policy status: Defined.
- Decision: Keep native hard block.

### 7. Human smoke test gate

- Native status: Implemented as a blocking condition.
- Policy status: Defined.
- Decision: Keep native hard block.

### 8. Settings first control plane

- Native status: Implemented via `/api/settings` and runtime loader.
- Policy status: Defined.
- Decision: Keep native route and environment override model.

### 9. Crystal Ball event stream

- Native status: Implemented local event bus and optional Mattermost bridge.
- Policy status: Expects Mattermost event visibility.
- Decision: Keep bridge and validate operationally.
- Notes: Falls back to local feed if Mattermost variables are not configured.

### 10. Mattermost validation and smoke

- Native status: Implemented via `/api/crystal-ball/validate` and `/api/crystal-ball/smoke`.
- Policy status: Requires reliable communication checks.
- Decision: Keep native checks and add operator runbook.

### 11. Secret masking in communication feed

- Native status: Implemented pre publish and pre archive masking.
- Policy status: Defined deny and mask expectations.
- Decision: Keep native masking and extend test corpus over time.
- Notes: Covers `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `AWS_SECRET_ACCESS_KEY`, and `ADMIN_` prefixed values.

### 12. Audit retention and archive integrity

- Native status: Implemented with local JSONL archive, TTL compaction, hash chain verification, and optional HMAC signing.
- Policy status: Requires retention and auditability.
- Decision: Keep native archive controls and validate in operations.
- Notes: Exposed through `/api/crystal-ball/health` and `/api/crystal-ball/audit`.

### 13. Provider inference only

- Native status: Implemented as runtime setting and environment override.
- Policy status: Defined.
- Decision: Keep native control and UI visibility.

## Deprecate and Bridge Decisions

1. Keep native runtime enforcement for gates, lifecycle transitions, and settings controls.
2. Use bridge behavior only for external event transport and validation workflows.
3. Keep compatibility adapters isolated in `compat/` and disabled by default.

## Next Actions

- Run scheduled Mattermost smoke checks in a live environment.
- Expand parity tests around policy edge cases and archive tamper handling.
- Complete prompt template runtime binding and verification.
- Promote this document to signed parity report after operational validation.
