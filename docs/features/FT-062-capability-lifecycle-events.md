---
id: FT-062
title: Capability lifecycle events
phase: 3
status: complete
depends-on: []
adrs:
- ADR-055
tests:
- TC-269
- TC-326
domains: []
domains-acknowledged: {}
---

## Description

Capability state changes emit lifecycle events to the platform event log (ADR-055). These events are projected into the cluster RDF graph and are subscribable by any workload with appropriate permissions.

### Events

| Event | When emitted |
|---|---|
| `CapabilityDeclared` | Capability resource received and validated |
| `CapabilityReady` | At least one implementing Product is deployed and structurally conformant |
| `CapabilityImplementorAdded` | A new Product declares `implements` for an existing capability |
| `CapabilityImplementorRemoved` | An implementing Product is removed or stops implementing |
| `CapabilityUnfulfilled` | No implementing Product exists — all consumers receive this event |
| `CapabilityDeleted` | Capability removed (only when no consumers exist) |

### Consumer notification

When `CapabilityUnfulfilled` is emitted, all Products that declare a `capabilities` dependency on the affected capability receive the event on their event bus. This gives consumers observable signal when their dependency breaks.

### Auditing

All capability lifecycle events carry the capability IRI, the Product IRI (where applicable), and the capability version. The event log provides a complete history of which Products implemented which capabilities and when.
