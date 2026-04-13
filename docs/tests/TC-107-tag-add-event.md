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
runner: cargo-test
runner-args: "tag_add_event"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

add a tag to a resource via `picloud tag add`. Assert a `TagAdded` event appears in the event log with the correct key, value, and resource IRI.