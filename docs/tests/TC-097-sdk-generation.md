---
id: TC-097
title: sdk_generation
type: scenario
status: failing
validates:
  features:
  - FT-010
  adrs:
  - ADR-033
phase: 1
runner: picloud-test
runner-args: "sdk-generation"
---

run `picloud sdk generate` against a live cluster. Assert the generated Rust crate compiles (`cargo build`), the TypeScript package compiles (`tsc`), and the .NET package builds (`dotnet build`).