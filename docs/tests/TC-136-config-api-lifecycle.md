---
id: TC-136
title: config_api_lifecycle
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-043
phase: 1
runner: cargo-test
runner-args: "config_api_lifecycle"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 1.0s
failure-message: "No matching test function found (0 tests ran)"
---

apply a `config` resource with 5 entries. GET each entry via the HTTP API. Assert correct key, value, and type for each.