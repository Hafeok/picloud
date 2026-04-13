---
id: TC-052
title: inter_product_event_delivery
type: scenario
status: unimplemented
validates:
  features:
  - FT-008
  adrs:
  - ADR-018
phase: 1
---

product A emits an event to the platform bus. Product B has a declared `event-subscription` resource for that event type. Assert product B's workload receives the event within 5 seconds. Assert event appears in the RDF graph.