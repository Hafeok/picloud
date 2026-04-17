---
id: TC-270
title: Event routed to implementing product resolved by capability IRI
type: scenario
status: passing
runner: cargo-test
runner-args: "tc270_event_routed_to_implementing_product_resolved_by_capability_iri"
validates:
  features: [FT-063]
  adrs: [ADR-055]
phase: 3
last-run: 2026-04-17T08:52:32.819120182+00:00
last-run-duration: 0.5s
---

## Description

Scenario test for FT-063: Exercises the full capability-aware event routing flow.
Declares a capability with an IRI, registers an implementor product, adds a consumer,
then routes an input event through the capability. Verifies that:

1. The CapabilityResolverImpl finds the implementing product via SPARQL on the RDF graph.
2. A `CapabilityEventRouted` event is appended to the event log.
3. The routed event is scoped to the implementor's product (the `product` field matches the implementor).
4. The routed event payload preserves the original event type, ID, and payload.
5. Correlation ID and source IRI propagate from input to routed event.
6. Version satisfaction is enforced — routing fails when minVersion exceeds the capability version.
7. Routing to a nonexistent capability fails with an error.