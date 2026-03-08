# Kaizen MAX Architecture

## Overview

Kaizen MAX is split into two primary runtime layers:

- `core/`: the Rust gateway and domain runtime
- `ui-rust-native/`: the Rust-native Mission Control desktop app

The desktop application is the operator surface. The gateway owns the runtime state.

## Runtime Layers

### Desktop

The desktop app is built with:

- Tauri v2 host
- Leptos frontend
- local launcher and repo-based updater

The desktop owns:

- main Mission Control shell
- detachable worker chat windows
- detachable office window
- release update checks and apply flow
- local shell-to-core request bridge

### Gateway

The gateway owns:

- Zeroclaw runtime state
- provider resolution and model routing
- conversation history
- branch, mission, and worker registry
- background worker runner and heartbeats
- Crystal Ball events
- optional Mattermost publishing
- gate state and workflow policy
- native tool execution

## Zeroclaw

Zeroclaw is the main runtime control plane.

It is responsible for:

- active provider selection
- model routing
- provider readiness
- native tool status
- worker tool-step execution state

Zeroclaw is not a cosmetic alias. It is also not yet full OpenClaw parity.

## Providers

Providers are treated as inference backends behind Zeroclaw.

Current supported paths:

- `codex-cli`
- `openai`
- `anthropic`
- `gemini`
- `nvidia`
- `gemini-cli`

## Native Tool Runtime

Current native Zeroclaw tools:

- `gmail`
- `reports`
- `leads`

Current tool behavior:

- Gmail uses app-managed Google OAuth and supports draft/send flows.
- Reports export structured CSV and XLSX artifacts under `data/worker_artifacts/`.
- Leads research fetches target sites, extracts public contact context, and exports structured artifacts.

## OpenClaw Compatibility

OpenClaw remains a selective fallback bridge for chosen tool paths.

Current intent:

- prefer native Zeroclaw capabilities first
- use OpenClaw only where compatibility is explicitly allowed
- keep UI status honest about what is native, what is borrowed, and what is not implemented

## Orchestration

Kaizen is the top-level operator agent.

Workers are persistent named entities with:

- `branch_id`
- `mission_id`
- `task_id`
- status
- conversation history
- runtime job history

The main Kaizen chat can dispatch work to named workers and the background runner can execute delegated work independently of the request/response path.

## Worker Runtime

The worker runtime currently includes:

- persistent job queue
- worker claim/lease model
- heartbeat tracking
- tool-step persistence
- artifact persistence
- completion and blocked-state reporting back into worker conversations and Crystal Ball

## Persistence

Current local persistence is file-backed under `data/`.

Important persisted state:

- agent registry
- conversation history
- worker runtime snapshot
- Gmail and Gemini OAuth tokens
- event archive
- exported worker artifacts

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
- image attachments in request transport

Important limitation:

- image transport is implemented
- true image understanding still depends on the active provider path
- CLI-based routes such as `codex-cli` remain more text-centric than a fully multimodal API provider

## Observability

Crystal Ball is the event spine.

It records:

- requests
- responses
- delegation
- worker progress
- gate transitions
- lifecycle actions

Mattermost is an optional outbound publication target for those events.

## Release Model

This repository is the release source.

- `main` is the public release branch
- the desktop updater compares the local checkout against `origin/main`
- update apply is blocked when the local checkout is dirty or not on `main`
