---
id: TC-065
title: subscription_lifecycle
type: scenario
status: passing
validates:
  features:
  - FT-005
  adrs:
  - ADR-022
phase: 1
runner: scripts/run-tc.sh
runner-args: "subscription-lifecycle"
last-run: 2026-04-13T19:48:54.098720974+00:00
---

delete the `event-subscription` resource. Assert events from the source product are no longer delivered and the subscription IRI is removed from the graph.