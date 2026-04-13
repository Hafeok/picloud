---
id: TC-055
title: product_sparql_endpoint
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-019
phase: 1
runner: scripts/run-tc.sh
runner-args: "product-sparql-endpoint"
last-run: 2026-04-13T21:37:33.242635225+00:00
---

deploy a product with `rdf-store`. Run a SPARQL SELECT against the product's SPARQL endpoint with a valid workload token. Assert 200 and correct results.