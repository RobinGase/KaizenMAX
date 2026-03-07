# Midnight Delivery Plan

## Mission
Deliver a production-usable Mission Control by tomorrow with:
- Rust-native desktop direction (no Node in target default path)
- Codex-like monochrome UI language
- Resizable workspace and readable chat density
- Detachable native agent windows across monitors
- Branch manager model for orchestrator + worker parallelism

## Manager and Team
- Agent 1 - Backend Contract Lead: API contract and route safety map.
- Agent 2 - Native Rust Frontend Architect: no-Node stack selection and migration path.
- Agent 3 - Windowing Specialist: detachable native windows and restore logic.
- Agent 4 - UI Style Lead: Codex-like monochrome design tokens and layout rules.
- Agent 5 - Branch Model Strategist: orchestrator/branch/mission/worker model.
- Agent 6 - QA + Heartbeat Controller: heartbeat cadence and acceptance gates.
- Agent 7 - Migration Planner: file-level migration/cleanup sequence.

## Delivery Gates
- G1 Functional: tabs + primary actions work and return user feedback.
- G2 Windowing: detach/reattach/focus/close for agent windows across monitors.
- G3 Layout: panes are resizable; chat width remains readable on wide screens.
- G4 Stability: launch/check/smoke pass with no critical regressions.

## Heartbeat Protocol
- Cadence: every 30 minutes.
- Format: progress, gate status, defects, blocker age, next 30-minute actions.
- Escalation: immediate replan if blocker > 20 minutes or repeated regression.

## Execution Sequence
1. Lock architecture direction: Tauri + Rust-native frontend pipeline.
2. Build window orchestration primitives for detached chat windows.
3. Implement resizable mission workspace and chat width constraints.
4. Implement branch manager surface with UI-backed simulation model.
5. Run validation matrix and close gaps until all gates are green.
