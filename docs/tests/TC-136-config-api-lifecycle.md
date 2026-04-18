---
id: TC-136
title: config_api_lifecycle
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-043
phase: 1
runner: cargo-test
runner-args: "config_api_lifecycle"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 1.9s
---

apply a `config` resource with 5 entries. GET each entry via the HTTP API. Assert correct key, value, and type for each.