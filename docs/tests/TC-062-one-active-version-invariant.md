---
id: TC-062
title: one_active_version_invariant
type: invariant
status: failing
validates:
  features:
  - FT-008
  adrs:
  - ADR-021
phase: 1
runner: picloud-test
runner-args: "one-active-version-invariant"
---

query `SELECT DISTINCT ?version WHERE { <product-iri> picloud:activeVersion ?version }` after any deployment. Assert the result always contains exactly one row.