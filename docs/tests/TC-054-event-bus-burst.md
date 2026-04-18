---
id: TC-054
title: event_bus_burst
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-018
phase: 1
runner: scripts/run-tc.sh
runner-args: "event-bus-burst"
last-run: 2026-04-18T14:42:27.228653472+00:00
last-run-duration: 0.0s
---

product A emits 1000 events in a 10-second burst. Assert all 1000 are received by product B's subscriber with zero loss. Assert event IDs match.