# Roadmap

## Phase 0 - Completed Baseline Reset
- Archive legacy frontend state and remove obsolete rewrite branch.
- Establish fresh branch and new architecture direction.
- Initialize new Tauri v2 + SolidJS Mission Control shell.

## Phase 1 - Mission Control Functional Baseline
- Implement all major tabs with working backend actions.
- Wire chat, agents, gates, events, settings, GitHub, secrets, and OAuth status flows.
- Update launcher and validation scripts for the new UI runtime.
- Exit criteria: each tab and primary button performs a verifiable API action.

## Phase 2 - Hardening and UX Reliability
- Add stronger error states, retries, and empty/loading states per tab.
- Add streaming chat path integration for richer response UX.
- Improve accessibility, keyboard behavior, and mobile-width behavior.
- Add resizable pane system and chat readability width presets.
- Add detachable native chat windows with multi-monitor restore.
- Exit criteria: stable repeated runs with no dead controls.

## Phase 3 - Governance and Delivery Discipline
- Operationalize Nex_Alignment checkpoints in planning and release notes.
- Add CI checks that validate API contract assumptions and launch smoke paths.
- Establish release checklist tying engineering pass + governance pass.

## Phase 4 - Branch Orchestration UX
- Ship company branch manager view (`Branch -> Mission -> Worker`).
- Support orchestrator-driven parallel worker operations and handoff visibility.
- Add manager dashboards for throughput, blockers, and gate readiness.

## Midnight Sprint Governance
- Heartbeat every 30 minutes with evidence-backed status.
- Immediate replan on blocker over 20 minutes or repeat regression.
- No scope expansion after 01:00 without manager-level tradeoff decision.

## Immediate Sprint Checklist
- [ ] `npm install` + `npm run check` passes in `ui-tauri-solid/`.
- [ ] `cargo check` passes in `core/`.
- [ ] `scripts/start-max.ps1` launches core + new UI.
- [ ] `scripts/validate-launch.ps1` passes core endpoint checks.
- [ ] Manual walkthrough confirms all tabs and critical buttons execute.
