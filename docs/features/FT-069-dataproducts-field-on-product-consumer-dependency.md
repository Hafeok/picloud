---
id: FT-069
title: dataProducts field on product — consumer dependency validated at resource apply time
phase: 3
status: complete
depends-on: []
adrs:
- ADR-056
tests:
- TC-231
- TC-202
domains: []
domains-acknowledged: {}
---

## Description

Products declare `dataProducts` dependencies to consume published data products from other Products (ADR-056). The dependency is on the data product contract, not on the producing Product directly.

### Resource syntax

```bicep
product 'maps-app' = {
  version: '1.0.0'
  dataProducts: [
    { source: 'photo-app/photo-locations', minVersion: '1.0.0' }
  ]
}
```

### Validation at deploy time

- The referenced data product must exist at the required `minVersion` or higher
- If the data product does not exist, `resource apply` fails with `DataProductNotFound`
- The consuming Product is not deployed — no partial state is created

### Access

Once deployed, the consuming Product's workloads can query the data product's published named graph:
```
https://picloud.local/products/photo-app/data-products/photo-locations/graph
```

Access is IAM-gated — the consuming workload must have the role required by the data product's `access.roles` declaration.

### Deletion guard

A data product cannot be deleted while any Product declares a `dataProducts` dependency on it. Deletion is rejected with an error listing the consuming Products.
