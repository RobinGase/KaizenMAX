# Kaizen MAX Context Architecture

This folder defines the context strategy for Kaizen MAX.

## Purpose
- Keep Kaizen as the main planner/reasoner.
- Keep sub-agents user-controlled and hardware-aware.
- Enforce hard review gates before finalization/deploy.
- Keep alignment and security rules consistent across all agent sessions.
- Keep desktop workflow native on Windows (no WSL baseline).
- Keep feature behavior controlled through settings toggles with personal defaults.

## Context Layers (highest priority first)
1. `Security/Integrity Rules`
2. `Nex Alignment Rules`
3. `Kaizen MAX Product Rules`
4. `Runtime Limits and Tooling Rules`
5. `Task Context`
6. `Agent-Specific Context`
7. `Conversation History`

If two rules conflict, higher-priority layers win.

## Files
- `templates/kaizen_system_prompt.md`
- `templates/subagent_system_prompt.md`
- `policies/review_gate_policy.yaml`
- `policies/agent_control_policy.yaml`
- `policies/crystal_ball_event_policy.yaml`
- `rollout_checklist.md`

## Notes
- These are planning-first templates and policies.
- During ZeroClaw integration, bind these to the actual ZeroClaw settings fields for system prompts, agent templates, and policy hooks.
- Preferred deployment mode: native Windows UI + native ZeroClaw core.
- Optional offload mode: native Windows UI + remote ZeroClaw core.
