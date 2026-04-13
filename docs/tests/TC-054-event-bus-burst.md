---
id: TC-054
title: event_bus_burst
type: scenario
status: unimplemented
validates:
  features:
  - FT-008
  adrs:
  - ADR-018
phase: 1
---

product A emits 1000 events in a 10-second burst. Assert all 1000 are received by product B's subscriber with zero loss. Assert event IDs match.