# PiCloud

A single Rust binary that turns a cluster of Raspberry Pi 5 nodes into a private cloud.

One binary runs on every node. Nodes discover each other via mDNS, form a Raft cluster, and present a unified platform for running workloads with distributed storage, IAM, and event sourcing — with no external dependencies.

## Key Ideas

- **Event-sourced state** — All cluster state flows through an append-only, Raft-replicated event log. The RDF knowledge graph (Oxigraph) is a continuously maintained projection. Nothing is written directly.
- **Every resource has a dereferenceable IRI** — `https://picloud.local/products/photo-app/containers/api-server` is both the identifier and the HTTP address. The cluster is a Linked Data platform by construction.
- **Products as deployment units** — Every workload is deployed as a Product: a versioned, hermetically sealed boundary with its own IAM scope, storage, networking, event bus, and SPARQL graph.
- **Platform as identity provider** — Full OIDC provider with passkey/FIDO2 authentication. No passwords. Products act as App Registrations.
- **Zero configuration** — Add a node, capacity grows. mDNS handles discovery. No static IPs, no manual join steps.

## Architecture

```
CLI (picloud)
  → emits command events, subscribes to result stream
Resource API
  → typed resources, dependency resolution, IAM
Product Runtime
  → workload scheduling, networking, storage allocation
Platform Services
  → IAM (OIDC provider), DNS, service discovery
Event Log + RDF Projection
  → append-only event store (Raft-replicated)
  → Oxigraph projection — the cluster's observable state
Cluster Consensus (openraft)
  → leader election, log replication, node membership
Node Discovery (mDNS)
  → automatic peer discovery on local network
```

## Repository Structure

```
picloud/
├── Cargo.toml                  # workspace root
├── docs/
│   ├── picloud-prd.md          # product requirements
│   └── picloud-adrs.md         # architecture decision records (34 ADRs)
├── crates/
│   ├── picloud-domain/         # shared types, traits, IRI model, errors
│   ├── picloud-cluster/        # mDNS discovery, Raft, node membership
│   ├── picloud-events/         # event log, product event stores
│   ├── picloud-rdf/            # Oxigraph projection engine, SPARQL
│   ├── picloud-iam/            # OIDC provider, WebAuthn, workload certs
│   ├── picloud-storage/        # block storage pool, volume allocation
│   ├── picloud-workload/       # OCI containers (youki), binary scheduler
│   ├── picloud-network/        # DNS, ingress, mTLS, certificate issuance
│   ├── picloud-http/           # HTTP server, IRI routing, content negotiation
│   ├── picloud-sdk-gen/        # SDK generator (Rust, TypeScript, .NET)
│   └── picloud-cli/            # CLI binary
└── src/
    └── main.rs                 # server binary composition root
```

### Dependency Rule

```
picloud-domain   ← no dependencies on other picloud crates
all slices       → picloud-domain ONLY
picloud-server   → all slices (composition root)
picloud-cli      → picloud-domain ONLY (talks to cluster via HTTP)
```

Slices never import each other. They communicate at runtime via the event log.

## Target Hardware

- Raspberry Pi 5, 16GB RAM, 1TB NVMe
- ARM64 Linux (Raspberry Pi OS or equivalent)
- Nodes on the same local network (mDNS broadcast domain)
- NVMe dedicated to platform storage; OS on separate boot device

## Quick Start

```bash
# Build
cargo build --workspace

# Build for Pi5 (ARM64)
cargo build --workspace --target aarch64-unknown-linux-gnu --release

# Run tests
cargo test --workspace

# Test a specific crate
cargo test -p picloud-domain

# Bootstrap a cluster (on the first node)
picloud cluster init

# Check cluster status
picloud cluster status

# Deploy a product
picloud resource apply ./my-product/

# Stream events
picloud events stream
```

## Resource Definitions

Resources are declared in `.picloud` files using a Bicep-inspired syntax:

```bicep
product 'photo-app' = {
  version: '1.0.0'
  description: 'Photo sharing application'
}

volume 'media-store' = {
  product: 'photo-app'
  storageIntent: {
    durability: 'full-replication'
    performance: 'standard'
  }
  size: '100GB'
}

container 'api-server' = {
  product: 'photo-app'
  image: 'photo-api:1.0.0'
  identity: 'api-worker'
  mounts: [
    { volume: 'media-store', path: '/data' }
  ]
}
```

## Phase Plan

| Phase | Focus | Status |
|---|---|---|
| 1 (MVP) | Cluster + IAM + Storage + Workloads | In progress |
| 2 | Products + OIDC + Secrets | Planned |
| 3 | RDF stores + Event stores + SDKs | Planned |
| 4 | Operational maturity | Planned |

## Documentation

- [Product Requirements](docs/picloud-prd.md) — full PRD
- [Architecture Decisions](docs/picloud-adrs.md) — all ADRs
- [Claude Code Context](CLAUDE.md) — development guide

## License

Apache-2.0
