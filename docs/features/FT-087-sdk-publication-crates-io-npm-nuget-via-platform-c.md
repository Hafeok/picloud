---
id: FT-087
title: SDK publication — crates.io, npm, NuGet via platform CI
phase: 3
status: planned
depends-on: []
adrs:
- ADR-033
tests:
- TC-097
- TC-098
- TC-099
- TC-218
- TC-233
domains: []
domains-acknowledged: {}
---

## Description

SDKs generated from the platform ontology (FT-086) are published to standard package registries (ADR-033). Publication happens both automatically on platform releases and on-demand from any live cluster.

### Target registries

| Language | Package | Registry |
|---|---|---|
| Rust | `picloud-sdk` | crates.io |
| TypeScript | `@picloud/sdk` | npm |
| .NET / C# | `PiCloud.Sdk` | NuGet |

### Publication triggers

**Platform CI** — on every versioned platform release, the generator runs against the release ontology and publishes SDK packages with matching version numbers. SDK versions are always aligned to platform versions.

**`picloud sdk publish`** (FT-088) — any operator can generate and publish SDKs from a live cluster's current ontology. Supports custom registries for air-gapped or internal deployments.

### Version alignment

SDK package versions match the platform version that generated them. Breaking platform changes produce breaking SDK changes. Semantic versioning is enforced.

### Custom registries

For air-gapped deployments, operators configure alternative registry URLs. The generator produces the same packages but publishes to the specified endpoints.
