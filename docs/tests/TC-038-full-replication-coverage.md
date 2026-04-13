---
id: TC-038
title: full_replication_coverage
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-013
phase: 1
---

allocate a `full-replication` volume on a three-node cluster. Write known data from node A. Assert the data is readable from node B and node C without contacting node A.