---
id: TC-058
title: cluster_registry_discovery
type: scenario
status: passing
validates:
  features:
  - FT-005
  adrs:
  - ADR-020
phase: 1
---

deploy three products with different event types, SPARQL endpoints, and ontologies. Query the cluster-level SPARQL endpoint for all products, their event schemas, and their ontology IRIs. Assert all three products discoverable in a single query.