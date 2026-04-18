---
id: TC-096
title: event_store_survivor
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-032
phase: 1
runner: scripts/run-tc.sh
runner-args: "event-store-survivor"
last-run: 2026-04-18T13:20:29.293271188+00:00
last-run-duration: 0.0s
---

append 100 events, kill the Raft leader, assert all 100 events readable after leader failover.