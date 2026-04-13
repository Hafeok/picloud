---
id: TC-055
title: product_sparql_endpoint
type: scenario
status: unimplemented
validates:
  features:
  - FT-008
  adrs:
  - ADR-019
phase: 1
---

deploy a product with `rdf-store`. Run a SPARQL SELECT against the product's SPARQL endpoint with a valid workload token. Assert 200 and correct results.