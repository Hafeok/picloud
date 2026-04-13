---
id: TC-107
title: tag_add_event
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-036
phase: 1
runner: picloud-test
runner-args: "tag-add-event"
---

add a tag to a resource via `picloud tag add`. Assert a `TagAdded` event appears in the event log with the correct key, value, and resource IRI.