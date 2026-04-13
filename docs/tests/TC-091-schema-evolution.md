---
id: TC-091
title: schema_evolution
type: scenario
status: passing
validates:
  features:
  - FT-002
  adrs:
  - ADR-031
phase: 1
---

emit 100 events under schema v1. Deploy a v2 projector that handles both v1 and v2. Replay the log. Assert the v2 projector correctly processes all v1 events.