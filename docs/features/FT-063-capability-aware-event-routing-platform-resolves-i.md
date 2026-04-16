---
id: FT-063
title: Capability-aware event routing — platform resolves implementing Product at dispatch time
phase: 3
status: complete
depends-on: []
adrs:
- ADR-055
- ADR-018
tests:
- TC-270
- TC-327
- TC-206
domains: []
domains-acknowledged: {}
---

## Description

The platform resolves which Product implements a required capability and routes events accordingly (ADR-055). When a consumer emits an event targeting a capability's input event type, the platform dispatches it to the implementing Product's event bus.

### Routing logic

1. Consumer emits an event matching a capability's `input` event type
2. Platform looks up all Products that `implement` the capability
3. Platform selects the implementor with the highest version satisfying the consumer's `minVersion` constraint
4. Event is routed to the selected implementor's event bus
5. Implementor processes the event and emits the capability's `output` event type
6. Output event is routed back to the consumer (and any other subscribers)

### One active implementor

In the current phase, one active implementor per capability is selected at dispatch time. If multiple Products implement the same capability, the highest-version implementor wins. Future phases may support load balancing across multiple implementors.

### Failure handling

If the selected implementor is unavailable (workload down, node failed), the event is queued and retried. If no implementor exists, `CapabilityUnfulfilled` is emitted and the event is dead-lettered.

### Integration with data products

A capability's output event is a first-class trigger for data product projection rebuilds (ADR-056). The capability is the operational act; the data product is the analytical record.
