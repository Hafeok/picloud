---
id: TC-282
title: Event subscription resource type receives filtered events
type: scenario
status: passing
runner: cargo-test
runner-args: "tc282_event_subscription_resource_type_receives_filtered_events"
validates:
  features: [FT-083]
  adrs: [ADR-022]
phase: 3
last-run: 2026-04-15T17:22:12.885893088+00:00
last-run-duration: 0.9s
---

## Description

Scenario test for FT-083: Exercises the full event subscription lifecycle.

1. Declares an EventSubscription resource for a specific source product and event_type
2. Subscribes to the event log with a filter matching the subscription's criteria
3. Emits a mix of events from different products and types
4. Asserts the subscriber receives ONLY the events matching the filter (product + event_type)
5. Verifies the EventSubscription resource transitions through lifecycle events (ResourceDeclared, ResourceReady)
6. Confirms the total event log contains all events regardless of per-subscription filtering