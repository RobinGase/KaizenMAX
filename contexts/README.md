# Kaizen MAX Context Architecture

This directory defines policy and prompt context for Kaizen MAX.

## Purpose

- Keep Kaizen as the main planner and reviewer.
- Keep sub agent orchestration explicit and user controlled.
- Keep review and deployment gate policy consistent.
- Keep security and masking rules consistent across agent sessions.
- Keep runtime behavior tied to settings and policy files.

## Priority Layers

1. `Security and Integrity Rules`
2. `Nex Alignment Rules`
3. `Kaizen MAX Product Rules`
4. `Runtime Limits and Tooling Rules`
5. `Task Context`
6. `Agent Context`
7. `Conversation History`

Higher layers override lower layers when rules conflict.

## Contents

- `templates/kaizen_system_prompt.md`
- `templates/subagent_system_prompt.md`
- `policies/review_gate_policy.yaml`
- `policies/agent_control_policy.yaml`
- `policies/crystal_ball_event_policy.yaml`
- `rollout_checklist.md`

## Runtime Binding Status

- Gate and lifecycle policy behavior is implemented in Rust runtime modules.
- Crystal Ball masking and archive integrity behavior is implemented in Rust runtime modules.
- Prompt templates remain source templates and are ready for direct runtime prompt binding.
