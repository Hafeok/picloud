---
id: TC-136
title: config_api_lifecycle
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-043
phase: 1
---

apply a `config` resource with 5 entries. GET each entry via the HTTP API. Assert correct key, value, and type for each.