---
id: TC-284
title: Cluster SPARQL query returns all products, events, ontologies
type: scenario
status: passing
runner: cargo-test
runner-args: "tc284_cluster_sparql_query_returns_all_products_events_ontologies"
validates:
  features: [FT-085]
  adrs: []
phase: 3
last-run: 2026-04-17T10:18:34.399178528+00:00
last-run-duration: 0.6s
---

## Description

Scenario test for product discoverability (FT-085). Deploys a representative
cluster state with three products, each owning different resource types
(ontologies, event stores, containers), plus cluster-scoped capabilities,
data domains, and data products. Then issues cluster-wide (non-product-scoped)
SPARQL queries to verify that ALL resource types are discoverable:

1. All 3 products with name and version
2. All ontologies across products
3. All event stores across products
4. Product events projected into the RDF graph
5. All capabilities with implementor links
6. All data products across domains
7. A single UNION query discovers all resource types at once
8. Product-scoped resources also appear in their named graphs