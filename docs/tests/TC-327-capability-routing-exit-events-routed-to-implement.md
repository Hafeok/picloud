---
id: TC-327
title: Capability routing exit — events routed to implementing product
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc327_capability_routing_exit_events_routed_to_implementing_product"
validates:
  features: [FT-063]
  adrs: [ADR-055]
phase: 3
last-run: 2026-04-17T08:52:32.819120182+00:00
last-run-duration: 0.5s
---

## Description

Exit criteria for FT-063: Verifies the minimum bar for capability-aware event routing —
a declared capability with an implementor correctly resolves at dispatch time, and the
resulting `CapabilityEventRouted` event is scoped to the implementing product.

The test declares a capability, adds an implementor, routes an input event, and asserts:
- Exactly one `CapabilityEventRouted` event is appended to the event log.
- The routed event's `product` field matches the implementing product name.
- The routed event payload references the capability and the resolved implementor.
- The original event type and payload are preserved in the routed event.