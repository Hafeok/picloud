# PiCloud — Product Requirements Document

> **Status:** Draft  
> **Version:** 0.1  
> **Companion:** See `picloud-adrs.md` for all architectural decisions

---

## 1. Vision

PiCloud is a single binary that turns a cluster of Raspberry Pi 5 nodes into a private cloud. You add nodes, capacity grows. You remove nodes, the platform adapts. No configuration changes, no external dependencies, no infrastructure to run the infrastructure.

The platform is built on a single foundational idea: **the cluster is a living event stream**. State is never stored directly — it is always derived from an append-only log of events. The RDF knowledge graph is the observable surface of that stream: a continuously maintained, queryable projection of everything that has happened in the cluster. Every deployment, every identity change, every storage allocation, every node join — all of it flows through the event log and surfaces in the graph.

This makes PiCloud a **sensing platform**. The cluster is always aware of itself. Workloads can subscribe to cluster events. Operators can query the graph to understand the current and historical state of every resource. Nothing is opaque.

Capabilities come from the platform, not from the operator. IAM, storage, service discovery, DNS, workload scheduling, and observability are built in. There are no plugins, no sidecars, no optional components. A node joins the cluster and immediately participates in all of these capabilities.

IaC is first-class. Nothing in PiCloud exists outside a resource definition. The Bicep-inspired resource language is the only interface between human intent and the platform. Resources are typed, versioned, and dependency-aware. The CLI emits commands as events and subscribes to the result stream — there is no synchronous request/response model.

PiCloud is designed for LLM-driven development. Resource definitions are the contract between intent and execution. The event log and RDF graph provide complete auditability. Every architectural decision in this document is explicit and justified so that the system can be built, extended, and reasoned about without tribal knowledge.

---

## 2. Goals

1. **Single binary operation** — one process per node, no external dependencies, no sidecar processes. The binary is the platform.
2. **Elastic capacity** — adding a node expands compute, storage, and replication capacity automatically. No configuration changes required.
3. **Event-sourced cluster state** — every state change in the platform is an event. The RDF graph is a continuously maintained projection of the event log. There is no other state store.
4. **Sensing platform** — the cluster is always aware of itself. Every resource change is observable. Workloads can subscribe to platform events natively.
5. **Products as deployment units** — every workload is deployed as a Product. A Product is a versioned, hermetically sealed deployment boundary with its own IAM scope, storage, networking, event bus, and SPARQL graph.
6. **Platform as identity provider** — PiCloud is a full OIDC provider. Products are App Registrations. Human users and workload identities are both first-class. Applications built on PiCloud never need an external IdP.
7. **Event-driven inter-product communication** — Products do not share resources. The only interfaces between Products are events they emit and SPARQL graphs they expose. This is enforced by the platform, not by convention.
8. **Self-documenting cluster** — every Product declares its ontology as a versioned `.ttl` or `.shacl` file. The cluster-level RDF graph is a semantic registry of all Products, their event interfaces, and their graph schemas.
9. **Storage intent model** — Products declare storage requirements semantically (durability, performance characteristics). The platform provisions and manages the underlying block storage accordingly.
10. **IaC as the only interface** — nothing exists in PiCloud outside a resource definition. Every resource is declared in a Bicep-inspired resource file, versioned, and auditable.
11. **CLI as primary interface** — all cluster operations are performed via the `picloud` CLI. The CLI emits commands as events and subscribes to the result stream.
12. **ARM64 native** — the platform is designed and optimized for Raspberry Pi 5 hardware. All dependencies compile to ARM64 with no emulation.
13. **Product event store** — every Product can declare a managed event store. The platform handles persistence, replication, schema versioning, and RDF projection. Developers get event sourcing without building it.
14. **Generated multi-language SDKs** — the platform generates SDKs in Rust, TypeScript, and .NET from its own RDF ontology. SDKs are published to crates.io, npm, and NuGet automatically on platform releases, and on-demand from any live cluster.

---

## 3. Non-Goals

1. **Multi-architecture support** — PiCloud targets ARM64 exclusively. x86_64 support is not planned.
2. **Cloud provider integration** — PiCloud is not a hybrid cloud platform. There is no Azure, AWS, or GCP integration.
3. **Web UI** — the management interface is CLI-only. A web portal may be built as a Product on top of PiCloud at a later stage.
4. **GPU or hardware accelerator scheduling** — resource scheduling is CPU and memory only. Hardware accelerators are out of scope.
5. **Object storage** — PiCloud provides block storage and RDF graph storage. S3-compatible object storage is not in scope.
6. **Multi-cluster federation** — a PiCloud cluster is a single administrative domain. Federation across multiple clusters is not planned.
7. **Windows or macOS node support** — nodes run Linux on ARM64. Other operating systems are not supported.
8. **Shared resources between Products** — Products are hermetically sealed. Cross-product resource sharing is explicitly not supported. All inter-product communication is via events and SPARQL graph queries.
9. **Synchronous inter-product calls** — Products do not call each other directly. There is no service mesh or RPC framework between Products.

---

## 4. Target Environment

**Hardware:** Raspberry Pi 5, 16GB RAM, 1TB NVMe per node. Clusters of 1–N nodes on a local network. The platform is designed and tested for this hardware profile. Other ARM64 Linux hardware may work but is not a supported configuration.

**Network:** Nodes communicate over a local network. mDNS is used for automatic node discovery — no static IP configuration or manual join steps are required. Nodes must be on the same broadcast domain for mDNS discovery to function.

**Operating system:** 64-bit Linux (ARM64). Raspberry Pi OS or equivalent.

**Storage:** NVMe drives are dedicated to platform-managed block storage. The operating system runs from a separate boot device (SD card or USB). The platform owns the NVMe entirely.

