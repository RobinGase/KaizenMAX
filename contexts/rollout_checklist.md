# Kaizen MAX Context Rollout Checklist

## Phase 1 - Context Baseline
- [ ] Confirm brand/product labels are `Kaizen` and `MAX` in UI and config.
- [ ] Set Kaizen as default main agent in runtime settings.
- [ ] Set runtime engine default to `zeroclaw` and keep compatibility adapter off.
- [ ] Set `MAX_SUBAGENTS=5` (or lower) and disable auto-spawn.
- [ ] Confirm no-WSL baseline for desktop workflow.

## Phase 2 - Prompt and Template Binding
- [ ] Bind `templates/kaizen_system_prompt.md` to ZeroClaw main prompt setting.
- [ ] Bind `templates/subagent_system_prompt.md` to spawned-agent template setting.
- [ ] Verify all spawned agents inherit alignment and gate rules.

## Phase 3 - Policy Binding
- [ ] Bind `policies/review_gate_policy.yaml` to workflow gate engine.
- [ ] Bind `policies/agent_control_policy.yaml` to spawn/lifecycle controls.
- [ ] Bind `policies/crystal_ball_event_policy.yaml` to Mattermost event publisher.

## Phase 4 - Hard Gate Verification
- [ ] Attempt finalize without `Passed Reasoners Test` and confirm block.
- [ ] Attempt deploy without human smoke test and confirm block.
- [ ] Confirm failed review forces return to execute/review.

## Phase 5 - UI Behavior Verification
- [ ] Kaizen main chat always visible.
- [ ] New sub-agent chat panels appear closed by default.
- [ ] Click-to-toggle open/close works for each agent panel.
- [ ] Crystal Ball feed is draggable/resizable and receives live events.

## Phase 6 - Security and Admin Controls
- [ ] Confirm `.env` admin variables use `ADMIN_` prefix.
- [ ] Validate secret masking in Crystal Ball feed.
- [ ] Validate audit event logging for gate decisions and state transitions.

## Phase 7 - Remote Core Validation
- [ ] Confirm native or remote ZeroClaw core is reachable from native Windows UI.
- [ ] Confirm low local RAM footprint during normal usage.
- [ ] Confirm provider-hosted inference calls succeed end to end.

## Exit Criteria
- [ ] Main-agent-first workflow is stable.
- [ ] Sub-agent orchestration is manual and bounded.
- [ ] Hard gates enforce integrity.
- [ ] Crystal Ball communication visibility is operational.
- [ ] No-WSL desktop experience is functional for day-to-day use.
