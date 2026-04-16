---
id: FT-060
title: implements field on product — structural SHACL conformance validated at resource apply time
phase: 3
status: complete
depends-on: []
adrs:
- ADR-055
tests:
- TC-232
domains: []
domains-acknowledged: {}
---

## Description

Products declare `implements` to assert that they fulfil a capability contract (ADR-055). At `resource apply` time, the platform validates structural SHACL conformance — the implementing Product must have the required event subscriptions and emit the required output events.

### Resource syntax

```bicep
product 'photo-app' = {
  version: '2.0.0'
  implements: ['gps-to-place@1.0.0']
}
```

### Validation at deploy time

1. The referenced capability must exist in the cluster
2. The Product must have an `event-subscription` to the capability's `input` event type
3. The Product must emit the capability's `output` event type
4. SHACL shapes from the capability are validated against the Product's ontology — structural conformance is required

If any validation fails, `resource apply` is rejected with a specific conformance error. No partial deployment occurs.

### RDF projection

```turtle
<https://picloud.local/products/photo-app>
    picloud:implements <https://picloud.local/capabilities/gps-to-place> .

<https://picloud.local/capabilities/gps-to-place>
    picloud:implementedBy <https://picloud.local/products/photo-app> .
```

### Enforcement direction

A Product that `implements` a capability cannot also declare a `capabilities` dependency on a capability it does not itself implement. This prevents circular dependency chains.
