---
id: TC-341
title: Discoverability exit — SPARQL returns all products and ontologies
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc341_discoverability_exit_sparql_returns_all_products_and_ontologies"
validates:
  features: [FT-085]
  adrs: []
phase: 3
last-run: 2026-04-17T10:18:34.399178528+00:00
last-run-duration: 0.6s
---

## Description

Exit criteria for product discoverability (FT-085). Verifies the minimum
discoverability guarantee: a cluster-wide SPARQL query returns all products
and their associated ontologies, capabilities, event stores, and data products
without requiring knowledge of which products exist.

1. Deploy 3 products with distinct versions
2. Declare ontologies for each product (Turtle and SHACL formats)
3. Declare a capability, an event store, and a data product
4. Verify a single SPARQL query discovers all products with versions
5. Verify a single SPARQL query discovers all ontologies with product links
6. Verify cluster-wide resource count by kind matches expectations
7. Deploy a 4th product after initial setup — verify it's immediately discoverable
8. Verify the new product's ontology appears in both default and named graphs