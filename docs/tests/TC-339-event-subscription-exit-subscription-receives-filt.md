---
id: TC-339
title: Event subscription exit — subscription receives filtered events
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc339_event_subscription_exit_subscription_receives_filtered_events"
validates:
  features: [FT-083]
  adrs: [ADR-022]
phase: 3
last-run: 2026-04-15T17:22:12.885893088+00:00
last-run-duration: 0.8s
---

## Description

Exit-criteria test for FT-083: Minimum bar for the event-subscription resource type.

1. Verifies EventSubscription resource type has all required fields (event_type, source_product_iri, handler_name)
2. Creates a filtered subscription on the EventLog matching the subscription's declared criteria
3. Emits matching events (correct product + event_type) and non-matching events (wrong type, wrong product, no product)
4. Asserts that ONLY the matching events are delivered to the filtered subscriber
5. Confirms payload integrity is preserved through filtering
6. Verifies the full event log retains all events regardless of per-subscription filtering