---
id: FT-072
title: DataProductProjector — cluster RDF graph reflects all data products, domains, producers, consumers, freshness
phase: 3
status: complete
depends-on: []
adrs:
- ADR-056
tests:
- TC-276
- TC-333
domains: []
domains-acknowledged: {}
---

## Description

The `DataProductProjector` maintains the cluster RDF graph's representation of all data products, their domains, producers, consumers, and freshness status (ADR-056). This is the cluster-level view of the data mesh — discoverable via SPARQL.

### Projected triples

For each data product:
```turtle
<https://picloud.local/products/photo-app/data-products/photo-locations>
    a picloud:DataProduct ;
    picloud:version "1.0.0" ;
    picloud:domain <https://picloud.local/data-domains/geospatial> ;
    picloud:producedBy <https://picloud.local/products/photo-app> ;
    picloud:consumedBy <https://picloud.local/products/maps-app> ;
    picloud:freshnessStatus "healthy" ;
    picloud:lastRefreshedAt "2025-07-01T12:00:00Z"^^xsd:dateTime ;
    picloud:maxAge "PT15M"^^xsd:duration .
```

For each data domain:
```turtle
<https://picloud.local/data-domains/geospatial>
    a picloud:DataDomain ;
    picloud:steward <https://picloud.local/platform/identities/alice> ;
    picloud:sensitivity "internal" ;
    picloud:hasDataProduct <https://picloud.local/products/photo-app/data-products/photo-locations> .
```

### Events consumed

The projector consumes: `DataProductDeclared`, `DataProductReady`, `DataProductRefreshed`, `DataProductSLOBreached`, `DataProductSLORestored`, `DataProductDeleted`, `DataDomainDeclared`, `DataDomainDeleted`.

### Query surface

Operators can query the full data mesh topology:
```sparql
SELECT ?domain ?product ?dp ?status WHERE {
  ?dp a picloud:DataProduct ;
      picloud:domain ?domain ;
      picloud:producedBy ?product ;
      picloud:freshnessStatus ?status .
}
```
