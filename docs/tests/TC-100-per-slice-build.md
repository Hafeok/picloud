---
id: TC-100
title: per_slice_build
type: scenario
status: passing
validates:
  features: []
  adrs:
  - ADR-034
phase: 1
runner: picloud-test
runner-args: run --scenario per-slice-build
last-run: 2026-04-13T20:35:13.782523150+00:00
---

for each slice in the workspace, build it independently: `cargo build -p picloud-{slice}`. Assert each compiles without requiring other slices to be present.