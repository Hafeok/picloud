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
last-run: 2026-04-18T11:08:48.461897691+00:00
last-run-duration: 0.0s
---

append aggregate events. Assert the product's SPARQL endpoint reflects the projected aggregate state within the projection latency budget.