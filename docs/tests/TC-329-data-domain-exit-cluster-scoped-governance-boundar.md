---
id: TC-329
title: Data domain exit — cluster-scoped governance boundary created
type: exit-criteria
status: passing
validates:
  features:
  - FT-065
  adrs:
  - ADR-056
phase: 3
runner: cargo-test
runner-args: tc329_data_domain_exit_cluster_scoped_governance_boundary_created
last-run: 2026-04-17T09:03:38.439227156+00:00
last-run-duration: 0.7s
---

## Description

Exit criteria for FT-065: verify the complete governance boundary contract for data domains.

Checks:
- All four sensitivity levels (public, internal, confidential, restricted) are valid and correctly stored
- Every data domain IRI follows the cluster-scoped pattern `https://picloud.local/data-domains/{name}`
- No data domain IRI is product-scoped
- Each domain carries complete governance metadata (steward, sensitivity, status, name)
- All domains are discoverable via a single SPARQL cross-cutting query
- All newly created domains have "declared" status