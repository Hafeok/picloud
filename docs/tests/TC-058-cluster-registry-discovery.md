---
id: TC-058
title: cluster_registry_discovery
type: scenario
status: failing
validates:
  features:
  - FT-005
  adrs:
  - ADR-020
phase: 1
runner: picloud-test
runner-args: "cluster-registry-discovery"
---

deploy three products with different event types, SPARQL endpoints, and ontologies. Query the cluster-level SPARQL endpoint for all products, their event schemas, and their ontology IRIs. Assert all three products discoverable in a single query.