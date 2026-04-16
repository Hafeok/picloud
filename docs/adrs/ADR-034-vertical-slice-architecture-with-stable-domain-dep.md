---
id: ADR-034
title: Vertical Slice Architecture with Stable Domain Dependency
status: accepted
features:
- FT-001
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:53717852abb147eb6295eebca609cd5420651b299c3fc751fa8e31691c6bbb39
---

**Status:** Accepted

**Context:** A platform of this scope will be built primarily by LLMs working on one capability at a time. If slices are tightly coupled — importing each other freely — a change in one slice can break another, and an LLM working on storage has to understand IAM internals to make progress. The codebase needs a structure that keeps slices independently buildable, testable, and deployable.

**Decision:** PiCloud uses vertical slice architecture. Each slice owns one platform capability end-to-end. The dependency rule is strictly enforced:

```
picloud-domain   ← depends on nothing internal
all slices       → picloud-domain only
picloud-server   → all slices (composition root only)
picloud-cli      → picloud-domain only
```

Slices never import each other. Runtime communication between slices happens exclusively via the event log and through domain traits injected at the composition root.

**Slices and their responsibilities:**

| Crate | Responsibility |
|---|---|
| `picloud-domain` | Shared types, traits, IRI model, error types — no implementations |
| `picloud-cluster` | mDNS node discovery, Raft consensus, cluster membership |
| `picloud-events` | Event log storage, product event stores, schema IRI serving |
| `picloud-rdf` | Oxigraph integration, event projection, SPARQL query execution |
| `picloud-iam` | OIDC provider, WebAuthn/passkeys, workload certificate issuance |
| `picloud-storage` | Block storage pool, NVMe management, volume allocation and replication |
| `picloud-workload` | OCI container runtime (youki), binary execution, workload scheduling |
| `picloud-network` | Internal DNS, mTLS, TLS certificate management, ingress routing |
| `picloud-http` | HTTP server, IRI routing, content negotiation, SSE event streams |
| `picloud-sdk-gen` | SDK generation from platform ontology (Rust, TypeScript, .NET) |
| `picloud-cli` | CLI binary — emits commands as events, subscribes to result stream |

**Why picloud-domain is the right name over picloud-core:**
`core` implies infrastructure utilities. `domain` signals that this crate contains the domain model of the platform — the types and abstractions that represent what PiCloud *is*, not what it *uses*. This distinction matters when an LLM reads the crate — it immediately understands this is where to look for the platform's nouns and verbs.

**Rationale:**
- An LLM working on `picloud-storage` only needs to understand `picloud-domain` and `picloud-storage` — the rest of the platform is irrelevant to its task
- Slices can be built and tested in isolation — `cargo test -p picloud-events` works without the rest of the platform compiling
- Adding a new platform capability means adding a new slice — no existing slice is modified
- The composition root (`src/main.rs`) is the only place that knows about all slices — if it gets complex, that signals a design problem
- Consistent with the platform's own low coupling principle (ADR-028) applied to the codebase itself

**Enforcing the rule:**
The dependency rule is enforced by `Cargo.toml` — slices literally cannot import each other because they are not listed as dependencies. Any attempt to add a cross-slice dependency should be treated as an architectural violation and resolved by either moving the shared concept to `picloud-domain` or routing through the event log.

**Rejected alternatives:**
- **Layered architecture** — slices would share horizontal layers (data access, business logic), creating coupling where a change in one capability breaks another.
- **Monolithic single crate** — an LLM working on storage would need to understand the entire codebase, and a change anywhere could break anything.

**Consequences:**
- New shared types must go in `picloud-domain` — this is the right place for them
- Slices communicate via injected trait implementations, not direct calls
- The composition root in `src/main.rs` grows as slices are added — this is expected and correct
- LLMs can be given a single slice plus `picloud-domain` as context and make meaningful progress without understanding the full platform