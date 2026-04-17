---
id: TC-342
title: Aspire exit — .NET integration package connects to cluster
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc342_aspire_exit_net_integration_package_connects_to_cluster"
validates:
  features: [FT-089]
  adrs: [ADR-033]
phase: 3
last-run: 2026-04-17T10:22:11.749302124+00:00
last-run-duration: 0.7s
---

## Description

Exit-criteria test: comprehensive verification that the .NET Aspire integration
package is production-ready. Validates generation, publish pipeline, resource types,
Aspire hosting patterns, and dependency structure.

Checks:
- All five files exist on disk and are listed in generation result
- All resource classes inherit from Aspire Resource base class with IResourceWithConnectionString
- Extension methods use correct Aspire builder patterns (IDistributedApplicationBuilder, IResourceBuilder)
- Connection strings target correct API endpoints (/api/events, /api/sparql, /api/iam)
- NuGet package metadata is correct (PackageId, namespace, Aspire dependency)
- Publish pipeline targets NuGet with custom registry support
- Aspire generation does not break the base .NET SDK (separate output directories)
- generate_all still produces exactly 3 base SDKs (Aspire is a companion package)