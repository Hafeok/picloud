---
id: TC-294
title: RDF projection exit — SPARQL query returns projected cluster state
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc294_rdf_projection_exit_sparql_query_returns_projected"
validates:
  features: [FT-016]
  adrs: [ADR-029]
phase: 1
last-run: 2026-04-13T20:59:55.200093379+00:00
---

## Description

Exit criteria for FT-016: Project a representative cluster state (nodes, products, containers, volumes, identities, leader election, metrics) into Oxigraph via the event projection pipeline, then verify the full cluster state is queryable via SPARQL SELECT, ASK, CONSTRUCT, and DESCRIBE queries.

Validates that:
1. Node resources are projected with correct types and metadata
2. Leader election is reflected in the graph
3. Products are projected with version and status
4. Resources (containers, volumes) have correct types and status transitions
5. Identities are projected with name and type
6. Node metrics are queryable via SPARQL
7. Product-scoped resources land in the correct named graphs (graph isolation)
8. Cross-cutting queries return the full cluster state
9. CONSTRUCT and DESCRIBE queries produce valid results