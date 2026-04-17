---
id: TC-283
title: Platform routes events between products via subscription
type: scenario
status: passing
runner: cargo-test
runner-args: "tc283_platform_routes_events_between_products_via_subscription"
validates:
  features: [FT-084]
  adrs: [ADR-022]
phase: 3
last-run: 2026-04-17T10:17:46.909230008+00:00
last-run-duration: 0.8s
---

## Description

Scenario test for FT-084: Exercises the full platform-managed event routing
lifecycle between Products via EventSubscription resources.

1. Create an EventSubscription: fulfillment-service subscribes to OrderCreated
   events from order-service, targeting handler "order-processor"
2. Register the subscription with the PlatformEventRouter
3. Emit a mix of events — some matching, some from other products/types
4. Route each event through the router
5. Verify: only matching events produce SubscriptionEventRouted events
6. Verify: routed events are scoped to the subscriber product
7. Verify: original payload, event type, and handler are preserved
8. Verify: the subscriber can receive routed events via product-scoped filter
9. Verify: unregistering a subscription stops further routing
10. Verify: multiple subscriptions from different products are all served