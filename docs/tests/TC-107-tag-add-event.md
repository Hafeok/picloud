---
id: TC-107
title: tag_add_event
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-036
phase: 1
---

add a tag to a resource via `picloud tag add`. Assert a `TagAdded` event appears in the event log with the correct key, value, and resource IRI.