---
id: FT-065
title: data-domain resource type — cluster-scoped governance boundary
phase: 3
status: complete
depends-on: []
adrs:
- ADR-056
tests:
- TC-272
- TC-329
- TC-196
- TC-205
- TC-208
domains: []
domains-acknowledged: {}
---

## Description

A `data-domain` is a cluster-scoped governance namespace that groups related data products across Products (ADR-056). Data domains provide organizational boundaries for analytical data — they declare a steward, sensitivity classification, and domain-level constraints.

### Resource syntax

```bicep
data-domain 'geospatial' = {
  description: 'All location and mapping data products across the cluster'
  steward: 'identity/alice'
  sensitivity: 'internal'
}
```

### Properties

- **Steward** — the identity responsible for governing data products within this domain
- **Sensitivity** — classification level (`public`, `internal`, `confidential`, `restricted`)
- **Description** — human-readable description of the domain's purpose

### RDF projection

```turtle
<https://picloud.local/data-domains/geospatial>
    a picloud:DataDomain ;
    picloud:description "All location and mapping data products across the cluster" ;
    picloud:steward <https://picloud.local/platform/identities/alice> ;
    picloud:sensitivity "internal" .
```

### Constraints

- A `data-domain` must exist before any `data-product` can be assigned to it
- A `data-domain` cannot be deleted while any `data-product` is assigned to it — deletion is rejected with a member count error
- Data domain names are cluster-unique

### Events

- `DataDomainDeclared` — domain resource received and validated (FT-071)
- `DataDomainDeleted` — domain removed (only when no member data products exist)
