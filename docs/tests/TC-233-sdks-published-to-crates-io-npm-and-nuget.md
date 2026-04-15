---
id: TC-233
title: SDKs published to crates.io, npm, and NuGet
type: exit-criteria
status: passing
validates:
  features: [FT-086]
  adrs: [ADR-033]
phase: 3
runner: cargo-test
runner-args: "tc233_sdks_published_to_crates_io_npm_and_nuget"
last-run: 2026-04-15T17:43:16.930096060+00:00
last-run-duration: 0.5s
---

## Description

Verify the complete SDK generation and publish pipeline for all three language
targets (Rust/crates.io, TypeScript/npm, .NET/NuGet). The test exercises:

1. `generate_all()` produces valid SDK packages for Rust, TypeScript, and .NET
2. Each generated SDK contains the correct package name for its registry
3. Each SDK exposes the full platform API surface (append_event, query_graph, get_resource)
4. `publish()` dry-run targets the correct registry command for each language
5. Custom registry overrides work for private deployments
6. ClusterDomain (ontology binding) propagates correctly to all generated SDKs
7. `SdkGenerationResult` and `SdkPublishResult` types are properly structured