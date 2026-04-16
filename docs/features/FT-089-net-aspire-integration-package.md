---
id: FT-089
title: .NET Aspire integration package
phase: 3
status: complete
depends-on: []
adrs:
- ADR-033
tests:
- TC-285
- TC-342
domains: []
domains-acknowledged: {}
---

## Description

The .NET SDK ships an Aspire integration package (`PiCloud.Sdk.Aspire`) that registers PiCloud resources as Aspire hosting components (ADR-033). Developers using .NET Aspire can add PiCloud event stores, SPARQL clients, and IAM clients to their Aspire AppHost.

### Aspire AppHost integration

```csharp
var builder = DistributedApplication.CreateBuilder(args);

var photoStore = builder.AddPiCloudEventStore("photos");
var graphClient = builder.AddPiCloudSparqlClient("photo-app");
var iamClient = builder.AddPiCloudIamClient("photo-app");

var api = builder.AddProject<Projects.PhotoApi>("api")
    .WithReference(photoStore)
    .WithReference(graphClient)
    .WithReference(iamClient);
```

### What the integration provides

- **`AddPiCloudEventStore`** — registers an event store resource; configures connection to the Product's event store endpoint; provides `IPiCloudEventStore` for dependency injection
- **`AddPiCloudSparqlClient`** — registers a SPARQL client; configures connection to the Product's SPARQL endpoint with content negotiation
- **`AddPiCloudIamClient`** — registers an IAM client; configures workload token exchange and incoming token validation

### Local development

In local development mode, the Aspire integration configures the clients to point to the development cluster endpoint. In production, it uses the injected `PICLOUD_CLUSTER_ENDPOINT` environment variable.

### Health checks

Each registered resource contributes an Aspire health check. The Aspire dashboard shows PiCloud resource health alongside standard .NET resources.

### Why Aspire

Aspire is the primary development experience for .NET developers building distributed applications. First-class PiCloud support in Aspire makes PiCloud a natural choice for .NET workloads.
