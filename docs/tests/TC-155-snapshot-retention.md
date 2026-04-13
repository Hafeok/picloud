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
runner: scripts/run-tc.sh
runner-args: "snapshot-retention"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

take 35 daily snapshots (accelerated in CI with a short schedule). Run the retention enforcement. Assert exactly 30 daily snapshots remain (retention policy: `daily: 30`).