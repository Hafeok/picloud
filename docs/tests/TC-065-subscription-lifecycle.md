---
id: TC-065
title: subscription_lifecycle
type: scenario
status: unimplemented
validates:
  features:
  - FT-005
  adrs:
  - ADR-022
phase: 1
---

delete the `event-subscription` resource. Assert events from the source product are no longer delivered and the subscription IRI is removed from the graph.