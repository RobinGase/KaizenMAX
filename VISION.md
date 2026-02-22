# Kaizen MAX Vision

## Purpose
Kaizen MAX is the operator-grade mission control for running, reviewing, and shipping multi-agent work with hard governance, not a generic chat shell.

## North Star
Deliver a native desktop Mission Control where a user can plan, execute, review, and deploy through gates with full observability and reproducible outcomes.

## Fresh Start Objectives
- Replace legacy frontend surfaces with a clean Tauri v2 Mission Control.
- Preserve and extend the Rust core as system-of-record for orchestration, gates, settings, credentials, and Crystal Ball events.
- Align UX to the clarity and operational tempo inspired by OpenCode and Agent Zero without copying their runtime architecture.
- Keep the desktop stack Rust-native and remove Node from the default development/runtime path.

## User Value
- **Operator**: sees system state at a glance, can drive every critical action without shell hopping.
- **Builder**: can run end-to-end tasks, verify gates, and iterate quickly with model and mode controls.
- **Reviewer**: can inspect event history, gate decisions, and safety posture before progression.

## Product Principles
- Ship vertical slices that are testable end-to-end.
- Keep destructive actions explicit and reversible where possible.
- Treat observability as a product feature, not an add-on.
- Prefer deterministic flows and typed contracts over implicit behavior.

## Governance and Nex_Alignment
- `tools/Nex_Alignment` is integrated as an external governance toolkit, not runtime core.
- Every major architecture or risk decision is tracked through documented checkpoints.
- Release readiness requires passing engineering checks and governance checks.

## Definition of Done for V2 Baseline
- All Mission Control tabs load and execute their mapped backend actions.
- All primary buttons have a working handler and user feedback path.
- Launch + validation scripts run against the new architecture.
- Root docs are current and usable by new contributors on day one.

## P0 Interaction Requirements
- Agent chats support three modes: docked, floating, and detached native windows.
- Detached windows move across monitors, stay synchronized with orchestrator state, and restore safely after restart.
- Mission workspace supports resizable panes and readable chat width constraints.

## Operating Model Vision
- Kaizen orchestrator acts as manager over company branches.
- Branches contain missions, missions coordinate workers, and workers collaborate in parallel with explicit handoffs.
- The interface must make parallel execution, bottlenecks, and gate readiness obvious at a glance.
