---
id: FT-052
title: IAM-gated SPARQL endpoint per Product
phase: 3
status: planned
depends-on: []
adrs:
- ADR-019
- ADR-027
tests:
- TC-265
- TC-322
domains: []
domains-acknowledged: {}
---

## Description

Every Product with an `rdf-store` resource gets an IAM-gated SPARQL 1.1 endpoint automatically. The endpoint is accessible at the Product's graph IRI and enforces access control on every query.

### Endpoint

```
https://picloud.local/products/{product-name}/graph
```

Supports:
- SPARQL 1.1 Query (SELECT, CONSTRUCT, ASK, DESCRIBE)
- Content negotiation: `text/turtle`, `application/ld+json`, `application/sparql-results+json`

### Access control

- **Product workloads** — authenticated via mTLS workload certificate + identity token. Must have a role with `{product}/rdf-store/graph:query` permission.
- **Platform admins** — full access to all Product graphs (admin exemption)
- **Other Products' workloads** — rejected with `403 Forbidden`. Cross-product internal graph access is blocked at the HTTP layer (ADR-056). Other Products access shared data only through `data-product` named graphs.

### IAM enforcement

Every SPARQL request is validated before execution:
1. Extract the caller's identity from the mTLS certificate or bearer token
2. Resolve the caller's roles (including group-inherited roles)
3. Check for the required permission on the target graph
4. Execute the query only if authorized; return `403` otherwise

A `UnauthorisedGraphAccess` event is emitted to the platform log on every rejected access attempt.
