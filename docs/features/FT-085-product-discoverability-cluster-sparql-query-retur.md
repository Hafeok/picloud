---
id: FT-085
title: Product discoverability — cluster SPARQL query returns all Products, events, ontologies, capabilities, data products
phase: 3
status: complete
depends-on: []
adrs:
- ADR-020
- ADR-029
tests:
- TC-284
- TC-341
domains: []
domains-acknowledged: {}
---

## Description

The cluster root IRI (`https://picloud.local/`) returns a comprehensive RDF document describing all Products in the cluster, their event interfaces, ontologies, capabilities, and data products. This is the semantic service registry — fully navigable by following IRI links.

### Discovery endpoint

```
GET https://picloud.local/
Accept: text/turtle
```

Returns triples describing:
- All Products with their versions and status
- All event types each Product emits and subscribes to
- All ontology IRIs and their versions
- All capabilities, their implementors and consumers
- All data domains and their data products
- All SPARQL endpoint IRIs

### SPARQL discovery

The same information is queryable via the cluster SPARQL endpoint:
```sparql
SELECT ?product ?version ?ontology ?capability WHERE {
  ?product a picloud:Product ;
           picloud:version ?version .
  OPTIONAL { ?product picloud:ontology ?ontology . }
  OPTIONAL { ?product picloud:implements ?capability . }
}
```

### Use cases

- **LLM agents** — dereference the cluster root to understand all available services
- **SDK generators** — discover all event schemas and ontologies for code generation
- **RDF tools** — navigate the entire cluster by following linked data IRIs
- **Operators** — understand the full Product topology without reading individual resource files

### Linked Data navigation

Every IRI in the discovery response is dereferenceable. Following a Product IRI returns that Product's detail. Following an ontology IRI returns the schema. The cluster is a Linked Data platform by construction.
