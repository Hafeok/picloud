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
runner: picloud-test
runner-args: "config-api-lifecycle"
---

apply a `config` resource with 5 entries. GET each entry via the HTTP API. Assert correct key, value, and type for each.