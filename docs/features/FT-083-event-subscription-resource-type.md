---
id: FT-083
title: event-subscription resource type
phase: 3
status: planned
depends-on: []
adrs:
- ADR-022
- ADR-018
tests:
- TC-282
- TC-339
domains: []
domains-acknowledged: {}
---

## Description

An `event-subscription` is a declared resource that binds a Product to another Product's event stream (ADR-022). Subscriptions are explicit, auditable, and version-controlled — no runtime subscriptions without a resource declaration.

### Resource syntax

```bicep
event-subscription 'user-created' = {
  product: 'photo-app'
  source: 'user-service@1.0.0'
  event: 'UserCreated'
  handler: 'api-server'
}
```

### Validation at deploy time

- The source Product must exist in the cluster
- The declared event type must exist in the source Product's event schema
- The handler workload must exist within the subscribing Product

### Platform behaviour

1. The platform provisions the subscription between the two Products' event buses
2. When the source Product emits a `UserCreated` event, the platform routes it to the subscribing Product's handler workload
3. Delivery is at-least-once — the handler must be idempotent

### IAM

The subscribing workload must have a role that permits receiving events from the source Product. The platform enforces this at subscription creation time.

### Lifecycle

- Subscriptions are created on `resource apply` and destroyed on Product deletion (cascading)
- The subscription itself is a resource with an IRI: `https://picloud.local/products/photo-app/event-subscriptions/user-created`

### Visibility

All event subscriptions are visible in the cluster graph:
```sparql
SELECT ?subscriber ?source ?event WHERE {
  ?sub a picloud:EventSubscription ;
       picloud:subscriber ?subscriber ;
       picloud:source ?source ;
       picloud:eventType ?event .
}
```