**Tenant identity:** Every cluster has a domain (default: `picloud.local`) and a cluster ID generated at `cluster init`. Together these form the dual tenant boundary — the domain is the human-readable identity, the cluster ID is the cryptographic boundary. All resource IRIs are scoped to the cluster domain. Multiple clusters on the same network are fully isolated by domain and CA. Changing a cluster's domain after init is not supported.

---

## 5. Core Architecture

PiCloud runs as a single binary (`picloud`) on every node. Each node is identical — there are no dedicated master nodes. Leadership is determined by Raft consensus and migrates automatically on node failure.

### Layers

```
┌─────────────────────────────────────────────────────────┐
│  CLI (picloud)                                          │
│  Emits command events, subscribes to result stream      │
├─────────────────────────────────────────────────────────┤
│  Resource API                                           │
│  Typed resources, dependency resolution, IAM            │
├─────────────────────────────────────────────────────────┤
│  Product Runtime                                        │
│  Workload scheduling, networking, storage allocation    │
│  Per-product event bus, SPARQL endpoint, ontology       │
├─────────────────────────────────────────────────────────┤
│  Platform Services                                      │
│  IAM (OIDC provider), DNS, service discovery            │
├─────────────────────────────────────────────────────────┤
│  Event Log + RDF Projection                             │
│  Append-only event store (Raft-replicated)              │
│  Oxigraph projection — the cluster's observable state   │
├─────────────────────────────────────────────────────────┤
│  Cluster Consensus (openraft)                           │
│  Leader election, log replication, node membership      │
├─────────────────────────────────────────────────────────┤
│  Node Discovery (mDNS)                                  │
│  Automatic peer discovery on local network              │
└─────────────────────────────────────────────────────────┘
```

### Data flow

Every operation in PiCloud follows the same path:

1. CLI emits a **command event** to the cluster
2. The Raft leader appends it to the **event log** and replicates it across nodes
3. The **RDF projector** processes the event and updates the Oxigraph graph
4. The CLI **subscribes to the result stream** and receives confirmation when the projection is complete
5. Subsequent state reads are served from the **RDF graph** — never from raw event log replay

This means every operation is eventually consistent by design. The CLI does not block on synchronous responses. It emits and subscribes.

### Node roles

Every node participates in:
- Raft consensus (voter or learner depending on cluster size)
- Event log storage
- RDF graph projection
- Block storage pool
- Workload scheduling

There are no dedicated storage nodes, no dedicated compute nodes, no dedicated control plane nodes. Every node is equal.

---

## 6. Resource Model

Everything in PiCloud is a resource. Resources are typed, versioned, named, and IAM-governed. There is no operation in the platform that does not create, update, or delete a resource.

### Resource addressing

Every resource in PiCloud has a canonical IRI that is both its unique identifier and its network location. IRIs follow a path-based hierarchy rooted at the cluster domain:

```
https://picloud.local/nodes/{node-name}
https://picloud.local/products/{product-name}
https://picloud.local/products/{product-name}/{resource-type}/{resource-name}
```

Examples:
```
https://picloud.local/nodes/pi-node-01
https://picloud.local/products/photo-app
https://picloud.local/products/photo-app/containers/api-server
https://picloud.local/products/photo-app/volumes/media-store
https://picloud.local/products/photo-app/identities/api-worker
https://picloud.local/products/photo-app/event-subscriptions/user-service-user-created
https://picloud.local/products/photo-app/graph
https://picloud.local/products/photo-app/ontology
https://picloud.local/products/photo-app/events
```

Every IRI is dereferenceable over HTTPS. The platform serves RDF representations at every resource IRI via HTTP content negotiation (`text/turtle`, `application/ld+json`, `application/json`). Resource IRIs are stable — they do not change when workloads reschedule to different nodes.

### Resource types

**Platform-scoped resources** (exist outside any Product):
- `node` — a cluster member
- `identity` — a human user
- `group` — a collection of identities sharing roles — membership managed by inference rules
- `role` — a platform-level RBAC role
- `inference-rule` — a SPARQL CONSTRUCT query that derives new graph facts, manages group membership, or fires alerts
- `capability` — a named, versioned interface contract (ontology + SHACL shapes + event schema); the operational sharing primitive — behaviour, not data (see ADR-054)
- `data-domain` — a governance namespace grouping related data products across Products; declares a steward identity, sensitivity classification, and domain-level SHACL constraints enforced at `resource apply` time (see ADR-055)

**Product-scoped resources**:
- `product` — the deployment unit itself, declares version, metadata, capability relationships (`implements`, `capabilities`), and data product dependencies (`dataProducts`)
- `container` — an OCI container workload
- `binary` — a raw executable workload
- `volume` — a block storage allocation with declared storage intent
- `rdf-store` — a managed Oxigraph instance with IAM-gated SPARQL endpoint; internal to the Product — not directly queryable by other Products (see ADR-055)
- `identity` — a workload identity (service account)
- `event-subscription` — a declared subscription to another Product's event stream
- `ontology` — a `.ttl` or `.shacl` file bound to the Product version
- `inference-rule` — a product-scoped SPARQL CONSTRUCT rule for alerts and derived facts
- `config` — typed key-value configuration store with tags, live-reloaded via events
- `feature-flag` — version-bound on/off flag, SDK-evaluated with event invalidation
- `data-product` — a versioned, published projection of selected domain data into a separate named graph; the analytical sharing primitive — knowledge, not behaviour (see ADR-055)

### Resource definition syntax

Resources are declared in `.picloud` files using a Bicep-inspired syntax:

