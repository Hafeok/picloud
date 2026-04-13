---
id: TC-185
title: adr_test_coverage_completeness
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-054
phase: 1
---

parse all ADRs in the repository. Assert every ADR that has a status of `Accepted` contains a `Test coverage` section with at least one scenario test and at least one exit criterion.