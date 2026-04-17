---
id: TC-333
title: Data projector exit — RDF graph reflects data products and domains
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc333_data_projector_exit_rdf_graph_reflects_data_products_and_domains"
validates:
  features: [FT-072]
  adrs: [ADR-056]
phase: 3
last-run: 2026-04-17T09:59:26.939256319+00:00
last-run-duration: 0.6s
---

## Description

Exit-criteria test verifying the complete invariant: the RDF graph correctly reflects
the full lifecycle of data products and domains together. This is the gate criterion —
if this passes, FT-072 is considered complete.

Exercises:
- Three products (billing-svc, ci-platform, monitoring-hub), each with data products
  across three overlapping domains (finance, engineering, operations)
- Domain creation, data product creation, freshness refresh
- Producer links (`pc:producedBy`) verified for all products
- Domain membership links (`pc:belongsToDomain`) verified across all domains
- Freshness SLO (`pc:maxAge`) discoverability and refresh metadata projection
- Domain reassignment via `DataProductUpdated` with graph consistency verification
- Selective deletion of data products — surviving products and domain links remain intact
- Domain deletion — data products survive with their declared domain affiliation
- Final cleanup: all data products and domains deleted, graph is empty