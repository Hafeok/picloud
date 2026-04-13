---
id: TC-038
title: full_replication_coverage
type: scenario
status: failing
validates:
  features:
  - FT-004
  adrs:
  - ADR-013
phase: 1
runner: picloud-test
runner-args: "full-replication-coverage"
---

allocate a `full-replication` volume on a three-node cluster. Write known data from node A. Assert the data is readable from node B and node C without contacting node A.