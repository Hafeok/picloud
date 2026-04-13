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
last-run: 2026-04-13T21:47:42.689812716+00:00
---

apply a `config` resource with 5 entries. GET each entry via the HTTP API. Assert correct key, value, and type for each.