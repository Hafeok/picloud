---
id: FT-086
title: SDK generator — Rust, TypeScript, .NET generated from platform ontology
phase: 3
status: planned
depends-on: []
adrs:
- ADR-033
- ADR-029
tests:
- TC-233
domains: []
domains-acknowledged: {}
---

## Description

The SDK generator produces typed client libraries in Rust, TypeScript, and .NET from the platform's RDF ontology (ADR-033). The ontology is the source of truth — adding a resource type or event type flows through to all SDKs automatically.

### Generation pipeline

```
Platform RDF ontology
  → picloud-sdk-gen
    → Rust crate     (picloud-sdk)      → crates.io
    → TypeScript pkg (@picloud/sdk)     → npm
    → .NET package   (PiCloud.Sdk)      → NuGet
```

### SDK surface per language

Each SDK covers the full platform API surface available to workloads:
- **Event store** — append events, read aggregate streams, subscribe to event streams
- **SPARQL client** — typed query client with content negotiation
- **IAM client** — workload token exchange, incoming token validation
- **Platform events** — subscribe to cluster-level event stream
- **Resource client** — read resource metadata from platform IRI space

### Generator input

The generator reads:
1. The platform's core ontology (built into the binary)
2. Product ontology files (deployed with each Product)
3. Event schema IRIs and their definitions

### Code generation model

- Resource types → typed structs/classes with serialization
- Event types → typed event classes with schema IRI constants
- SPARQL endpoints → typed query builders
- IAM flows → token exchange and validation helpers

### Consistency guarantee

The ontology-as-source-of-truth model means the SDK cannot drift from the API. If a resource type exists in the platform, it exists in the SDK.
