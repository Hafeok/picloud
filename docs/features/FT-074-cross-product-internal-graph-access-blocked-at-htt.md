---
id: FT-074
title: Cross-product internal graph access blocked at HTTP layer — 403 for non-owner non-admin
phase: 3
status: planned
depends-on: []
adrs:
- ADR-056
tests:
- TC-278
- TC-335
- TC-201
domains: []
domains-acknowledged: {}
---

## Description

Cross-product access to another Product's internal RDF graph is blocked at the HTTP layer (ADR-056). Any non-owner, non-admin identity that attempts a SPARQL query against a Product's internal graph receives `403 Forbidden`.

### Enforcement

When a SPARQL request targets `https://picloud.local/products/{product-name}/graph`:
1. Extract the caller's identity from the mTLS certificate or bearer token
2. Check if the caller is a workload identity belonging to the target Product → **allow**
3. Check if the caller has platform-admin role → **allow**
4. Otherwise → **reject with `403 Forbidden`**

### Audit

Every rejected access attempt emits a `UnauthorisedGraphAccess` event to the platform log:
```json
{
  "type": "UnauthorisedGraphAccess",
  "payload": {
    "caller_iri": "https://picloud.local/products/maps-app/identities/worker",
    "target_graph": "https://picloud.local/products/photo-app/graph",
    "rejected_at": "2025-07-01T12:00:00Z"
  }
}
```

### What remains accessible

- **Data product graphs** (`…/data-products/{name}/graph`) — accessible to consumers with appropriate roles
- **Cluster-level graph** — accessible to workloads with platform-level permissions
- **Product's own graph** — accessible to the Product's own workloads

### Why this matters

Without this enforcement, data products (FT-066) would be optional in practice — Products could bypass them by querying internal graphs directly. Hard enforcement at the HTTP layer ensures the operational/analytical boundary is real.
