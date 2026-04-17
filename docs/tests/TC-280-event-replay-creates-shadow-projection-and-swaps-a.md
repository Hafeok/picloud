---
id: TC-280
title: Event replay creates shadow projection and swaps atomically
type: scenario
status: passing
runner: cargo-test
runner-args: "tc280_event_replay_creates_shadow_projection_and_swaps_atomically"
validates:
  features:
    - FT-081
  adrs:
    - ADR-035
phase: 3
last-run: 2026-04-17T10:15:56.069363902+00:00
last-run-duration: 0.7s
---

## Description

Verifies that executing an event replay creates a shadow named graph, projects
replayed events into it, and then atomically swaps the shadow triples into the
target product graph. After the swap the shadow graph is empty (cleaned up),
the target graph contains the replayed triples, and the live default graph is
unaffected. Also verifies that replayed events can carry ReplayMetadata and
that this metadata round-trips through serde.