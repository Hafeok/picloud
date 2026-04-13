---
id: TC-155
title: snapshot_retention
type: scenario
status: failing
validates:
  features:
  - FT-004
  adrs:
  - ADR-047
phase: 1
runner: picloud-test
runner-args: "snapshot-retention"
---

take 35 daily snapshots (accelerated in CI with a short schedule). Run the retention enforcement. Assert exactly 30 daily snapshots remain (retention policy: `daily: 30`).