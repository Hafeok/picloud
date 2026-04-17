---
id: TC-321
title: RDF store exit — per-product Oxigraph created and serves SPARQL
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc321_rdf_store_exit_per_product_oxigraph_created_and_serves_sparql"
validates:
  features: [FT-051]
  adrs: [ADR-006, ADR-019]
phase: 3
last-run: 2026-04-17T06:58:07.074746972+00:00
last-run-duration: 1.3s
---

## Description

Exit criteria: end-to-end verification that a product can declare an
`rdf-store` resource, the platform projects it into the RDF graph with the
correct type and SPARQL endpoint IRI, a per-product Oxigraph instance is
created, and SPARQL query + update operations succeed against the isolated store.

Verifies:
- RdfStore resource type projection (`rdf:type picloud:RdfStore`)
- `picloud:sparqlEndpoint` triple with correct IRI
- `picloud:backingVolume` triple with correct IRI
- Resource status transitions to Ready
- RdfStore appears in product's named graph
- Per-product Oxigraph instance creation and isolation
- Full SPARQL 1.1 support: SELECT, ASK, CONSTRUCT, INSERT DATA, DELETE DATA
- Product isolation — separate stores see no cross-product data
- Store lifecycle (create, query, update, drop)