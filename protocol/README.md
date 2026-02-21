# protocol

This directory contains alignment and protocol assets used by the Kaizen MAX runtime.

## Purpose

- Keep Nex alignment constraints visible and auditable.
- Track parity between Kaizen native runtime capabilities and protocol level controls.
- Document decisions for native enforcement, bridge behavior, and optional adapters.

## Current Artifacts

- `collision_matrix/phase_b_initial_collision_matrix.md`
  - Current parity snapshot across gate controls, Crystal Ball controls, and audit controls.

- `mcp/`
  - Reserved for MCP definitions that are required after parity validation.

- `alignment/`
  - Reserved for alignment packages that must be injected into runtime prompts and policies.

## Integration Flow

1. Inventory native runtime controls in `core/`.
2. Compare with Nex alignment expectations.
3. Keep equivalent controls native where possible.
4. Bridge only missing controls.
5. Place optional compatibility work in `compat/` if native parity is not feasible.

## Notes

- The current runtime already enforces gate transitions, spawn constraints, event masking, and local archive integrity in Rust.
- Mattermost bridge behavior is available and now includes validation and smoke test endpoints.