```bicep
product 'photo-app' = {
  version:    '1.0.0'
  description: 'Photo sharing application'
  implements: ['gps-to-place@1.0.0']   // fulfils a capability contract
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
  env: {
    DB_URL: secret('db-connection')
  }
}

event-subscription 'user-created' = {
  product: 'photo-app'
  source:  'user-service@1.0.0'
  event:   'UserCreated'
  handler: 'api-server'
}

// Cluster-scoped capability — the operational sharing primitive
capability 'gps-to-place' = {
  version:     '1.0.0'
  description: 'Translates GPS coordinates to a named place with confidence score'
  ontology:    './capabilities/gps-to-place.ttl'
  shapes:      './capabilities/gps-to-place.shacl'
  events: {
    input:  'CoordinatesReceived'
    output: 'PlaceResolved'
  }
}

// Cluster-scoped data domain — the governance grouping for analytical data
data-domain 'geospatial' = {
  description: 'Location and mapping data products across the cluster'
  steward:     'identity/alice'
  sensitivity: 'internal'
}

// Product-scoped data product — the analytical sharing primitive
// Published into a separate named graph; never exposes the internal graph directly
data-product 'photo-locations' = {
  product:     'photo-app'
  domain:      'geospatial'
  version:     '1.0.0'
  description: 'Geo-tagged photo locations aggregated by resolved place'
  ontology:    './data-products/photo-locations.ttl'
  shapes:      './data-products/photo-locations.shacl'
  projection:  './data-products/photo-locations.rq'   // SPARQL CONSTRUCT over internal graph
  freshness: {
    maxAge:   '15m'
    triggers: ['PlaceResolved', 'PhotoDeleted']       // capability output drives refresh
  }
  access: {
    visibility: 'cluster'
    roles:      ['data-consumer']
  }
}

// maps-app declares dependencies on both the capability and the data product
// In both cases the dependency is on the contract, not on photo-app directly
product 'maps-app' = {
  version:      '1.0.0'
  capabilities: [
    { capability: 'gps-to-place', minVersion: '1.0.0' }
  ]
  dataProducts: [
    { source: 'photo-app/photo-locations', minVersion: '1.0.0' }
  ]
}
```

### Dependency resolution

The platform resolves resource dependencies before provisioning. A container that references a volume will not start until the volume is provisioned. Dependencies are declared implicitly through resource references — there is no explicit `dependsOn` syntax.

### Resource lifecycle events

Every resource state change emits an event to the platform event log:

- `ResourceDeclared` — resource definition received
- `ResourceProvisioning` — platform is actively provisioning
- `ResourceReady` — resource is available
- `ResourceFailed` — provisioning failed, reason attached
- `ResourceDeleted` — resource and all its data removed

These events are projected into the RDF graph and are subscribable by any workload with appropriate IAM permissions.

---

## 7. Event & State Model

The event log is the source of truth for all cluster state. The RDF graph is the continuously maintained read model derived from it. No component writes state directly — all state changes flow through events.

### Event log

The event log is append-only and Raft-replicated across all nodes. Every event has:

```
{
  id:         UUID,
  type:       string,          // e.g. "ResourceReady", "NodeJoined"
  timestamp:  ISO8601,
  source:     resource-path,   // who emitted it
  product:    string | null,   // product scope if applicable
  payload:    {}               // event-specific data
}
```

Events are never modified or deleted. The log is the permanent record of everything that has happened in the cluster.

### RDF projection

The Oxigraph triplestore is populated by projectors — components that consume events from the log and write triples. Each event type has a corresponding projector. Projectors are deterministic: replaying the event log from the beginning always produces the same graph.

The cluster-level graph contains:
- All nodes and their status
- All Products, their versions, and their resource inventories
- All identities and role assignments
- All event subscription relationships between Products
- All ontology declarations and their bindings to Product versions

### Named graphs — operational and analytical planes

Each Product maintains two categories of named graph within Oxigraph:

**Operational graph** (`https://picloud.local/products/{name}/graph`) — the Product's internal RDF state. Live projection of the event log. Schema evolves with the domain. Private: accessible only to workloads within the Product's own IAM scope. Cross-product SPARQL access to the operational graph is rejected at the HTTP layer.

**Data product graphs** (`https://picloud.local/products/{name}/data-products/{dp-name}/graph`) — published, versioned analytical projections. Each `data-product` resource has its own named graph, populated by a SPARQL CONSTRUCT query run against the operational graph on declared trigger events. IAM-gated for consumers. These are the only cross-product readable surfaces in the cluster.

The cluster-level graph contains:
- All nodes and their status
- All Products, their versions, and their resource inventories
- All identities and role assignments
- All event subscription relationships between Products
- All ontology declarations and their bindings to Product versions
- All capabilities, their implementors and consumers
- All data domains, their data products, freshness SLOs and dependency graph

**Platform event stream** — internal cluster events (node joins, resource lifecycle, IAM changes). Available to workloads with platform-level IAM permissions.

**Product event stream** — domain events emitted by a Product's workloads. Declared in the resource definition. Other Products subscribe via `event-subscription` resources. The platform routes events between Products — Products never communicate directly.

### Observability

Because all state is derived from events, the platform provides complete historical observability for free. Any point-in-time cluster state can be reconstructed by replaying the event log to that timestamp. This is not a debugging tool — it is a fundamental property of the architecture.

### Replay

Replay is a first-class platform and product capability. Any product or the platform itself can replay its event log over any time range, against specific aggregates, or in bulk batches of up to 1000 aggregates.

Replay always uses the **currently deployed version's projectors** — not the projectors that originally processed the events. This is how bugs in historical projectors are corrected retroactively. Schema IRIs on every event (ADR-031) ensure current projectors can correctly interpret any historical payload.

Replay builds a **shadow projection** in a separate named graph while the live graph continues serving traffic. When the shadow projection is complete, it is atomically swapped with the live graph. Live traffic is never interrupted.

Replayed events are re-emitted to all active subscribers with a `replay` marker field containing the `replay_id`, `original_timestamp`, and `replayed_at`. Subscribers can inspect this field to suppress irreversible side effects (emails, payments) while still updating their projections. The event `id` field ensures fully idempotent subscribers require no changes.

Replay is accessible via the CLI, the HTTP API, and the SDK. It emits its own lifecycle events (`ReplayStarted`, `ReplayProgress`, `ReplayCompleted`, `ReplayFailed`) which are projected into the cluster RDF graph and subscribable via the standard event stream.

