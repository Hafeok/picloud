---
id: TC-081
title: slice_dependency_enforcement
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-028
phase: 1
runner: cargo-test
runner-args: "tc081_slice_dependency_enforcement"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

for each slice crate, run `cargo build -p {crate}` with all other slices removed from the workspace. Assert each slice compiles independently with only `picloud-domain` as an internal dependency.