---
id: TC-337
title: Event replay exit — shadow projection created and swapped atomically
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc337_event_replay_exit_shadow_projection_created_and_swapped_atomically"
validates:
  features:
    - FT-081
  adrs:
    - ADR-035
phase: 3
last-run: 2026-04-15T17:10:57.028211647+00:00
last-run-duration: 0.7s
---

## Description

Exit-criteria gate for FT-081. Given a sequence of platform events, a replay
operation MUST: (a) build a shadow projection in a dedicated named graph,
(b) atomically swap the shadow into the target graph (clearing stale triples),
(c) leave no residual shadow triples, (d) leave the live default graph
unmodified, (e) report correct event counts, (f) create shadow graph metadata
via start_replay, and (g) support ReplayMetadata marking on replayed events
to distinguish them from live events.