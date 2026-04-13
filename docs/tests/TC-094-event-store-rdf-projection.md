---
id: TC-094
title: event_store_rdf_projection
type: scenario
status: failing
validates:
  features:
  - FT-008
  adrs:
  - ADR-032
phase: 1
runner: picloud-test
runner-args: "event-store-rdf-projection"
---

append aggregate events. Assert the product's SPARQL endpoint reflects the projected aggregate state within the projection latency budget.