---

## 8. IAM Model

PiCloud is a full OIDC provider. It issues tokens, manages identities, and enforces authorization for both platform operations and application-level authentication. There is no external IdP dependency.

### Identity types

**Human identities** — users who interact with the cluster via the CLI or via applications built on PiCloud. Authenticated via OIDC flows. Assigned platform-level roles and/or Product-level roles.

**Workload identities** — service accounts assigned to containers and binaries. The platform automatically injects credentials into workloads at runtime. Workloads never handle secrets directly — they exchange their injected identity token for scoped access tokens.

### Product as App Registration

Every Product acts as an OIDC App Registration. When a user authenticates against a Product-hosted application:

1. The application redirects to the PiCloud OIDC authorization endpoint
2. The user authenticates against their platform identity
3. PiCloud issues a token scoped to that Product with the user's roles within that Product
4. The application validates the token against PiCloud's JWKS endpoint

This means every application built on PiCloud gets SSO, token management, and user management for free.

### IAM scopes

**Platform scope** — governs access to cluster operations: node management, Product creation, platform identity management.

**Product scope** — governs access to a Product's resources and determines what roles a user has within an application built on that Product.

A user can have different roles in different Products. Platform administrators are not automatically administrators of all Products.

### RBAC

Roles are declared as resources. Permissions are additive. Every API operation on every resource requires an explicit permission check against the caller's identity token.

```bicep
role 'photo-viewer' = {
  product: 'photo-app'
  permissions: [
    'photo-app/containers/api-server:read'
    'photo-app/rdf-store/graph:query'
  ]
}
```

### Authentication — Passkeys and FIDO2

Human authentication in PiCloud uses passkeys and FIDO2 exclusively. There are no passwords. This applies to all human identity flows — platform administration, application login via OIDC, and CLI authentication.

**Browser-based flows** use the WebAuthn API. The platform's OIDC authorization endpoint initiates a WebAuthn ceremony. The user completes authentication with their platform authenticator (Touch ID, Face ID, hardware security key).

**CLI authentication** supports two modes:
- **Device flow** — the CLI initiates a device authorization flow, the operator completes passkey authentication in a browser on any device, the CLI polls for completion and receives a token
- **FIDO2 directly in terminal** — for operators with a hardware security key (YubiKey), FIDO2 assertion can be completed directly in the terminal without a browser

**App Registrations** (OAuth machine flows) use client ID and client secret as normal. Passkeys apply to human identities only — machine-to-machine authentication uses mTLS workload certificates and OAuth client credentials.

### Bootstrap

On a fresh cluster with no identities, `picloud cluster init` generates a one-time bootstrap token. The operator opens the platform's enrollment endpoint in a browser, exchanges the token for a passkey registration ceremony, and completes FIDO2 enrollment. This creates the first admin identity with the registered passkey bound to it. The bootstrap token is single-use and expires after 15 minutes.

### Passkey recovery

PiCloud enforces a three-tier recovery model:

**Tier 1 — Admin-initiated reset.** An admin can initiate a passkey reset for any user. The platform generates a single-use re-enrollment token (same mechanism as bootstrap) and the user registers a new authenticator. The previous passkey is revoked.

**Tier 2 — Backup key enforcement.** Admin accounts are required to have a minimum of two passkeys registered. The platform enforces this constraint — an admin cannot remove a passkey if it would leave them with fewer than two registered. Backup keys are typically a hardware security key stored offline.

**Tier 3 — Physical recovery.** If all admin accounts are inaccessible, an operator with physical access to any cluster node can run `picloud cluster recover` to generate a new bootstrap token. This requires local non-network access to the node and is logged as a high-severity event in the platform event log. This mirrors the original `cluster init` flow.

### Secret management

Secrets are first-class resources. They are encrypted at rest, replicated across the cluster, and injected into workloads by the platform. Workloads never see secret values directly in their resource definitions — they reference secrets by name and the platform handles injection.

```bicep
container 'api-server' = {
  env: {
    DB_PASSWORD: secret('db-password')
  }
}
```

---

## 9. Storage Model

### Block storage

Every node contributes its NVMe to a cluster-wide block storage pool. The platform manages allocation, replication, and placement. Operators never interact with individual disks.

Products declare storage intent — not storage implementation:

```bicep
volume 'media-store' = {
  product: 'photo-app'
  size: '100GB'
  storageIntent: {
    durability: 'full-replication'   // replicate to all available nodes
    performance: 'standard'          // balanced read/write
  }
}
```

**Durability tiers (MVP):**
- `full-replication` — data is replicated to every node. Maximum durability. Default for MVP.

**Snapshots — local NAS (ADR-047):**
Point-in-time immutable copies of a volume stored on a local NAS. Fast recovery from accidental deletion, corruption, or logical failures without internet dependency. Retention policy controls how many daily, weekly, and monthly snapshots are kept. Snapshots are stored separately from cluster NVMe to preserve live storage capacity.

**Offsite backup — S3-compatible (ADR-047):**
Encrypted, deduplicated, incremental backups to any S3-compatible endpoint (Backblaze B2, Cloudflare R2, self-hosted MinIO). Protects against total cluster loss — fire, flood, or theft. Data is encrypted client-side before upload. The platform manages scheduling, chunking, deduplication, and retention.

**The three layers together:**
```
live data:   cluster NVMe  (full-replication across nodes)
snapshots:   local NAS     (fast recovery, no internet)
offsite:     S3 endpoint   (disaster recovery, survives total cluster loss)
```

Replication protects against hardware failure. Snapshots protect against accidental deletion and logical failures. Offsite protects against physical disasters. All three are declared in a single volume resource definition.

**Durability tiers (future phases):**
- `quorum` — replicated to a majority of nodes
- `local` — single node, no replication, for ephemeral or cache workloads
- `none` — ephemeral storage, lost on container restart

