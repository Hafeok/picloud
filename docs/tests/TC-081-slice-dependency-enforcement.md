---
id: TC-081
title: slice_dependency_enforcement
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-028
phase: 1
runner: picloud-test
runner-args: "slice-dependency-enforcement"
---

for each slice crate, run `cargo build -p {crate}` with all other slices removed from the workspace. Assert each slice compiles independently with only `picloud-domain` as an internal dependency.