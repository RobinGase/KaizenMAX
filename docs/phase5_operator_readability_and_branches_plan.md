# Phase 5 Execution Plan: Operator Readability + Company Branches

## Vision Alignment
This phase implements the next product step from `VISION.md`:
- Mission Control must be operator-grade, readable, and actionable.
- Branch orchestration must follow `Branch -> Mission -> Worker` with clear status and bottlenecks.

## Workstream A: Structured Chat Readability

### Objective
Turn long assistant text into readable, structured output with strong scanning behavior.

### Scope
1. Render markdown for chat responses in Mission and detached chat views.
2. Add readable measure constraints (`max-width` in character units) and spacing rhythm.
3. Style headings, lists, blockquotes, tables, inline code, and code blocks.
4. Add one-click copy button for code blocks.
5. Preserve current grayscale visual language.

### Implementation Targets
- `ui-rust-native/frontend/src/app.rs`
- `ui-rust-native/frontend/src/styles.css`
- `ui-rust-native/frontend/Cargo.toml` (markdown parser dependency)

### Acceptance Criteria
- Multi-paragraph responses render with clear visual hierarchy.
- Ordered and unordered lists are readable without wrapping collisions.
- Code blocks have dedicated container and copy action.
- Detached chat mirrors the same rendering and formatting quality.

## Workstream B: Company Branches Model and UX

### Objective
Move from implicit task grouping to explicit `Branch -> Mission -> Worker` operations.

### Scope
1. Introduce explicit branch identity in worker lifecycle payloads.
2. Support branch-aware mission grouping in Branches tab.
3. Update Workspace spawn flow to assign branch + mission.
4. Add branch-level metrics and status counts.
5. Add branch/misson filters in Activity and Workspace.

### Implementation Targets
- `core/src/agents.rs`
- `core/src/main.rs`
- `ui-rust-native/frontend/src/models/types.rs`
- `ui-rust-native/frontend/src/app.rs`

### Acceptance Criteria
- Operator can spawn a worker with branch + mission assignment.
- Branches tab shows hierarchy and aggregate worker state distribution.
- Activity can be filtered by selected branch and mission.
- Branch-driven workflow is usable without manual API calls.

## Delivery Sequence
1. Ship Workstream A first (readability uplift with low backend risk).
2. Ship Workstream B data model slice (`branch_id`) and API contract updates.
3. Ship Workstream B UI redesign in Branches and Workspace.
4. Run launch smoke + manual walkthrough after each slice.

## Validation Protocol Per Slice
- `cargo check --manifest-path "ui-rust-native/Cargo.toml"`
- `trunk build --release` (in `ui-rust-native/frontend`)
- `scripts/validate-launch.ps1 -UseStartMax`
- Manual operator walkthrough of touched tabs

## Definition of Done for Phase 5
- Chat output is structured and consistently readable for long responses.
- Branch orchestration is explicit and visible in UI hierarchy.
- No primary button in Mission/Branches/Workspace/Activity is a dead control.
- Launch + smoke remain green after final integration.
