---
id: TC-094
title: event_store_rdf_projection
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-032
phase: 1
runner: scripts/run-tc.sh
runner-args: "event-store-rdf-projection"
last-run: 2026-04-13T21:37:33.242635225+00:00
---

append aggregate events. Assert the product's SPARQL endpoint reflects the projected aggregate state within the projection latency budget.