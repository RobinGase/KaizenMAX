# compat/

Optional compatibility adapters for Kaizen MAX.

## Status
**Disabled by default.** The baseline stack uses ZeroClaw (Rust) natively. This directory is reserved for:

- OpenClaw/Node bridge adapters (if a required feature is missing from ZeroClaw).
- Any future protocol translation layers.

## Policy
- Adapters here are opt-in only, controlled via `config/defaults.json` (`openclaw_compat_enabled`).
- No adapter in this directory should be loaded unless the user explicitly enables it in settings.
- Node.js is **not** part of the default runtime stack.
