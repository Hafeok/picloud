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
last-run: 2026-04-18T14:42:27.228653472+00:00
last-run-duration: 0.0s
---

deploy a product with `rdf-store`. Run a SPARQL SELECT against the product's SPARQL endpoint with a valid workload token. Assert 200 and correct results.