**Performance tiers (future phases):**
- `fast` — optimized for low-latency read/write (e.g. databases)
- `standard` — balanced
- `archive` — optimized for sequential write, infrequent read

### Volume types

**Mounted volumes** — presented as a filesystem path inside a container or binary. The platform handles mount lifecycle.

**Raw block devices** — presented as a raw block device. For workloads that manage their own filesystem (e.g. databases).

### RDF graph storage

Each Product that declares an `rdf-store` resource gets a dedicated Oxigraph instance. The instance is:
- Backed by a platform-managed block volume with `full-replication` durability
- IAM-gated — all SPARQL queries and updates require a valid identity token
- Accessible via a SPARQL 1.1 endpoint scoped to the Product
- Automatically backed by the platform event log — graph mutations are events

The cluster-level RDF graph (platform state) is a separate Oxigraph instance managed by the platform, not accessible to workloads directly.

---

## 10. Workload Model

PiCloud runs two kinds of workloads: OCI containers and raw binaries. Both are scheduled, monitored, and managed identically by the platform.

### Scheduling

The scheduler assigns workloads to nodes based on available CPU and memory. Scheduling is automatic — operators do not specify which node runs a workload. Constraints (affinity, anti-affinity) are a future phase concern.

When a node fails, its workloads are rescheduled to remaining nodes automatically. The event log records the failure and rescheduling as events, which are projected into the RDF graph.

### OCI containers

Containers are run via an embedded OCI runtime (youki). Images are pulled from any OCI-compatible registry. The platform injects:
- Workload identity credentials
- Secret values (as environment variables or mounted files)
- Volume mounts
- Network configuration

```bicep
container 'api-server' = {
  product: 'photo-app'
  image: 'registry.example.com/photo-api:1.0.0'
  identity: 'api-worker'
  resources: {
    cpu: '500m'
    memory: '512MB'
  }
  mounts: [
    { volume: 'media-store', path: '/data' }
  ]
}
```

### Raw binaries

Binaries are ARM64 executables deployed as platform-managed processes. Useful for native Rust services, scripts, or workloads where container overhead is undesirable. The same identity injection, secret injection, and volume mount model applies.

```bicep
binary 'background-worker' = {
  product: 'photo-app'
  executable: 'worker-arm64'
  identity: 'background-worker-identity'
  resources: {
    cpu: '250m'
    memory: '256MB'
  }
}
```

### Health and restart policy

The platform monitors workload health via process liveness and optional HTTP health endpoints. Failed workloads are restarted according to their declared restart policy. All health state changes are emitted as events and projected into the RDF graph.

---

## 11. Networking Model

### HTTP and DNS as the RDF identity layer

RDF is an HTTP-native technology. IRIs are simultaneously the identifier and the locator for every resource. PiCloud treats this as a first-class architectural constraint — every resource in the platform has a stable, dereferenceable IRI. DNS and HTTP are not networking conveniences, they are the identity layer of the entire RDF model.

The cluster runs on a single domain: `picloud.local` (configurable). Every resource in the cluster has a canonical IRI following a path-based hierarchy:

```
https://picloud.local/                                           # cluster root
https://picloud.local/nodes/pi-node-01                          # node
https://picloud.local/products/photo-app                        # product
https://picloud.local/products/photo-app/containers/api-server  # container
https://picloud.local/products/photo-app/volumes/media-store    # volume
https://picloud.local/products/photo-app/identities/api-worker  # workload identity
https://picloud.local/products/photo-app/graph                  # SPARQL endpoint
https://picloud.local/products/photo-app/ontology               # ontology file
https://picloud.local/products/photo-app/events                 # event stream
```

Every IRI is dereferenceable. The platform serves each resource IRI with HTTP content negotiation:

```
Accept: text/turtle            → Turtle RDF representation
Accept: application/ld+json    → JSON-LD representation
Accept: application/json       → Plain JSON representation
Accept: text/html              → Human-readable view (future portal)
```

This means the cluster is a Linked Data platform by construction. Any RDF tool, SPARQL client, or LLM that can dereference an IRI can navigate the entire cluster graph by following links.

### Internal DNS

The platform runs an internal DNS resolver. Every node and product is registered at its canonical hostname derived from the IRI hierarchy:

```
picloud.local               → cluster ingress
pi-node-01.picloud.local    → direct node access
```

Products and their resources are served via path routing under the cluster domain — not subdomains. A single wildcard TLS certificate is not required. Each product gets a TLS certificate for `picloud.local/products/{name}` path space, issued by the platform's built-in CA.

Workloads address each other using their canonical IRIs. The platform's internal DNS resolves `picloud.local` to the cluster ingress, which routes by path to the correct node and product.

### Service discovery

The cluster root IRI (`https://picloud.local/`) returns a Turtle or JSON-LD document describing all Products, their IRIs, their event stream endpoints, their SPARQL endpoints, and their ontology locations. This is the semantic service registry — fully navigable by following IRI links.

### Ingress

The platform manages ingress routing for all resource IRIs automatically. No explicit ingress resource is needed for platform-managed resources. For workloads that expose custom HTTP endpoints, an ingress resource maps a path under the product's IRI space:

```bicep
ingress 'api-ingress' = {
  product: 'photo-app'
  target: 'api-server'
  port: 8080
  path: '/products/photo-app/api'
  tls: true
}
```

### Workload communication and mTLS

PiCloud enforces low coupling, high cohesion at the network layer. The event bus and the SPARQL graph are intentionally separate interfaces — events for fire-and-forget domain communication, SPARQL for read queries. This decoupling means a Product can evolve its internal state without breaking event subscribers, and event subscribers can react without needing to query.

**Workload → platform event bus:** All event publishing and subscription is routed via the platform. The platform enforces IAM on every event operation and maintains the full audit trail in the event log. Transport is mTLS — the platform issues certificates to workloads at runtime.

