---
id: TC-052
title: inter_product_event_delivery
type: scenario
status: failing
validates:
  features:
  - FT-008
  adrs:
  - ADR-018
phase: 1
runner: picloud-test
runner-args: "inter-product-event-delivery"
---

product A emits an event to the platform bus. Product B has a declared `event-subscription` resource for that event type. Assert product B's workload receives the event within 5 seconds. Assert event appears in the RDF graph.