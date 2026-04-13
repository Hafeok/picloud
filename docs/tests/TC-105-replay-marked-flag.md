---
id: TC-105
title: replay_marked_flag
type: scenario
status: passing
validates:
  features:
  - FT-002
  adrs:
  - ADR-035
phase: 1
runner: cargo-test
runner-args: "tc105_replay_marked_flag"
---

replay 100 events. Inspect the re-emitted events. Assert every replayed event carries `replay.is_replay: true` and a `replay.replay_id` that groups all events from the same replay operation.