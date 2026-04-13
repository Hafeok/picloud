---
id: TC-137
title: config_live_reload
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-043
phase: 1
runner: picloud-test
runner-args: "config-live-reload"
---

update a config entry via the API. Assert `ConfigChanged` event emitted. Assert the workload SDK reflects the new value within 5 seconds without a process restart.