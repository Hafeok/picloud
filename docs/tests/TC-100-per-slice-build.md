---
id: TC-100
title: per_slice_build
type: scenario
status: unimplemented
validates:
  features: []
  adrs:
  - ADR-034
phase: 1
---

for each slice in the workspace, build it independently: `cargo build -p picloud-{slice}`. Assert each compiles without requiring other slices to be present.