# Kaizen MAX Vision

## Purpose

Kaizen MAX is an operator-grade desktop for running, reviewing, and shipping multi-agent work with clear runtime state, persistent staff, and practical governance.

It is not intended to be a generic chat shell.

## North Star

Deliver a native Mission Control where one operator can:

- plan work
- delegate to persistent workers
- monitor background execution
- review outputs and artifacts
- move work through gates with full observability

## Product Principles

- ship vertical slices that work end to end
- keep runtime state explicit and inspectable
- prefer typed contracts over hidden behavior
- make operator decisions clear and reversible where possible
- keep community-facing documentation clean and public-safe

## Operator Value

- **Operator**: sees company state, worker state, and tool readiness in one place
- **Builder**: can run real work through Kaizen, workers, and native tools without shell hopping
- **Reviewer**: can inspect events, artifacts, and gate state before approving progression

## Product Direction

Kaizen MAX is built around:

- a Rust gateway as the source of truth
- a Rust-native desktop Mission Control
- Zeroclaw as the runtime control plane
- native tools for business and operational work

## Operating Model

- Kaizen acts as the executive orchestrator
- branches contain missions
- missions coordinate workers
- workers can execute background jobs and return artifacts

The interface should make delegation, bottlenecks, blocked work, and output quality obvious at a glance.

## Delivery Standard

A feature is only complete when:

- the UI path works
- the backend path works
- runtime state persists correctly
- the result is testable
- the public documentation is current
