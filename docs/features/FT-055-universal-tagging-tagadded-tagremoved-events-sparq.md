---
id: FT-055
title: Universal tagging — TagAdded/TagRemoved events, SPARQL-queryable on all resources
phase: 3
status: planned
depends-on: []
adrs:
- ADR-036
- ADR-004
tests:
- TC-228
domains: []
domains-acknowledged: {}
---

## Description

Every platform resource supports an arbitrary set of `key:value` tags (ADR-036). Tags are the primary input to SPARQL CONSTRUCT inference rules — particularly for IAM group membership (FT-058).

### Resource syntax

```bicep
container 'api-server' = {
  product: 'photo-app'
  image: 'photo-api:1.0.0'
  tags: {
    'team': 'backend'
    'environment': 'production'
  }
}
```

### Events

- `TagAdded` — emitted when a tag is added to any resource. Payload includes resource IRI, tag key, and tag value.
- `TagRemoved` — emitted when a tag is removed. Payload includes resource IRI, tag key, and tag value.
- Tag events on initial resource creation are emitted for each declared tag.

### RDF projection

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

### SPARQL queryability

Tags are immediately queryable across all resource types:
```sparql
SELECT ?resource WHERE {
  ?resource picloud:tag [
    picloud:tagKey "environment" ;
    picloud:tagValue "production"
  ] .
}
```

### CLI

```bash
picloud tag add photo-app/containers/api-server team=backend
picloud tag remove photo-app/containers/api-server team
picloud tag list photo-app/containers/api-server
picloud tag find environment=production
```

### Inference trigger

`TagAdded` and `TagRemoved` events trigger evaluation of all SPARQL CONSTRUCT inference rules that declare these event types as triggers. This is the mechanism by which group membership (FT-058) cascades from tag changes.
