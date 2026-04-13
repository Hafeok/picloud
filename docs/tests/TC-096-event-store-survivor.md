---
id: TC-096
title: event_store_survivor
type: scenario
status: failing
validates:
  features:
  - FT-008
  adrs:
  - ADR-032
phase: 1
runner: picloud-test
runner-args: "event-store-survivor"
---

append 100 events, kill the Raft leader, assert all 100 events readable after leader failover.