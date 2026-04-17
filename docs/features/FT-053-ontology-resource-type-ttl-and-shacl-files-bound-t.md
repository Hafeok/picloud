---
id: FT-053
title: Ontology resource type — .ttl and .shacl files bound to Product version
phase: 3
status: planned
depends-on: []
adrs:
- ADR-023
- ADR-019
tests:
- TC-266
- TC-323
domains: []
domains-acknowledged: {}
---

## Description

Products declare their RDF schema as `.ttl` (Turtle) and `.shacl` (SHACL shapes) files. These ontology files are bound to the Product version — a schema cannot change without a Product version bump. The platform serves ontology files at stable, dereferenceable IRIs.

### Resource syntax

```bicep
ontology 'photo-ontology' = {
  product: 'photo-app'
  files: [
    './ontology/photo-app.ttl'
    './ontology/photo-app.shacl'
  ]
}
```

### Platform behaviour

- Ontology files are deployed with the Product and stored alongside the Product's resource definitions
- The platform serves the ontology at `https://picloud.local/products/{product-name}/ontology` with content negotiation (`text/turtle`, `application/ld+json`)
- Ontology IRIs are permanent — all past versions remain served even after Product upgrades
- SHACL shapes are loaded into Oxigraph for validation and inference (ADR-039)

### Version binding

Ontology files are immutable within a Product version. Changing a `.ttl` or `.shacl` file requires bumping the Product version. This ensures:
- Event store schemas (ADR-032) reference a stable ontology
- SPARQL clients can rely on the schema contract for the current version
- Historical ontology versions remain dereferenceable for interpreting past events

### Discovery

The cluster-level RDF graph includes triples linking each Product to its ontology IRI:
```turtle
<https://picloud.local/products/photo-app>
    picloud:ontology <https://picloud.local/products/photo-app/ontology> ;
    picloud:ontologyVersion "1.0.0" .
```
