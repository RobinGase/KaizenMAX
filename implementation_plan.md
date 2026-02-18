# Kaizen MAX - Implementation Plan (Refreshed)

## 1) Product Identity
- **Brand:** Kaizen
- **Product:** MAX
- **Display name:** **Kaizen MAX**
- **Primary AI name:** **Kaizen**

## 2) Current Context
- This plan replaces the previous status snapshot that marked implementation as complete.
- Frontend direction is now **Dioxus desktop** on branch `DioxusFrontend`.
- Previous egui exploration is preserved on branch `RustEguiTestBranch`.
- The UI is now in active redesign to match the user vision and prioritize agent workflow speed.

## 3) Product Goal
Build a Windows-first developer cockpit that integrates ZeroClaw core with a high-velocity multi-agent workspace so the team can:
- Talk mainly to **Kaizen**.
- Spawn and operate sub-agents quickly.
- Manage many agent chats without losing context.
- Enforce review and deployment gates safely.
- Use local and Git-connected workspaces from one interface.

## 4) Locked Priorities (User-Directed)

### P0 - Agent Chat Workspace First
- Agent chats must be fully functional before broad settings polish.
- Chat interface must support:
  - free drag
  - free placement
  - free resize
  - detached windows on other monitors
- Agent controls must be obvious and fast: add, remove, clear, stop.

### P1 - Sidebar Clarity and Workspace Hub
- Left sidebar becomes a **Workspace Hub**.
- It must support local workspaces and Git-connected workspaces.
- GitHub CLI connection state and workspace context must be visible.

### P2 - Settings Consolidation
- Technical settings are removed from sidebars.
- Use one main settings surface with tabbed sections.
- Terms must be plain-language by default, with advanced details hidden behind explicit expanders.

## 5) Core Operating Principles
- **Kaizen-first orchestration:** Kaizen remains primary planner/reviewer.
- **User-controlled spawn:** no hidden auto-spawn behavior.
- **Fast operator loop:** optimize for speed of creating, steering, and finishing agent tasks.
- **Settings-first control plane:** advanced controls exist, but are not visually overwhelming.
- **Security-first:** secret handling constraints remain strict and non-negotiable.

## 6) Repository Layout
```text
KaizenMAX/
  core/                  # ZeroClaw runtime/gateway (Rust)
  ui-dioxus/             # Active desktop frontend (Dioxus)
  ui/                    # Legacy frontend assets
  protocol/              # Nex_Alignment fork and MCP assets
  compat/                # Optional compatibility adapters
  scripts/
    start-max.ps1        # Start core + desktop app
  implementation_plan.md
```

## 7) Target Information Architecture

### Top Header (Modern, Minimal)
- Kaizen MAX title and lightweight workflow status.
- Quick actions: Refresh, Next Step.
- Settings entry remains available but does not expose all config inline.

### Left Sidebar (Workspace Hub)
- Workspace list and active workspace indicator.
- Local workspace attach/select flows.
- Git workspace connect flow via GH CLI.
- Compact integration panel: auth status, repo, branch, sync hints.
- Settings access icon at top-right of sidebar.

### Center Area (Agent Canvas)
- Multi-agent chat canvas based on the user layout vision.
- Each agent card can run as:
  - docked card
  - floating panel
  - detached native window
- Chat remains the primary center interaction.

### Right Sidebar (Workflow-Only Rail)
- Keep this rail focused and understandable:
  - active agents summary
  - current workflow phase
  - recent activity feed
- Remove deep config controls from this rail.

## 8) Agent Chat Workspace Requirements (Highest Priority)

### Window Modes
- **Docked:** card in canvas grid.
- **Floating:** draggable/resizable panel inside main window.
- **Detached:** native window that can move to another monitor.

### Interaction Requirements
- Drag by header region.
- Resize from corners/edges.
- Bring-to-front support for overlapping windows.
- Snap optional, free movement default.

### Persistence Requirements
- Save per-workspace layout state:
  - position
  - size
  - mode (docked/floating/detached)
  - z-order
  - detached monitor placement metadata
- Restore layout on startup with safe fallback if monitor topology changed.

### Chat Controls Per Agent
- Send message.
- Clear chat.
- Stop agent work.
- Remove agent from active board.

## 9) Settings System Redesign

### Main Settings Surface
- One centralized settings modal/page with tabs:
  - General
  - Workspaces
  - Agents and Workflow
  - Integrations
  - Models and Providers
  - Security and Secrets
  - Advanced

### UX Rules
- Replace unclear internal terms with plain language.
- Advanced terminology appears only in explicit advanced sections.
- Include short helper text where a user action can cause confusion.

