---
id: TC-035
title: mounted_volume
type: scenario
status: failing
validates:
  features:
  - FT-004
  adrs:
  - ADR-012
phase: 1
runner: picloud-test
runner-args: "mounted-volume"
---

allocate a mounted volume, attach it to a container at `/data`, write a sentinel file inside the container, restart the container, assert the sentinel file is present.