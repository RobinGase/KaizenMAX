# compat

This directory is reserved for optional compatibility adapters.

## Current Status

- Compatibility mode is disabled by default.
- Native runtime behavior is implemented in Rust through Kaizen.
- This directory is currently a reserved extension point.

## When to Use

- Add an adapter only if a required capability cannot be delivered in native runtime modules.
- Keep adapters isolated from the default execution path.

## Control Model

- Compatibility toggles are controlled by runtime settings.
- No adapter should load unless explicitly enabled by the operator.
- Node runtime remains outside the default stack.
