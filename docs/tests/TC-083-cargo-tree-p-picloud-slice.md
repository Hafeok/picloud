---
id: TC-083
title: cargo tree -p picloud-{slice}
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-028
phase: 1
runner: picloud-test
runner-args: "no_cross_slice_imports"
---