## 10) Workspace Integration Plan

### Local Workspace
- Add local path selector.
- Persist recent workspaces.
- Show current path and active project metadata.

### Git Workspace
- Add GH CLI-backed connect flow.
- Show connection health and auth state.
- Show repo, branch, and basic status summary.
- Phase 1 is read-focused, with controlled actions added later.

### Safety
- No arbitrary command execution from UI.
- Use allowlisted backend actions for workspace and Git operations.

## 11) Backend API Roadmap for Agentic Workflow

### Existing Endpoints (already used)
- `POST /api/chat`
- `GET/POST /api/agents`
- `PATCH /api/agents/{agent_id}` (rename)
- `PATCH /api/agents/{agent_id}/status`
- `GET /api/gates`
- `POST /api/gates/advance`
- `GET /api/events`

### New or Expanded Endpoints (planned)
- `DELETE /api/agents/{agent_id}` for remove from active board.
- `POST /api/agents/{agent_id}/clear` for chat reset.
- `POST /api/agents/{agent_id}/stop` for explicit halt.
- Workspace endpoints for local and Git connection state.
- GH integration endpoints with strict backend allowlist.

## 12) Security Baseline (Unchanged and Mandatory)
- Secrets encrypted at rest.
- No plaintext secret exposure in API, UI, logs, events, or archive.
- Local mode binds to loopback only.
- Remote mode requires private networking and TLS.
- CORS allowlist enforced.
- Redaction middleware and leak tests remain release gates.

## 13) Phased Execution Plan (Proceeding Steps)

Status legend: `[x] done`, `[~] partial/in progress`, `[ ] pending`

### Phase L0 - Plan Refresh and Alignment
- [x] Refresh implementation plan to match the new vision.
- [x] Lock priority order around agent chat workspace first.

### Phase L1 - Settings Consolidation Shell
- [~] Remove config-heavy controls from sidebars.
- [x] Implement main settings surface and tab navigation shell.
- [~] Move right and left sidebar settings links to the central settings surface.

### Phase L2 - Agent Canvas Foundation
- [x] Build robust card grid with room for many agents.
- [x] Standardize card actions and status display.
- [~] Ensure chat send/receive reliability across many cards.

### Phase L3 - Floating Panels
- [x] Add draggable and resizable floating chat panels.
- [x] Add z-order handling and focus behavior.
- [x] Add per-panel mode switch between docked and floating.

### Phase L4 - Detached Multi-Monitor Windows
- [~] Add detachable native chat windows.
- [~] Sync detached windows with shared state store.
- [~] Persist and restore detached placement safely.

### Phase L5 - Agent Lifecycle Controls
- [x] Implement remove, clear, stop actions end-to-end.
- [x] Add confirmations for destructive actions where needed.
- [x] Keep actions fast and visible on every agent chatbox.

### Phase L6 - Workspace Hub and GH Integration
- [~] Add local workspace selector.
- [x] Add Git workspace connect via GH CLI status bridge.
- [x] Display repo and branch context in sidebar.

### Phase L7 - Language and Usability Pass
- [~] Replace unclear terms with plain language labels.
- [x] Reduce cognitive load in right sidebar.
- [~] Add concise help text and better empty states.

### Phase L8 - Validation and Release Gate
- [x] Functional tests for multi-agent chat flow.
- [~] Layout persistence tests including monitor change fallback.
- [x] Security regression checks against existing secret policies.
- [x] Launcher validation for core + Dioxus app lifecycle.

## 14) Immediate Sprint Checklist (Now)
- [x] Refresh plan with new priorities and proceeding steps.
- [~] Build central settings shell and move sidebar configs into it.
- [x] Simplify right sidebar to workflow and activity only.
- [x] Implement floating drag and resize for agent chat panels.
- [~] Implement detachable native chat windows across monitors.
- [x] Add agent remove/clear/stop backend and UI wiring.
- [~] Add workspace hub local path and Git context support.
- [x] Run integration and security regression pass.

## 15) Success Criteria for This Cycle
- Agent chats are the fastest path in the app.
- Multi-chat operation works with drag, resize, and detach.
- Sidebars are clean and not overloaded with technical settings.
- Users can understand workflow terms without internal knowledge.
- Workspace context is visible and actionable from the left sidebar.
- Security and secret-handling guarantees remain intact.

## 16) Execution Status Snapshot
- **Foundation status:** core APIs and Dioxus base UI are running.
- **Current phase:** L1 to L2 transition.
- **Next focus:** settings consolidation shell and chat workspace controls.
