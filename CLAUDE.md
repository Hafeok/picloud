# PiCloud — Claude Code Context

> Read this file before touching any code.
> Read the ADRs before making any architectural decision.
> Read the PRD before adding any new capability.

---

## What is PiCloud?

PiCloud is a single Rust binary that turns a cluster of Raspberry Pi 5 nodes into a private cloud. One binary runs on every node. Nodes discover each other via mDNS, form a Raft cluster, and present a unified platform for running workloads with distributed storage, IAM, and event sourcing — with no external dependencies.

The platform is built on three foundational ideas:
1. **The event log is the source of truth.** State is never written directly — it flows through an append-only, Raft-replicated event log.
2. **The RDF graph is the read model.** Oxigraph is a continuously maintained projection of the event log. All state reads are SPARQL queries.
3. **Every resource has a dereferenceable IRI.** `https://picloud.local/products/photo-app/containers/api-server` is both the identifier and the HTTP address of that resource.

---

## Repository Layout

```
picloud/
├── CLAUDE.md                   ← you are here
├── Cargo.toml                  ← workspace root
├── docs/
│   ├── picloud-prd.md          ← full product requirements
│   └── picloud-adrs.md         ← all architectural decisions (58 ADRs)
├── crates/
│   ├── picloud-domain/         ← STABLE FOUNDATION — read this first
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── error.rs        ← all error types
│   │       ├── iri.rs          ← IRI model and builder
│   │       ├── events.rs       ← platform event types and envelope
│   │       ├── resources.rs    ← all resource types
│   │       ├── identity.rs     ← IAM types (passkeys, workload identity)
│   │       ├── storage.rs      ← storage intent types
│   │       ├── workload.rs     ← container and binary specs
│   │       ├── product.rs      ← product and upgrade types
│   │       └── traits.rs       ← abstractions each slice implements
│   │
│   ├── picloud-cluster/        ← mDNS discovery, Raft, node membership
│   ├── picloud-events/         ← event log, product event stores
│   ├── picloud-rdf/            ← Oxigraph projection engine, SPARQL
│   ├── picloud-iam/            ← OIDC provider, WebAuthn, workload certs
│   ├── picloud-storage/        ← block storage pool, volume allocation
│   ├── picloud-workload/       ← OCI containers (youki), binary scheduler
│   ├── picloud-network/        ← DNS, ingress, mTLS, certificate issuance
│   ├── picloud-http/           ← HTTP server, IRI routing, content negotiation
│   ├── picloud-sdk-gen/        ← SDK generator (Rust, TypeScript, .NET)
│   └── picloud-cli/            ← CLI binary (`picloud`)
└── src/
    └── main.rs                 ← server binary composition root (`picloud-server`)
```

---

## The Dependency Rule — Never Break This

```
picloud-domain   ← no dependencies on other picloud crates
all slices       → picloud-domain ONLY
picloud-server   → all slices (composition root only)
picloud-cli      → picloud-domain ONLY (talks to cluster via HTTP)
```

**Slices never import each other.** They communicate at runtime via the event log.
If you find yourself importing `picloud-iam` from `picloud-workload`, stop — find the domain trait in `picloud-domain::traits` and use that instead.

---

## Where to Make Changes

| What you're building | Crate to touch |
|---|---|
| New resource type | `picloud-domain/src/resources.rs` + new event payloads in `events.rs` |
| New platform event | `picloud-domain/src/events.rs` |
| New domain trait | `picloud-domain/src/traits.rs` |
| Raft / node discovery | `picloud-cluster` |
| Event log storage | `picloud-events` |
| RDF projection / SPARQL | `picloud-rdf` |
| Passkeys / OIDC / tokens | `picloud-iam` |
| Block storage / volumes | `picloud-storage` |
| Container / binary scheduling | `picloud-workload` |
| DNS / TLS / ingress | `picloud-network` |
| HTTP endpoints / IRI routing | `picloud-http` |
| SDK generation | `picloud-sdk-gen` |
| CLI commands | `picloud-cli` |
| Wiring slices together | `src/main.rs` ONLY |

---

## Key Architectural Patterns

### Every operation is an event

```
CLI → POST /api/commands (EventEnvelope)
    → Raft leader appends to log
    → RDF projector updates Oxigraph
    → CLI subscribes to SSE stream, filtered by correlation_id
    → Terminal event (ResourceReady / ResourceFailed) ends the operation
```

### Every resource has an IRI

Use `IriBuilder` from `picloud-domain::iri` to construct all IRIs:
```rust
let iri_builder = IriBuilder::new(ClusterDomain::default());
let container_iri = iri_builder.resource("photo-app", "containers", "api-server");
// → https://picloud.local/products/photo-app/containers/api-server
```

