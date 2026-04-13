---
id: TC-035
title: mounted_volume
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-012
phase: 1
runner: scripts/run-tc.sh
runner-args: "mounted-volume"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

allocate a mounted volume, attach it to a container at `/data`, write a sentinel file inside the container, restart the container, assert the sentinel file is present.