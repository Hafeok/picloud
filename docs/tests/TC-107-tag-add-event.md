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
runner: cargo-test
runner-args: "tag_add_event"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 1.2s
failure-message: "No matching test function found (0 tests ran)"
---

add a tag to a resource via `picloud tag add`. Assert a `TagAdded` event appears in the event log with the correct key, value, and resource IRI.