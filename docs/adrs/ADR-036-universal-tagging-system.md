---
id: ADR-036
title: Universal Tagging System
status: accepted
features: [FT-009, FT-055]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Resources across the platform — nodes, products, users, groups, containers, volumes — need a lightweight, flexible labelling mechanism. Tags are the primary input to SPARQL CONSTRUCT inference rules (ADR-038), particularly for IAM group membership rules (ADR-037). Tags must be queryable in the RDF graph and travel through the event log.

**Decision:** Every platform resource supports an arbitrary set of tags. A tag is a `key:value` string pair. Tags are declared in resource definition files and can be added or removed via CLI and API. Tag changes are events — `TagAdded` and `TagRemoved` — projected into the RDF graph immediately.

**Tag syntax:**
```bicep
container 'api-server' = {
  product: 'photo-app'
  image: 'photo-api:1.0.0'
  tags: {
    'team': 'backend'
    'environment': 'production'
    'tier': 'api'
  }
}
```

**RDF representation:**
```turtle
<https://picloud.local/products/photo-app/containers/api-server>
    picloud:tag [
        picloud:tagKey "team" ;
        picloud:tagValue "backend"
    ] ;
    picloud:tag [
        picloud:tagKey "environment" ;
        picloud:tagValue "production"
    ] .
```

**CLI:**
```bash
picloud tag add photo-app/containers/api-server team=backend
picloud tag remove photo-app/containers/api-server team=backend
picloud tag list photo-app/containers/api-server
picloud tag find environment=production          # all resources with this tag
```

**Rationale:**
- Tags are a universal primitive — one mechanism for labelling any resource type
- RDF representation makes tags immediately queryable via SPARQL across all resource types
- Event-driven — `TagAdded`/`TagRemoved` trigger inference rule evaluation instantly (ADR-037, ADR-038)
- Key:value pairs are the simplest model that supports meaningful inference patterns

**Rejected alternatives:**
- **Labels as metadata only (not events)** — tag changes would not trigger inference rule evaluation, breaking the SPARQL CONSTRUCT membership model (ADR-037).
- **Hierarchical taxonomy** — rigid hierarchies are harder to evolve and do not support the flexible, cross-cutting labelling patterns that inference rules require.

**Consequences:**
- `Tag` becomes a domain type in `picloud-domain` used by all resource types
- Tag events must be emitted whenever tags change, including on initial resource creation
- Tag keys should be namespaced by convention (`team:`, `environment:`, `tier:`) to avoid collisions — enforced by documentation, not by the platform