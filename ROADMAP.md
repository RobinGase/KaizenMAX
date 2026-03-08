# Roadmap

## Current Product Direction

Kaizen MAX is now centered on the Rust-native desktop and the Rust gateway.

The current roadmap is about turning that base into a stable operator product, not starting another frontend rewrite.

## Phase 1 - Completed Foundation

- Rust-native desktop app established as the primary UI
- repo-based launcher and updater working on Windows
- Zeroclaw runtime introduced as the top-level control plane
- Codex CLI path working as the default local route
- persistent workers, branches, and conversations added

## Phase 2 - Completed Operator Usability Baseline

- streaming chat in the desktop app
- detachable worker chat windows
- detachable office window support
- resizeable panes and office workbench
- simpler integrations flow centered on Zeroclaw

## Phase 3 - Current Hardening Track

### Product reliability

- tighten launcher and updater behavior
- expand smoke coverage for desktop window behavior
- keep repo/docs aligned with the shipped product

### Orchestration quality

- improve Kaizen's executive reasoning and delegation quality
- keep worker naming, branch structure, and routing stable across restarts
- strengthen Crystal Ball and Mattermost operational flow

### Attachment and context handling

- ship true multimodal routing when images are attached
- make provider fallback behavior more transparent
- preserve useful context without bloating local state

## Phase 4 - Zeroclaw Tool Expansion

- implement local Zeroclaw tools instead of leaving them as planned markers
- expand beyond chat into shell, files, browser, and scheduler
- decide which capabilities stay local and which remain OpenClaw fallback paths

## Phase 5 - Business Tooling

- Gmail send/read
- lead capture
- CRM integration
- operator-friendly tool setup inside the desktop app

## Phase 6 - Release Discipline

- stronger desktop and backend smoke suites
- release notes tied to `main`
- tighter documentation around deployment, rollback, and support

## Immediate Priorities

- commit and ship the current Rust-native UX pass
- finish live Mattermost validation with real credentials
- improve Office screenshots and public docs
- decide the next Zeroclaw tool slice
