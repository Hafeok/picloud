---
id: TC-272
title: Data domain created as cluster-scoped governance boundary
type: scenario
status: passing
validates:
  features:
  - FT-065
  adrs:
  - ADR-056
phase: 3
runner: cargo-test
runner-args: "tc272_data_domain_created_as_cluster_scoped_governance_boundary"
last-run: 2026-04-15T14:19:09.008847312+00:00
last-run-duration: 0.5s
---

## Description

Declare a data domain with steward identity and sensitivity classification, then verify it is projected into the RDF graph as a cluster-scoped `pc:DataDomain` resource.

Checks:
- The data domain IRI is cluster-scoped (under `/data-domains/`, not product-scoped)
- All governance metadata triples are present (name, steward, sensitivity, status)
- Multiple data domains coexist independently with different sensitivity levels
- All data domains are discoverable via SPARQL
- Deleting a data domain removes all its triples without affecting other domains