**Workload → product SPARQL endpoint:** SPARQL queries go directly from the querying workload to the target Product's SPARQL endpoint over mTLS. The platform issues certificates to both parties. IAM is enforced at the SPARQL endpoint, not by routing through the platform. This avoids an unnecessary hop for request-response queries.

**Node-to-node communication:** All node-to-node communication (Raft replication, storage replication, event routing) uses mTLS. Certificates are issued by the platform's built-in CA at node join time. No external PKI is required.

The separation of event bus (platform-routed) and SPARQL (direct) is a deliberate expression of the platform's coupling model: events are loosely coupled by design and benefit from platform mediation; graph queries are a known dependency between two Products and benefit from directness.

---

## 12. IaC & CLI Design

### Resource files

Resources are declared in `.picloud` files. A Product and all its resources can be declared in a single file or split across multiple files. The platform resolves dependencies across files.

Files are the source of truth. Deleting a resource from a file and redeploying cascades deletion to the platform.

### CLI commands

```bash
# Cluster management
picloud cluster init                               # default tenant (picloud.local)
picloud cluster init --domain acme.local           # named tenant
picloud cluster init --domain acme.local \         # BYO CA
  --ca-cert ./acme-ca.pem --ca-key ./acme-ca-key.pem
picloud cluster recover                            # physical recovery
picloud cluster status                             # query cluster state from RDF graph

# Resource operations
picloud resource apply ./photo-app/     # deploy all .picloud files in directory
picloud resource delete ./photo-app/    # delete all resources declared in directory
picloud resource status photo-app       # query product status from RDF graph

# Identity operations
picloud identity create --name alice    # create human identity
picloud identity token                  # get CLI token for current user

# Event stream
picloud events stream                   # subscribe to platform event stream
picloud events stream --product photo-app  # subscribe to product event stream

# Replay
picloud cluster replay --from "2025-06-01T00:00:00Z"               # platform replay
picloud resource replay photo-app --from "2025-06-01T00:00:00Z"    # product replay
picloud resource replay photo-app \                                 # aggregate replay
  --aggregate Photo \
  --id 123e4567-e89b-12d3-a456-426614174000 \
  --from "2025-06-01T00:00:00Z"
picloud resource replay photo-app \                                 # batch replay
  --aggregate Photo --ids-file ./photo-ids.txt \
  --from "2025-06-01T00:00:00Z"

# Graph queries
picloud graph query --sparql "SELECT ?s ?p ?o WHERE { ?s ?p ?o }"
picloud graph query --product photo-app --sparql "..."

# Telemetry queries (ADR-046)
picloud telemetry query --signal traces \
  --from "2025-07-01T00:00:00Z" --to "2025-07-01T01:00:00Z" \
  --sql "SELECT operation_name, AVG(duration_ms) FROM traces GROUP BY operation_name"
picloud telemetry query --signal metrics --sql "SELECT * FROM metrics WHERE product = 'photo-app'"
```

### Command execution model

Every CLI command follows the same model:

1. The CLI authenticates using the current identity token
2. The command is serialized as a command event and submitted to the cluster
3. The CLI subscribes to the result stream, filtered by the command's correlation ID
4. The platform processes the command, emits result events
5. The CLI renders the result when the terminal event arrives (e.g. `ResourceReady` or `ResourceFailed`)

This means all CLI operations are non-blocking by default. Long-running operations (large volume allocation, multi-container deployment) stream progress events to the CLI in real time.

### Idempotency

Every command event carries a client-generated idempotency key. The platform deduplicates commands by key. Re-running `picloud resource apply` on an unchanged set of files is safe and produces no effect.

---

## 13. Product Event Store

The platform exposes its own event sourcing infrastructure as a first-class storage primitive for Products. A developer building a Product declares an `event-store` resource and gets a fully managed event log, aggregate streams, schema versioning, and RDF projection — without building any of it.

### Resource declaration

```bicep
event-store 'photos' = {
  product: 'photo-app'
  aggregates: [
    {
      type: 'Photo'
      schema: 'schemas/photo-events.ttl'
    }
    {
      type: 'Album'
      schema: 'schemas/album-events.ttl'
    }
  ]
}
```

The `.ttl` or `.shacl` schema files are deployed with the Product and bound to its version. Schemas only change when the Product version changes — the event log for a given version is immutable in its schema contract.

### What the platform provisions

- A replicated event log scoped to the Product, backed by the same Raft-replicated infrastructure as the platform event log
- An aggregate stream per declared type — events for `Photo/123` are addressable as a coherent stream
- Schema IRIs served at `https://picloud.local/products/{name}/schemas/events/{EventType}/v{n}` — dereferenceable, permanent
- Automatic projection of aggregate events into the Product's Oxigraph RDF store via platform-managed projectors
- The projected graph is immediately queryable via the Product's SPARQL endpoint

### HTTP API

Workloads interact with the event store via HTTP — consistent with the IRI model:

```
# Append an event to an aggregate stream
POST https://picloud.local/products/photo-app/event-store/photos/Photo/123/events
Content-Type: application/json
Authorization: Bearer {workload-token}

{
  "schema": "https://picloud.local/products/photo-app/schemas/events/PhotoCreated/v1",
  "type": "PhotoCreated",
  "payload": { ... }
}

# Read an aggregate stream
GET https://picloud.local/products/photo-app/event-store/photos/Photo/123/events

# Read current aggregate state (from RDF projection)
GET https://picloud.local/products/photo-app/event-store/photos/Photo/123
Accept: text/turtle
```

All endpoints are IAM-gated using the workload's mTLS certificate and identity token.

### Schema lifecycle

Event schemas are declared as `.ttl` or `.shacl` files and deployed as part of the Product. They are bound to the Product version — a schema cannot change without a Product version change. The platform serves all past schema versions permanently, ensuring the event log remains interpretable forever.

