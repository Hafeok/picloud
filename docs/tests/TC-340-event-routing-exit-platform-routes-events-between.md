---
id: TC-340
title: Event routing exit — platform routes events between products
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc340_event_routing_exit_platform_routes_events_between_products"
validates:
  features: [FT-084]
  adrs: [ADR-022]
phase: 3
last-run: 2026-04-17T10:17:46.909230008+00:00
last-run-duration: 0.8s
---

## Description

Exit-criteria test for FT-084: Minimum bar for the feature. The platform
correctly routes events from a source product to a subscriber product via
EventSubscription resources.

1. A valid EventSubscription can be registered with the router
2. Matching events (source product + event_type) are routed
3. Routed events are scoped to the subscriber product
4. Original payload is preserved verbatim in the routed event
5. Non-matching events (wrong type, wrong product, no product) are NOT routed
6. Subscriber can receive routed events via product-scoped event filter
7. Subscription IRI is tracked in each routed event