Never construct IRI strings manually.

### Every event carries a schema IRI

```rust
EventEnvelope::new(
    iri_builder.event_schema("ResourceReady", 1),
    "ResourceReady",
    source_iri,
    Some("photo-app".to_string()),
    correlation_id,
    serde_json::to_value(&payload)?,
)
```

### Slices implement domain traits

```rust
// In picloud-workload:
use picloud_domain::traits::WorkloadScheduler;

pub struct YoukiScheduler { ... }

#[async_trait]
impl WorkloadScheduler for YoukiScheduler {
    async fn schedule(&self, workload_iri: &ResourceIri, spec: &WorkloadSpec) -> Result<WorkloadHandle> {
        // implementation
    }
}
```

### The composition root wires everything

In `src/main.rs`, all slice implementations are instantiated and injected:
```rust
let scheduler: Arc<dyn WorkloadScheduler> = Arc::new(YoukiScheduler::new(...));
let storage: Arc<dyn StorageBackend> = Arc::new(NvmeStorageBackend::new(...));
// etc.
```

---

## Error Handling

All errors use `picloud_domain::error::PiCloudError` and `picloud_domain::error::Result<T>`.
Add new error variants to `PiCloudError` in `picloud-domain/src/error.rs` — never define crate-local error types.

---

## IAM Rules (Important)

- Human users authenticate via passkeys/FIDO2 only — no passwords anywhere (ADR-025)
- Admin accounts must have ≥ 2 passkeys registered — enforce this in `picloud-iam`
- Workload identity credentials are injected by the platform — workloads never handle certificate generation
- Every HTTP endpoint validates the caller's token before any business logic

---

## Product Isolation Rules (Important)

Products are hermetically sealed (ADR-016, ADR-018, ADR-028):
- Products never share resources
- Products never call each other's HTTP endpoints directly
- Inter-product communication is exclusively via the platform event bus and SPARQL queries
- When implementing any feature that touches multiple products, route through events

---

## Before Making an Architectural Decision

1. Read `docs/picloud-adrs.md` — the decision may already be made
2. If not covered, create a new ADR at the end of that file before writing code
3. ADRs follow the format: Status, Context, Decision, Rationale, Rejected Alternatives, Consequences

---

## Product CLI — The Single Source of Truth for Project Artifacts

