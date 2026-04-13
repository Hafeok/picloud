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
---

append 100 events, kill the Raft leader, assert all 100 events readable after leader failover.