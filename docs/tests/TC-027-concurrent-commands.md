---
id: TC-027
title: concurrent_commands
type: scenario
status: unimplemented
validates:
  features:
  - FT-002
  adrs:
  - ADR-008
phase: 1
---

emit 10 `resource apply` commands concurrently from separate CLI processes. Assert all 10 terminal events arrive with matching correlation IDs and no events are cross-contaminated.