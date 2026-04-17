---
id: TC-052
title: inter_product_event_delivery
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-018
phase: 1
runner: scripts/run-tc.sh
runner-args: "inter-product-event-delivery"
last-run: 2026-04-17T13:58:22.735189315+00:00
last-run-duration: 0.1s
---

product A emits an event to the platform bus. Product B has a declared `event-subscription` resource for that event type. Assert product B's workload receives the event within 5 seconds. Assert event appears in the RDF graph.