All features, ADRs, and test criteria are managed through the **Product CLI** (`product`), available at [github.com/Hafeok/product-cli](https://github.com/Hafeok/product-cli). **Never create or edit feature files, ADR files, or test criterion files by hand — always use the CLI or MCP server.**

### Why

The CLI maintains a knowledge graph (`docs/graph/index.ttl`) that tracks relationships between features, ADRs, and test criteria. Hand-editing files will cause the graph to drift out of sync.

### CLI Usage

```bash
# Features
product feature list                    # list all features
product feature show FT-012             # show feature details
product feature new                     # create a new feature (interactive)
product feature status FT-012 building  # update feature status
product feature link FT-012 --adr ADR-005  # link feature to ADR
product feature adrs FT-012             # list ADRs linked to a feature
product feature tests FT-012            # list test criteria for a feature
product feature next                    # next feature to implement (topo order)

# ADRs
product adr list                        # list all ADRs
product adr show ADR-005                # show ADR details
product adr new                         # create a new ADR
product adr status ADR-005 accepted     # update ADR status

# Test Criteria
product test list                       # list all test criteria
product test show TC-001                # show test criterion details
product test new                        # create a new test criterion
product test untested                   # find features with no tests

# Planning & Analysis
product status                          # project-wide status summary
product gap                             # gap analysis (ADRs ↔ features ↔ tests)
product impact FT-012                   # impact analysis for a feature
product checklist                       # generate implementation checklist
product context FT-012                  # assemble LLM context bundle for a feature
product preflight FT-012                # pre-flight checks before implementing
```

### MCP Server Mode

The Product CLI can run as an MCP server, exposing all graph operations as tools for Claude Code:

```bash
product mcp                             # stdio transport (for Claude Code)
product mcp --http --port 7777          # HTTP transport
product mcp --write                     # enable write operations
```

When the MCP server is available, prefer using MCP tools over shell commands for managing features, ADRs, and test criteria.

### Rules

- **Always use `product feature new` / `product adr new` / `product test new`** to create artifacts — never create markdown files manually in `docs/features/` or `docs/adrs/`.
- **Always use `product feature link`** to establish relationships — never add links by editing frontmatter directly.
- **Always use `product feature status` / `product adr status` / `product test status`** to update statuses.
- **Run `product gap` before starting implementation** to verify coverage.
- **Run `product preflight <FT-ID>` before implementing a feature** to check domain and cross-cutting readiness.

### Implementation Workflow

Use the Product CLI (or MCP tools) to stay in sync with the knowledge graph.

**If using `product implement FT-XXX`** — the pipeline assembles the context bundle and passes it to the spawned agent automatically. Do **not** also run `product context` — that would duplicate the context.

**If implementing manually** (without `product implement`):

1. **Get context** — run `product context FT-XXX --depth 2` to get the full bundle (linked ADRs + test criteria)
2. **Check decisions** — run `product impact ADR-XXX` to understand what a change affects before modifying behavior

**Always, regardless of path:**

1. **Configure TC runners** — before verifying, ensure every TC linked to the feature has `runner: cargo-test` and `runner-args: "tc_XXX_snake_case_name"` in its front-matter (see "TC Runner Configuration" below). Without these fields, `product verify` silently skips the TC.
2. **Verify work** — run `product verify FT-XXX` after implementation to execute TC runners and update test status in front-matter
3. **Mark done** — when all TCs pass, `product verify` auto-updates feature status to `complete` and regenerates `CHECKLIST.md`
4. **Check health** — run `product gap check` and `product drift check` to catch specification issues before committing

**Do not manually edit feature status or `CHECKLIST.md`** — let the CLI manage that through `verify` and `checklist generate`.

### TC Runner Configuration

Every test criterion file (`docs/tests/TC-*.md`) must have runner metadata in its YAML front-matter for `product verify` to execute it:

```yaml
---
id: TC-013
title: event_log_replay
type: scenario
status: failing
runner: cargo-test
runner-args: "tc013_event_log_replay"
validates:
  features: [FT-002]
  adrs: [ADR-004, ADR-035]
phase: 1
---
```

**Runner types:**
- `cargo-test` — runs `cargo test --workspace --test '*' -- tc_name` (most common for unit/integration tests)
- `scripts/run-tc.sh` — runs the E2E test harness via `picloud-test` binary (for cluster-level scenarios)

**Convention:** test function names use the pattern `tc{ID}_{snake_case_title}` (e.g., `tc013_event_log_replay`). The `runner-args` field must match the Rust test function name exactly.

---

## Build and Test

### Local (x86_64)

```bash
# Build everything
cargo build --workspace

# Test everything (438 tests across 14 crates)
cargo test --workspace

# Test a specific slice
cargo test -p picloud-domain
cargo test -p picloud-events

# Check without building
cargo check --workspace

# Release build
cargo build --workspace --release
```

### Pi Cluster (aarch64)

The project builds natively on Raspberry Pi 5 nodes. Use the **build node** (see `/picloud-e2e` skill for IPs and SSH details). Build once there, then copy binaries to the other nodes.

Node IPs, SSH users, and cluster topology are **not checked into the repo** — they live in:
- The `/picloud-e2e` skill (for build/deploy/test workflows)
- The `/picloud-implement` skill (for implementation context)
- `crates/picloud-test/cluster.toml` is a **template** — fill in IPs before use

```bash
# General workflow (substitute <BUILD_NODE> with actual IP from skill):
# 1. Sync source to build node
rsync -az --delete --exclude='target/' --exclude='.git/' ./ admin@<BUILD_NODE>:~/picloud/

# 2. Build + test on build node
ssh admin@<BUILD_NODE> "source ~/.cargo/env && cd ~/picloud && cargo test --workspace"
ssh admin@<BUILD_NODE> "source ~/.cargo/env && cd ~/picloud && cargo build --workspace --release"

# 3. Copy release binaries to worker nodes
scp admin@<BUILD_NODE>:~/picloud/target/release/picloud-server /tmp/
scp admin@<BUILD_NODE>:~/picloud/target/release/picloud /tmp/
scp /tmp/picloud-server /tmp/picloud admin@<WORKER_NODE>:~/
```

---

## Phase Plan Summary

| Phase | Focus | Key deliverables |
|---|---|---|
| 1 (MVP) | Cluster + IAM + Storage + Workloads | Nodes form cluster, container runs, volume mounts |
| 2 | Products + OIDC + Secrets | Full product lifecycle, passkey login works |
| 3 | RDF + Event Store + SDKs | Per-product SPARQL, event sourcing, SDKs published |
| 4 | Operational maturity | Storage tiers, log compaction, node drain |

**Start here:** `picloud-cluster` — get two nodes forming a cluster via mDNS and Raft.
