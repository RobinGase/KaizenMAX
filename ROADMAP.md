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

## Phase 5 - Operator Readability + Company Branches (Current Plan)

### Track A - Chat Readability and Structured Output
- Render mission chat as structured content (headings, paragraphs, lists, code blocks) instead of raw wall-of-text output.
- Constrain assistant message width for readable line length and clearer scanning.
- Style markdown primitives for operator-grade readability in low-light grayscale UI.
- Add copy affordances for code blocks and command snippets.
- Keep docked/floating/detached chat behavior synchronized.

Milestones:
- A1: Add markdown rendering pipeline in `ui-rust-native/frontend`.
- A2: Ship typography and spacing system for chat content blocks.
- A3: Add code block copy action and visible success feedback.
- A4: Validate readability in Mission tab and detached chat windows.

Exit criteria:
- Responses with lists, steps, and code display as structured readable blocks.
- No horizontal overflow for normal prose.
- Operator can copy code blocks with one click.

### Track B - Company Branches Functionality and UI Design
- Move from flat mission grouping to explicit hierarchy: `Branch -> Mission -> Worker`.
- Add branch-aware worker spawning and management flows.
- Make bottlenecks and worker status visible at branch and mission levels.
- Support branch-scoped filtering in activity and workspace views.

Milestones:
- B1: Introduce explicit `branch_id` and mission mapping in core agent flow.
- B2: Add branch-aware APIs and typed frontend models.
- B3: Redesign Branches tab with clear hierarchy and aggregate stats.
- B4: Update Workspace spawn and control UX to assign workers to branches/missions.

Exit criteria:
- Operator can create/select a branch, assign workers, and monitor branch progress in one flow.
- Branch cards show mission and worker counts with status distribution.
- Activity and workspace can be filtered by selected branch.

## Midnight Sprint Governance
- Heartbeat every 30 minutes with evidence-backed status.
- Immediate replan on blocker over 20 minutes or repeat regression.
- No scope expansion after 01:00 without manager-level tradeoff decision.

## Immediate Sprint Checklist
- [ ] Chat readability: markdown rendering + spacing + code block copy in Mission and detached chat views.
- [ ] Company branches: branch-aware data model + branch/mission worker hierarchy in Branches tab.
- [ ] Workspace UX: spawn worker into selected branch and mission without free-form ambiguity.
- [ ] Validate launch and API smoke (`scripts/validate-launch.ps1 -UseStartMax`) after each major slice.
- [ ] Manual walkthrough confirms branch assignment, status transitions, and readable operator chat.
