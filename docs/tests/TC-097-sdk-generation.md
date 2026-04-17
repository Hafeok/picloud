---
id: TC-097
title: sdk_generation
type: scenario
status: passing
validates:
  features:
  - FT-010
  - FT-087
  - FT-088
  adrs:
  - ADR-033
phase: 1
runner: picloud-test
runner-args: run --scenario sdk-generation
last-run: 2026-04-17T10:21:08.824446971+00:00
last-run-duration: 0.0s
---

run `picloud sdk generate` against a live cluster. Assert the generated Rust crate compiles (`cargo build`), the TypeScript package compiles (`tsc`), and the .NET package builds (`dotnet build`).