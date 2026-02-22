# KaizenMAX Sub-Agent Forward Plan (Post UI Recovery)

## Mission
Ship a stable, reactive, production-ready Rust-native Mission Control experience on top of Tauri v2 + Leptos, with clear ownership boundaries for parallel sub-agent execution.

## Current Baseline
- Black-screen root causes resolved (router CSR, global Tauri bridge, canonical routes).
- Mission Control layout is live and now includes dynamic chat history, telemetry feed, collapsible hierarchy, status-aware agent rail, and detached-window focus flow.
- Core CORS now supports Tauri origins and stream logging instrumentation is active.
- Debug boot overlay is now gated behind explicit debug mode and release `wasm-opt=z` is restored.

## Delivery Model
Each phase below should be executed with one manager agent and scoped worker agents. No worker should own cross-cutting architecture decisions; those stay with manager.

---

## Phase A - Reactivity Hardening (High Priority)

### Agent A1: IPC/Chat Reactivity
**Scope**
- Validate that main mission chat refresh loop and detached chat refresh loop stay consistent under rapid sends.
- Eliminate stale-message race conditions caused by overlapping polling and writes.

**Targets**
- `ui-rust-native/frontend/src/app.rs`
- `core/src/main.rs` (`/api/chat`, `/api/chat/history`, `/api/chat/stream`)

**Acceptance**
- 10 rapid sends in detached chat produce ordered history with no dropped entries.
- Main and detached views converge to identical history within 3 seconds.

### Agent A2: Event Timeline Reliability
**Scope**
- Guarantee telemetry feed ordering and stable keying when events arrive in bursts.
- Add explicit client-side truncation policy and visual indicator when list is clipped.

**Targets**
- `ui-rust-native/frontend/src/app.rs`
- `ui-rust-native/frontend/src/models/types.rs`

**Acceptance**
- Event order remains deterministic under 100+ burst events.
- UI never freezes or grows unbounded.

---

## Phase B - Operator UX Upgrade (High Priority)

### Agent B1: Mission Composer UX
**Scope**
- Add keyboard shortcuts (`Ctrl+Enter` send, `Esc` clear draft).
- Add explicit send failure feedback for backend errors.

**Targets**
- `ui-rust-native/frontend/src/app.rs`
- `ui-rust-native/frontend/src/styles.css`

**Acceptance**
- Failed requests are surfaced inline, not silently swallowed.
- Keyboard flow is fully operable without mouse.

### Agent B2: Hierarchy Intelligence
**Scope**
- Add filters (active/blocked/done) and search on workers.
- Persist branch panel collapse state in local storage.

**Targets**
- `ui-rust-native/frontend/src/app.rs`

**Acceptance**
- Operator can isolate blocked workers in <=2 interactions.
- Collapse state survives app restart.

---

## Phase C - Observability and Diagnostics (Medium Priority)

### Agent C1: Structured Trace Events
**Scope**
- Add request correlation IDs from frontend -> Tauri command -> core request path.
- Ensure logs include method, path, status, latency, and correlation ID.

**Targets**
- `ui-rust-native/src-tauri/src/commands.rs`
- `ui-rust-native/src-tauri/src/lib.rs`
- `core/src/main.rs`

**Acceptance**
- A single chat send can be traced end-to-end by one ID.
- No secrets or prompt content leaked in logs.

### Agent C2: Debug Mode Guardrails
**Scope**
- Add environment-controlled debug panel enablement.
- Keep release binaries clean by default.

**Targets**
- `ui-rust-native/frontend/index.html`
- `ui-rust-native/frontend/src/lib.rs`

**Acceptance**
- Boot pane appears only with explicit debug toggle.

---

## Phase D - Release Control (Medium Priority)

### Agent D1: Build and Artifact Verification
**Scope**
- Verify `cargo tauri build` output and startup scripts on clean machine assumptions.
- Ensure release binary path resolution remains correct.

**Targets**
- `scripts/start-max.ps1`
- `scripts/validate-launch.ps1`
- `ui-rust-native/src-tauri/tauri.conf.json`

**Acceptance**
- `start-max.ps1` boots core + UI without manual path edits.
- Validation script passes all checks with final build.

### Agent D2: Branch and Release Hygiene
**Scope**
- Keep `main`, `DevMaster`, and `v2-mission-control` synchronized.
- Ensure no build artifacts are tracked.

**Acceptance**
- Branch heads match after each release batch.
- `git status` clean after build.

---

## Manager Runbook (Per Cycle)
1. Launch A/B/C/D agents with scoped prompts and explicit file boundaries.
2. Merge only green outputs that pass `cargo check` and `cargo tauri build`.
3. Run smoke validation:
   - Health endpoint
   - Main chat send
   - Detached chat send
   - Telemetry refresh
4. Commit by phase, then fast-forward sync all active branches.
5. Push and post release note with known risks and next queue.

## Definition of Done (Overall)
- Live mission UI remains reactive under burst usage.
- Detached windows are stable and focus-aware.
- Logs are structured and traceable without leaking secrets.
- Build/release workflow is deterministic.
- `main`, `DevMaster`, `v2-mission-control` all point to the same validated state.
