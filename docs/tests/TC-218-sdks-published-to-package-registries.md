---
id: TC-218
title: SDKs published to package registries
type: exit-criteria
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
runner-args: run --scenario sdks-published-to-package-registries
last-run: 2026-04-17T10:21:08.824446971+00:00
last-run-duration: 0.0s
---

Verify that the SDK generation and publish pipeline supports all three language targets (Rust/crates.io, TypeScript/npm, .NET/NuGet) and that each generated SDK includes the complete platform API surface.