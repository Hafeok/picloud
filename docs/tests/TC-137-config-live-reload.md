---
id: TC-137
title: config_live_reload
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-043
phase: 1
runner: cargo-test
runner-args: "config_live_reload"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 1.1s
---

update a config entry via the API. Assert `ConfigChanged` event emitted. Assert the workload SDK reflects the new value within 5 seconds without a process restart.