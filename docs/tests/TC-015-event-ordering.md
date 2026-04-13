---
id: TC-015
title: event_ordering
type: scenario
status: failing
validates:
  features:
  - FT-002
  adrs:
  - ADR-004
phase: 1
runner: picloud-test
runner-args: "event_ordering"
---

apply 50 resources in parallel from two CLI clients, assert the event log index is strictly monotonic and the final RDF graph reflects all 50 resources with no duplicates or gaps.