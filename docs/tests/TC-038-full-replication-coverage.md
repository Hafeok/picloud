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
runner: scripts/run-tc.sh
runner-args: "full-replication-coverage"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

allocate a `full-replication` volume on a three-node cluster. Write known data from node A. Assert the data is readable from node B and node C without contacting node A.