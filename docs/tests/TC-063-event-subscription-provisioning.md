---
id: TC-063
title: event_subscription_provisioning
type: scenario
status: failing
validates:
  features:
  - FT-005
  adrs:
  - ADR-022
phase: 1
runner: picloud-test
runner-args: "event-subscription-provisioning"
---

declare an `event-subscription` resource in a product file. Apply it. Assert the subscription IRI appears in the RDF graph and events from the source product are delivered.