---

## 15. Inference, Metrics & Alerts

### Tagging

Every platform resource supports an arbitrary set of `key:value` tags. Tags are declared in resource definition files and manageable via CLI. Tag changes emit `TagAdded` and `TagRemoved` events, which are projected into the RDF graph and immediately trigger inference rule evaluation.

```bicep
container 'api-server' = {
  product: 'photo-app'
  image: 'photo-api:1.0.0'
  tags: {
    'team': 'backend'
    'environment': 'production'
  }
}
```

### Groups

A `group` is an IAM resource that holds a set of roles. Users in a group inherit all roles assigned to it. Group membership is managed automatically by SPARQL CONSTRUCT inference rules — never by manual assignment.

```bicep
group 'backend-developers' = {
  roles: ['product-developer', 'log-viewer']
  tags: { 'team': 'backend' }
}

inference-rule 'backend-group-membership' = {
  scope: 'platform'
  trigger: 'event'
  trigger-events: ['TagAdded', 'TagRemoved', 'IdentityCreated']
  reconciliation: true
  construct: '''
    CONSTRUCT {
      <https://picloud.local/groups/backend-developers>
          picloud:hasMember ?user .
    }
    WHERE {
      ?user a picloud:HumanIdentity ;
            picloud:tag [ picloud:tagKey "team" ; picloud:tagValue "backend" ] .
    }
  '''
}
```

When a user receives the tag `team:backend`, the rule fires within one event cycle and the user is added to the group. Their next token includes the inherited roles.

### Inference rules

SPARQL CONSTRUCT queries are a first-class resource type. Rules run on matching events and on a 10-minute reconciliation schedule. Produced triples are written to the appropriate named graph. New or retracted triples emit events.

Two inference layers work together:
- **RDFS/OWL inference** (Oxigraph built-in) — structural facts from ontology axioms. Subclass hierarchies, transitive properties, equivalences. Always live, no trigger needed.
- **SPARQL CONSTRUCT rules** — operational rules. Group membership, alert conditions, derived state. Event-driven with reconciliation safety net.

### Hardware metrics

The platform ships a built-in metrics agent in the `picloud-server` binary. Every node samples hardware metrics every 15 seconds and emits `MetricRecorded` events:

- CPU usage (%) — per core and aggregate
- Memory used / total (MB)
- Disk used / total / read rate / write rate
- CPU temperature (°C)
- Network bytes in/out

The RDF projector writes the latest values as triples on each node's IRI, overwriting previous values. Historical values are queryable via event log replay.

Product workloads emit their own domain metrics (request counts, error rates, latency) as events to the product event bus. The platform does not collect these — workloads emit them, the SDK provides helpers.

### Alerts

Alerts are produced by SPARQL CONSTRUCT rules that assert `picloud:Alert` triples. When a new alert triple is materialised, the platform emits `AlertFired`. When the condition clears and the triple is retracted, `AlertResolved` is emitted. No built-in notification targets — subscribers build notification products on top.

**Built-in platform alert rules:**

| Condition | Threshold | Severity |
|---|---|---|
| CPU temperature | > 80°C | critical |
| CPU temperature | > 70°C | warning |
| Memory usage | > 90% | critical |
| Memory usage | > 80% | warning |
| Disk usage | > 90% | critical |
| Node unreachable | Raft heartbeat missed | critical |
| Workload failed | `ResourceStatus = Failed` | critical |

Active alerts are always queryable from the cluster graph:
```bash
picloud graph query --sparql "SELECT * WHERE { ?a a picloud:Alert . }"
```

---

## 16. SDK Model

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

## 17. Phase Plan

### Phase 1 — Cluster Foundation (MVP)

**Goal:** Two nodes can form a cluster, a container can be scheduled and run, a volume can be allocated and mounted.

- [ ] Single binary compiles to ARM64
- [ ] mDNS node discovery
- [ ] Raft consensus and leader election (openraft)
- [ ] Append-only event log with Raft replication
- [ ] Oxigraph RDF projection of cluster state
- [ ] Basic IAM: human identities, workload identities, RBAC, token issuance
- [ ] Block storage pool: NVMe contribution, volume allocation, full-replication durability
- [ ] Mounted volume support
- [ ] OCI container scheduling (youki)
- [ ] Internal DNS and service discovery
- [ ] CLI: `cluster init`, `resource apply`, `resource status`, `identity create`
- [ ] mTLS node-to-node communication with platform-issued certificates

**Exit criteria:** A two-node cluster runs a containerized workload with a replicated volume. The cluster survives one node restart without data loss.

---

### Phase 2 — Products and IAM Completeness

**Goal:** Products can be deployed end-to-end. Platform acts as OIDC provider.

- [ ] Product resource type with versioning
- [ ] Product-scoped IAM and role assignment
- [ ] Platform as OIDC provider — authorization endpoint, token endpoint, JWKS
- [ ] Product as App Registration — OIDC client credentials
- [ ] Raw binary workload support
- [ ] Raw block device support
- [ ] Secret management — encrypted at rest, workload injection
- [ ] Cascading deletion — delete Product cascades to all child resources
- [ ] Per-product event bus
- [ ] Volume snapshots — NAS storage, configurable schedule and retention
- [ ] Offsite backup — S3-compatible, client-side encryption, incremental deduplication
- [ ] Snapshot and backup lifecycle events (SnapshotCreated, BackupCompleted, BackupFailed etc.)
- [ ] Backup failure alert rules (built-in)
- [ ] CLI: `picloud volume snapshots`, `picloud volume restore`, `picloud volume backup`
- [ ] Product configuration store — typed key-value with tags, workload override, live reload
- [ ] Feature flags — version-bound on/off, SDK evaluation, event invalidation
- [ ] `PICLOUD_PRODUCT_VERSION` injected into all workloads at startup
- [ ] OTel environment variables injected into all workloads at startup
- [ ] OTLP endpoint at `https://picloud.local/otel`
- [ ] OTel event stream — in-process pub/sub for traces, metrics, logs
- [ ] Parquet time-series store — traces, metrics, logs with hourly partitioning
- [ ] DataFusion SQL over Parquet via `picloud telemetry query`
- [ ] Metric aggregator — OTel stream → MetricRecorded events every 15s
- [ ] CLI traces — every command produces an OTel trace
- [ ] W3C trace context propagation — platform to workload correlation
- [ ] Telemetry retention policy — configurable per signal type
- [ ] CLI: `events stream`, `graph query`, `identity token`, `telemetry query`

