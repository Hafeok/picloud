---
id: FT-084
title: Platform-managed event routing between Products
phase: 3
status: complete
depends-on: []
adrs:
- ADR-018
- ADR-022
tests:
- TC-283
- TC-340
domains: []
domains-acknowledged: {}
---

## Description

The platform manages event routing between Products (ADR-018, ADR-022). Products never communicate directly — all inter-product events flow through the platform event bus, which enforces IAM, maintains audit trails, and handles delivery.

### Routing model

1. Source Product's workload emits an event to its product event bus
2. Platform identifies all `event-subscription` resources that match the event type
3. Platform validates IAM permissions for each subscription
4. Platform delivers the event to each subscribing Product's handler workload
5. Delivery is recorded in the platform event log

### Delivery guarantees

- **At-least-once** — events may be delivered more than once; handlers must be idempotent
- **Ordered per aggregate** — events for the same aggregate are delivered in order
- **No cross-aggregate ordering** — events for different aggregates may arrive out of order

### IAM enforcement

Every event delivery is IAM-checked:
- The source Product must permit the event type to be subscribable
- The subscribing workload must have a role that permits receiving the event
- Unauthorized deliveries are rejected and logged as `UnauthorisedEventDelivery`

### Dead letters

If a handler workload is unreachable (down, OOM-killed, node failed), events are retried with exponential backoff. After configurable retry exhaustion (default: 5 attempts), the event is dead-lettered and a `DeadLetterEvent` is emitted.

### Audit

The complete event routing chain is auditable:
- Source event (in source Product's log)
- Routing decision (in platform log)
- Delivery confirmation (in subscribing Product's log)
