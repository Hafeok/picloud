---
id: FT-010
title: SDK Model
phase: 3
status: complete
depends-on:
- FT-008
adrs:
- ADR-033
- ADR-032
- ADR-019
tests:
- TC-097
- TC-098
- TC-099
- TC-218
domains:
- sdk
- api
domains-acknowledged: {}
---

The platform generates SDKs in three languages from its own RDF ontology. The ontology is the source of truth — adding a resource type or event type to the platform automatically flows through to all SDKs on the next generation pass.

### Language targets

- **Rust** — published to crates.io as `picloud-sdk`
- **TypeScript** — published to npm as `@picloud/sdk`
- **.NET / C#** — published to NuGet as `PiCloud.Sdk`

### SDK surface per language

Each SDK covers the full platform API surface available to workloads:

- **Event store** — append events, read aggregate streams, subscribe to aggregate event streams
- **SPARQL client** — typed query client for the Product's RDF store, with content negotiation handled automatically
- **IAM client** — exchange workload identity certificate for scoped access tokens, validate incoming tokens from other workloads or users
- **Platform events** — subscribe to cluster-level events from within a workload
- **Resource client** — read resource metadata from the platform IRI space

### Generation pipeline

```
Platform RDF ontology
  → picloud sdk generate
    → Rust crate     (picloud-sdk)      → crates.io
    → TypeScript pkg (@picloud/sdk)     → npm
    → .NET package   (PiCloud.Sdk)      → NuGet
```

### Publication triggers

**Platform CI** — on every versioned platform release, the generator runs against the release ontology and publishes SDK packages with matching version numbers. SDK versions are always aligned to platform versions.

**`picloud sdk publish`** — any operator can run this against a live cluster to generate and publish SDKs from the cluster's current live ontology. Useful for custom forks, internal extensions, or air-gapped registries.

### .NET / Aspire integration

The .NET SDK ships an Aspire integration package (`PiCloud.Sdk.Aspire`) that registers PiCloud resources as Aspire hosting components. Developers using .NET Aspire can add PiCloud event stores, SPARQL clients, and IAM clients to their Aspire AppHost and get full local development support with the same resource definitions used in production.

```csharp
var builder = DistributedApplication.CreateBuilder(args);

var photoStore = builder.AddPiCloudEventStore("photos");
var api = builder.AddProject<Projects.PhotoApi>("api")
    .WithReference(photoStore);
```

---