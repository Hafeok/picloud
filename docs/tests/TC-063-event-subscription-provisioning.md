---
id: TC-063
title: event_subscription_provisioning
type: scenario
status: passing
validates:
  features:
  - FT-005
  adrs:
  - ADR-022
phase: 1
runner: scripts/run-tc.sh
runner-args: "event-subscription-provisioning"
last-run: 2026-04-13T19:48:54.098720974+00:00
---

declare an `event-subscription` resource in a product file. Apply it. Assert the subscription IRI appears in the RDF graph and events from the source product are delivered.