# Roadmap

## Current Product Direction

Kaizen MAX is centered on the Rust-native desktop and the Rust gateway.

The roadmap is focused on a usable operator product with persistent orchestration and native tooling, not on adding another frontend stack.

## Completed Foundation

- Rust-native desktop app established as the primary UI
- repo-based launcher and updater working on Windows
- Zeroclaw introduced as the runtime control plane
- persistent workers, branches, conversations, and heartbeats added
- detachable worker chats and detachable office view shipped

## Completed Native Tool Baseline

- native Gmail OAuth support added
- native report export added
- native lead research export added
- worker jobs can now execute tool steps and persist artifacts
- Integrations reflects real native tool readiness instead of placeholder text

## Current Hardening Track

### Product reliability

- tighten launcher and updater behavior
- expand smoke coverage for desktop and background worker behavior
- keep public docs aligned with the shipped product

### Orchestration quality

- improve Kaizen delegation and worker follow-up quality
- strengthen blocked/completed reporting for long-running work
- improve Crystal Ball and Mattermost operational visibility

### Attachment and context handling

- ship stronger multimodal routing when images are attached
- make provider-dependent image handling more transparent
- preserve useful context without bloating local state

## Next Native Tool Slices

- richer lead extraction and structured company/contact summaries
- Gmail handoff directly from researched leads
- CRM integrations
- native browser and scheduler tools
- shell and file tools under Zeroclaw

## Release Discipline

- stronger desktop and backend smoke suites
- release notes tied to `main`
- tighter deployment and rollback guidance
- cleaner public and private documentation separation

## Immediate Priorities

- finish Mattermost live validation with real credentials
- strengthen worker UI around artifacts and progress history
- improve multimodal routing for image-attached requests
- continue replacing compatibility-only tool paths with native Zeroclaw capabilities
