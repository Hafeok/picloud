---
id: TC-062
title: one_active_version_invariant
type: invariant
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-021
phase: 1
---

query `SELECT DISTINCT ?version WHERE { <product-iri> picloud:activeVersion ?version }` after any deployment. Assert the result always contains exactly one row.