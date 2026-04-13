---
id: ADR-033
title: Generated Multi-Language SDKs Published to Package Registries
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Workloads interact with the platform via HTTP APIs (event store, SPARQL, IAM, platform events). Raw HTTP calls are verbose and error-prone. Developers need idiomatic clients in their language of choice. Handwriting SDKs for three languages and keeping them in sync with platform evolution is not sustainable.

**Decision:** SDKs are generated from the platform's RDF ontology. The ontology is the source of truth — adding a resource type or event type flows through to all SDKs automatically. Three language targets are supported: Rust, TypeScript, and .NET (C#). SDKs are published to package registries on every platform release and on-demand via `picloud sdk publish`.

**Language targets and registries:**
- **Rust** → crates.io as `picloud-sdk`
- **TypeScript** → npm as `@picloud/sdk`
- **.NET / C#** → NuGet as `PiCloud.Sdk`

**SDK surface:**
- Event store — append events, read aggregate streams, subscribe to aggregate event streams
- SPARQL client — typed query client with content negotiation
- IAM client — workload token exchange, incoming token validation
- Platform events — subscribe to cluster-level event stream
- Resource client — read resource metadata from platform IRI space

**Generation pipeline:**
```
Platform RDF ontology
  → picloud sdk generate
    → Rust crate        → crates.io
    → TypeScript pkg    → npm
    → .NET package      → NuGet
```

**Publication triggers:**
- **Platform CI** — generator runs on every versioned release. SDK package versions are aligned to platform versions.
- **`picloud sdk publish`** — operator runs against any live cluster to generate from the cluster's live ontology and publish to configured registries. Supports custom registries for air-gapped or internal deployments.

**.NET Aspire integration:**
The .NET SDK ships a companion `PiCloud.Sdk.Aspire` package. PiCloud resources are registered as Aspire hosting components, enabling local development with the same resource definitions used in production.

**Rationale:**
- The ontology-as-source-of-truth model means SDK accuracy is guaranteed — the SDK cannot drift from the API
- Three language targets cover the primary developer audiences: systems developers (Rust), web/Node developers (TypeScript), and enterprise developers (C#/.NET)
- On-demand generation via `picloud sdk publish` means custom forks and internal extensions get SDK support without waiting for upstream releases
- .NET Aspire integration reflects the primary developer's workflow and makes PiCloud a first-class Aspire resource

**Consequences:**
- The SDK generator is a significant piece of platform tooling — it must handle three target languages from one ontology source
- SDK versioning is coupled to platform versioning — breaking platform changes are breaking SDK changes
- The generator must be part of the platform's own CI from day one — not an afterthought