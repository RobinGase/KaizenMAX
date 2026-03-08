# Kaizen MAX Architecture

## Overview

Kaizen MAX is split into two primary runtime layers:

- `core/`: the Rust gateway and domain runtime
- `ui-rust-native/`: the Rust-native Mission Control desktop app

The desktop UI is the operator surface. The gateway is the source of truth.

## Runtime Model

### Desktop

The desktop app is built with:

- Tauri v2 host
- Leptos frontend
- local launcher and repo-based updater

The desktop owns:

- detachable agent windows
- detachable office window
- release update checks and apply flow
- local shell-to-core request bridge

### Gateway

The gateway owns:

- Zeroclaw runtime state
- provider resolution
- conversation history
- branch, mission, and worker registry
- Crystal Ball events
- Mattermost publishing
- gate state and workflow policy

## Key Domain Boundaries

### 1. Zeroclaw

Zeroclaw is the main runtime control plane. It is responsible for:

- active provider selection
- model routing
- provider readiness
- tool inventory exposure

It is not just a cosmetic alias anymore, but it is also not full standalone OpenClaw parity yet.

### 2. Providers

Providers are treated as execution backends behind Zeroclaw.

Current supported paths:

- `codex-cli`
- `openai`
- `anthropic`
- `gemini`
- `nvidia`
- `gemini-cli`

### 3. OpenClaw fallback

OpenClaw is integrated as a fallback tool bridge, not as the primary runtime.

Current intent:

- prefer local Zeroclaw behavior
- fall back to OpenClaw only for allowed tool paths
- keep the UI honest about which tools are ready, borrowed, or still planned

### 4. Orchestration

Kaizen is the top-level operator agent.

Workers are persistent named entities with:

- `branch_id`
- `mission_id`
- `task_id`
- status
- conversation history

The main Kaizen chat can dispatch work to named workers and record that delegation through Crystal Ball.

## Persistence

Current local persistence is file-backed under `data/`.

Important persisted state:

- agent registry
- conversation history
- Gemini OAuth tokens
- event archive

## Windowing Model

The desktop app supports:

- main shell
- detached worker chat windows
- detached office window

Detached window state is queried from Tauri instead of being inferred by frontend-only state.

## Chat Model

Chat supports:

- streaming replies
- main Kaizen chat
- direct worker chat
- image attachments in the request transport

Current important limitation:

- image transport is implemented
- true image understanding depends on the active provider path
- CLI-based routes like `codex-cli` still behave more like metadata-aware text paths than full multimodal vision paths

## Crystal Ball and Mattermost

Crystal Ball is the system event spine.

It records:

- requests
- responses
- delegation
- gate transitions
- lifecycle actions

Mattermost is an optional outbound publication target for Crystal Ball events.

## Release Model

This repo is the release source.

- `main` is the release branch
- the desktop updater compares the local checkout against `origin/main`
- update apply is blocked when the local checkout is dirty or not on `main`