**Exit criteria:** A Product with a container, volume, and workload identity deploys end-to-end. A user authenticates against a Product-hosted application via OIDC.

---

### Phase 3 — RDF, Event Store, Inference, Metrics & Alerts

**Goal:** Products have first-class RDF storage and event sourcing. Inference rules, group membership, hardware metrics, and alerts are operational. SDKs ship.

- [ ] `rdf-store` resource type — managed Oxigraph per Product; internal graph private to owning product (ADR-055)
- [ ] IAM-gated SPARQL endpoint per Product — internal graph access restricted to owning product and platform admin only
- [ ] Ontology resource type — `.ttl` and `.shacl` files bound to Product version
- [ ] RDFS/OWL inference enabled on platform and product graphs
- [ ] Universal tagging — `TagAdded`/`TagRemoved` events, SPARQL-queryable on all resources
- [ ] `group` resource type — roles assignable to groups, users inherit
- [ ] `inference-rule` resource type — SPARQL CONSTRUCT, event-triggered + 10min reconciliation
- [ ] Group membership rules via inference engine
- [ ] `capability` resource type — cluster-scoped interface contract with ontology, SHACL shapes, and declared input/output event types (ADR-054)
- [ ] `implements` field on `product` — structural SHACL conformance validated at `resource apply` time
- [ ] `capabilities` field on `product` — resolution validated at `resource apply` time; deployment blocked if unfulfilled
- [ ] Capability lifecycle events — `CapabilityDeclared`, `CapabilityReady`, `CapabilityImplementorAdded`, `CapabilityImplementorRemoved`, `CapabilityUnfulfilled`, `CapabilityDeleted`
- [ ] Capability-aware event routing — platform resolves implementing Product at dispatch time; highest satisfying version wins
- [ ] `picloud capability list` — all capabilities, implementors, consumers, and fulfilment status
- [ ] `data-domain` resource type — cluster-scoped governance boundary; required before any data product can be assigned (ADR-055)
- [ ] `data-product` resource type — product-scoped; own named graph, push-triggered SPARQL CONSTRUCT projection, declared freshness SLO, domain assignment (ADR-055)
- [ ] Projection runner — subscribes to declared trigger events, executes CONSTRUCT over internal graph, shadow-swaps data product named graph, emits `DataProductUpdated`
- [ ] Freshness monitor — tracks `maxAge` per data product; emits `DataProductStale` when breached; integrates with alert inference rules
- [ ] `dataProducts` field on `product` — consumer dependency validated at `resource apply` time; deployment blocked if data product not present at required version
- [ ] Data product lifecycle events — `DataProductDeclared`, `DataProductReady`, `DataProductUpdated`, `DataProductStale`, `DataProductFailed`, `DataProductDeleted`
- [ ] Data domain lifecycle events — `DataDomainDeclared`, `DataDomainDeleted`
- [ ] `DataProductProjector` — cluster RDF graph reflects all data products, their domains, producers, consumers, and freshness status
- [ ] `picloud data-product list` and `picloud data-domain list` — mesh topology and freshness status
- [ ] Cross-product internal graph access blocked at HTTP layer — 403 for all non-owner, non-admin identities
- [ ] Platform metrics agent — `MetricRecorded` events at 15s interval per node
- [ ] Built-in platform alert rules (CPU temp, memory, disk, node health, workload failure)
- [ ] `AlertFired` / `AlertResolved` events
- [ ] `event-store` resource type — managed event log + aggregate streams per Product
- [ ] Product event schema IRIs served from platform HTTP layer
- [ ] Automatic RDF projection of Product aggregate events into Product graph
- [ ] Event replay — shadow projection, atomic swap, marked replay events
- [ ] Aggregate-scoped replay (single and batch up to 1000)
- [ ] `event-subscription` resource type
- [ ] Platform-managed event routing between Products
- [ ] Product discoverability — cluster SPARQL query returns all Products, their events, their ontologies, their capabilities, and their published data products
- [ ] SDK generator — Rust, TypeScript, .NET generated from platform ontology
- [ ] SDK publication — crates.io, npm, NuGet via platform CI
- [ ] `picloud sdk publish` command
- [ ] .NET Aspire integration package

**Exit criteria:** An inference rule automatically assigns a user to a group on tag change. A CPU temperature alert fires and resolves. A Product appends to its event store and queries the RDF projection. A data product is declared, its projection is rebuilt on a trigger event, and a second product queries it. A capability is declared and fulfilled by an implementing product. SDKs are published.

---

### Phase 4 — Operational Maturity

- [ ] Additional storage intent tiers (quorum, local, archive, fast)
- [ ] Workload resource constraints (CPU/memory limits)
- [ ] Node drain and graceful workload migration
- [ ] Event log compaction and snapshotting
- [ ] Platform self-monitoring via its own RDF graph
- [ ] Multi-node Raft voter configuration tuning

---

## 18. Open Questions

1. **CA trust distribution** — external clients (operator laptops, browsers, RDF tools) must trust the platform CA or BYO-CA to connect to `picloud.local` over HTTPS. The `picloud ca export` command handles certificate export, but the installation step is OS-specific and manual. A `picloud ca install` command that handles OS trust store installation on common platforms (macOS, Linux, Windows) would improve the setup experience.
