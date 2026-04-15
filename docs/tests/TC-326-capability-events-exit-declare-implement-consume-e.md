---
id: TC-326
title: Capability events exit — declare, implement, consume events emitted
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc326_capability_events_exit_declare_implement_consume_events_emitted"
validates:
  features: [FT-062]
  adrs: [ADR-055]
phase: 3
last-run: 2026-04-15T14:01:42.604958848+00:00
last-run-duration: 0.8s
---

## Description

Exit criteria for FT-062: Verify that the three core capability lifecycle events
(CapabilityDeclared, CapabilityImplementorAdded, CapabilityConsumerAdded) are each
emitted and correctly projected into the RDF graph. Each event must produce the
expected triples — this is the minimum bar for the feature.