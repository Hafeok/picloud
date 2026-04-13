---
id: TC-034
title: phase_dependency_order
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-011
phase: 1
---

assert that the block storage scenario suite (`volume_mount.rs`, `replication_coverage.rs`) passes before the RDF store scenario suite (`product_sparql.rs`) is executed. The test runner enforces this ordering and fails the RDF store suite immediately if any block storage test has failed in the same run.