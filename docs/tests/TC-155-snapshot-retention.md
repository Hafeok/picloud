---
id: TC-155
title: snapshot_retention
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-047
phase: 1
---

take 35 daily snapshots (accelerated in CI with a short schedule). Run the retention enforcement. Assert exactly 30 daily snapshots remain (retention policy: `daily: 30`).