# Kaizen MAX Context Rollout Checklist

## Phase 1 - Context Baseline
- [x] Confirm brand and product labels are `Kaizen` and `MAX` in UI and config.
- [x] Set Kaizen as default main agent in runtime settings.
- [x] Set runtime engine default to `zeroclaw` and keep compatibility adapter off.
- [x] Set max sub agent default to `5` and disable auto spawn.
- [x] Confirm no WSL baseline for desktop workflow.

## Phase 2 - Prompt and Template Binding
- [ ] Bind `templates/kaizen_system_prompt.md` to ZeroClaw main prompt setting.
- [ ] Bind `templates/subagent_system_prompt.md` to spawned agent template setting.
- [ ] Verify all spawned agents inherit alignment and gate rules through live prompt binding.

## Phase 3 - Policy Binding
- [x] Bind `policies/review_gate_policy.yaml` to workflow gate engine behavior.
- [x] Bind `policies/agent_control_policy.yaml` to spawn and lifecycle controls.
- [x] Bind `policies/crystal_ball_event_policy.yaml` core controls to event masking and audit archive behavior.

## Phase 4 - Hard Gate Verification
- [x] Attempt finalize without `Passed Reasoners Test` and confirm block.
- [x] Attempt deploy without human smoke test and confirm block.
- [x] Confirm failed review blocks progression until conditions are met.

## Phase 5 - UI Behavior Verification
- [x] Kaizen main chat always visible.
- [x] New sub agent chat panels appear closed by default unless setting is changed.
- [x] Click toggle open and close works for each agent panel.
- [x] Crystal Ball feed is draggable and resizable and receives live events.

## Phase 6 - Security and Admin Controls
- [x] Confirm `.env` admin variables use `ADMIN_` prefix.
- [x] Validate secret masking in Crystal Ball feed.
- [x] Validate audit event logging for gate decisions and state transitions.
- [x] Validate local archive integrity reports and optional HMAC coverage reporting.

## Phase 7 - Remote Core Validation
- [ ] Confirm native or remote ZeroClaw core is reachable from native Windows UI.
- [ ] Confirm low local RAM footprint during normal usage.
- [ ] Confirm provider-hosted inference calls succeed end to end.

## Exit Criteria
- [x] Main agent first workflow is stable in current development branch.
- [x] Sub agent orchestration is manual and bounded.
- [x] Hard gates enforce integrity checks.
- [x] Crystal Ball communication visibility is operational with local archive controls.
- [ ] Full remote core and production smoke operations are validated.

## Notes

- Checklist reflects current `DevMaster` status.
- Prompt injection runtime binding is the largest remaining context task.
