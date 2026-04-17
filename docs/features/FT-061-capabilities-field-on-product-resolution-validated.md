---
id: FT-061
title: capabilities field on product — resolution validated at resource apply time
phase: 3
status: planned
depends-on: []
adrs:
- ADR-055
tests:
- TC-232
domains: []
domains-acknowledged: {}
---

## Description

Products declare `capabilities` to express a dependency on a named capability being available in the cluster (ADR-055). The consumer binds to the capability contract, not to any specific implementing Product.

### Resource syntax

```bicep
product 'maps-app' = {
  version: '1.0.0'
  capabilities: [
    { capability: 'gps-to-place', minVersion: '1.0.0' }
  ]
}
```

### Validation at deploy time

- The referenced capability must exist at the required `minVersion` or higher
- At least one Product in the cluster must currently declare `implements` for that capability
- If no implementing Product exists, `resource apply` fails with `CapabilityUnfulfilled`
- Deployment is blocked until the dependency is satisfied

### Resolution

The platform resolves which Product currently implements the required capability. If multiple Products implement the same capability, the platform selects the implementor with the highest version that satisfies the consumer's `minVersion` constraint.

### RDF projection

```turtle
<https://picloud.local/products/maps-app>
    picloud:requiresCapability [
        picloud:capability <https://picloud.local/capabilities/gps-to-place> ;
        picloud:minVersion "1.0.0"
    ] .
```
