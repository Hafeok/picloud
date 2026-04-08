# PiCloud — Architecture Decision Records

> **Status:** Draft  
> **Version:** 0.1  
> **Companion:** See `picloud-prd.md` for full product requirements

---

## ADR-001: Rust as Implementation Language

**Status:** Accepted

**Context:** PiCloud must compile to a single ARM64 binary with no runtime dependencies. The platform handles storage, scheduling, and cryptography — domains where memory safety and predictable performance matter. An LLM will be writing most of the implementation code, so the language's type system and explicitness are assets.

**Decision:** Implement PiCloud in Rust.

**Rationale:**
- Single binary compilation to ARM64 with no runtime (no JVM, no GC)
- Memory safety guarantees without garbage collection pauses — critical for storage and scheduling paths
- The full stack is Rust-native: `openraft` (consensus), `oxigraph` (RDF), `youki` (OCI runtime), `mdns-sd` (mDNS) — no cross-language FFI required
- Rust's type system forces explicit error handling, which maps well to a distributed system where partial failure is the norm
- LLMs produce high-quality Rust when given explicit type contracts and architectural context

**Rejected alternatives:**
- **Go** — first instinct for systems tooling, but the key dependencies (Oxigraph, youki) are Rust-native. Go would require FFI bridges or inferior alternatives. Also, GC pauses are undesirable in storage hot paths.
- **C++** — memory safety is not guaranteed. Adds risk without benefit given Rust's maturity.

---

## ADR-002: openraft for Cluster Consensus

**Status:** Accepted

**Context:** PiCloud requires distributed consensus for leader election, event log replication, and cluster membership. This is the foundational distributed systems problem. Using a proven library is strongly preferred over implementing Raft from scratch.

**Decision:** Use `openraft` for Raft consensus.

**Rationale:**
- Pure Rust implementation, no FFI, compiles cleanly to ARM64
- Actively maintained with a well-documented API
- Storage and network layers are pluggable — PiCloud can provide its own implementations
- Used in production systems (TiKV, among others)
- Supports both voter and learner roles, enabling flexible cluster size management

**Rejected alternatives:**
- **hashicorp/raft** — Go only. Ruled out by ADR-001.
- **Custom Raft implementation** — Raft is notoriously subtle. The cost of correctness is too high for a project that should be building on top of consensus, not debugging it.

---

## ADR-003: mDNS for Node Discovery

**Status:** Accepted

**Context:** Nodes need to find each other on the local network. The target environment is a home lab — nodes are on the same broadcast domain, static IPs are undesirable, and there is no infrastructure DNS.

**Decision:** Use mDNS (Multicast DNS) for automatic node discovery. Nodes broadcast their presence on startup and discover peers passively. The platform also advertises `picloud.local` via mDNS, which means external clients (operator laptops, browsers, RDF tools) resolve the cluster domain automatically on any mDNS-capable OS — no DNS configuration required.

**mDNS client support:**
- macOS — native, zero configuration
- Linux — requires `avahi-daemon` (standard on most distributions)
- Windows 10+ — native mDNS resolver

This means external DNS is not a separate concern. The same mDNS mechanism that handles node discovery also handles client-side name resolution for `picloud.local`. One implementation, two purposes.

**Rationale:**
- Zero configuration — nodes join the cluster by powering on
- Works on any local network without infrastructure changes
- `mdns-sd` crate provides a production-quality Rust implementation
- Consistent with the "add a node and capacity grows" user experience

**Constraints:**
- Nodes must be on the same broadcast domain (same L2 network segment)
- mDNS does not work across routers without explicit multicast forwarding
- Not suitable for multi-site or WAN deployments (out of scope per PRD)

**Rejected alternatives:**
- **Static IP configuration** — requires manual intervention on every new node. Violates the zero-configuration goal.
- **Bootstrap token with known seed address** — requires knowing at least one node's address. Adds operational friction.
- **Consul/etcd for discovery** — external infrastructure dependency. Violates single-binary goal.

---

## ADR-004: Event Sourcing as Platform State Foundation

**Status:** Accepted

**Context:** Distributed systems need a consistent view of cluster state across nodes. Traditional approaches use a replicated key-value store (etcd, Consul) as a state store. PiCloud takes a different approach.

**Decision:** All platform state is derived from an append-only, Raft-replicated event log. No component writes state directly. State is always a projection of events.

**Rationale:**
- Complete audit trail of every cluster operation — no separate logging infrastructure needed
- Point-in-time state reconstruction by replaying the log to any timestamp
- Natural fit for eventually consistent operations — the CLI emits commands, subscribes to results
- Aligns with the "sensing platform" vision — every change is an observable event
- Projections (RDF graph) can be rebuilt from scratch by replaying the log, making schema migrations safe
- Event sourcing is well-understood and maps cleanly to Rust's algebraic types

**Consequences:**
- All reads go to the RDF projection, not the raw event log
- The log grows indefinitely — snapshotting and compaction must be addressed (see Open Questions)
- Eventual consistency is a first-class design constraint, not a compromise

**Rejected alternatives:**
- **etcd as state store** — external dependency, eventual consistency is hidden, no inherent audit trail
- **Embedded key-value store (sled, rocksdb)** — strong consistency possible but loses event history, audit trail, and time-travel queries

---

## ADR-005: RDF Graph as Event Projection and Read Model

**Status:** Accepted

**Context:** The event log is the source of truth, but raw event replay is not suitable for queries ("what is the current state of all containers in product X?"). A read model is needed. The choice of read model determines how the cluster is queried and observed.

**Decision:** The read model is an RDF knowledge graph (Oxigraph). All events are projected into the graph by deterministic projectors. All state reads are SPARQL queries against the graph.

**Rationale:**
- RDF naturally models the relationships between cluster resources (Products contain containers, containers reference volumes, identities are bound to workloads)
- SPARQL enables complex queries that would require multiple round-trips in a key-value model
- The graph is self-describing — ontologies can be queried to understand the schema
- Consistent with the application-level RDF storage model — the platform eats its own cooking
- Makes the cluster semantically discoverable — not just a list of resources, but a web of typed relationships
- Oxigraph is pure Rust, embedded, no external process required

**Consequences:**
- All platform developers need working knowledge of RDF and SPARQL
- Query performance is bounded by Oxigraph's capabilities — complex analytical queries over large clusters may need optimization
- Schema evolution requires projector updates and potentially graph migration

**Rejected alternatives:**
- **SQLite as read model** — relational model is less natural for graph-shaped cluster state. Joins become complex. No semantic discovery.
- **In-memory hash maps** — fast but not queryable, not persistent, not observable from outside the process.

---

## ADR-006: Oxigraph as Embedded Triplestore

**Status:** Accepted

**Context:** Both the platform's internal state (ADR-005) and application RDF storage require an embedded triplestore. The triplestore must be embeddable in a Rust binary, support SPARQL 1.1, and run on ARM64.

**Decision:** Use Oxigraph as the embedded triplestore for both platform state and per-product RDF stores.

**Rationale:**
- Pure Rust — embeds directly into the PiCloud binary, no separate process
- Full SPARQL 1.1 support including SPARQL Update
- Named graph support — platform and per-product graphs coexist in one instance
- Actively maintained
- Consistent with ADR-001 (Rust stack)

**Rejected alternatives:**
- **Apache Jena** — JVM dependency. Ruled out.
- **RDFox** — not open source.
- **External triplestore (Fuseki, Stardog)** — external process dependency. Violates single-binary goal.

---

## ADR-007: Bicep-Inspired Declarative Resource Syntax

**Status:** Accepted

**Context:** IaC is a first-class citizen in PiCloud. Operators and LLMs must be able to read and write resource definitions clearly. The syntax must be expressive enough to capture all resource types and their relationships.

**Decision:** Resource files use a Bicep-inspired syntax with typed resource declarations, property blocks, and symbolic references between resources.

**Rationale:**
- Bicep is well-understood by the target audience (developers on Microsoft stacks)
- Clear and readable — LLMs produce accurate Bicep-style syntax with minimal prompting
- Symbolic references between resources make dependencies explicit and readable
- The `resource 'type' 'name' = { }` pattern maps cleanly to PiCloud's resource model
- Avoids YAML's ambiguity (indentation errors, type coercion) and HCL's complexity

**File extension:** `.picloud`

**Rejected alternatives:**
- **YAML** — ubiquitous but ambiguous. Indentation errors are silent. Type coercion is surprising.
- **HCL (Terraform)** — well-designed but requires a large parser. Bicep is simpler and more readable.
- **Custom DSL** — unnecessary complexity. Bicep-inspired syntax covers all requirements.
- **JSON** — not human-writable at scale.

---

## ADR-008: Eventually Consistent Command Model

**Status:** Accepted

**Context:** The event-sourced architecture (ADR-004) means state changes are asynchronous. The CLI must reflect this. Blocking until a command is fully executed would require synchronous request-response, which conflicts with the distributed, event-driven model.

**Decision:** All CLI commands are eventually consistent. The CLI emits a command event with a correlation ID, subscribes to the platform event stream, and streams progress events until a terminal event (success or failure) arrives.

**Rationale:**
- Consistent with the event-sourced architecture — the CLI is just another event emitter and subscriber
- Long-running operations (volume allocation, multi-container deployment) stream real-time progress
- The model is transparent to the operator — they see what is happening, not just a spinner
- Commands are idempotent via client-generated idempotency keys (see ADR-015)

**User experience:**
```
$ picloud resource apply ./photo-app/
→ ResourceDeclared: photo-app
→ ResourceDeclared: media-store
→ ResourceProvisioning: media-store (allocating 100GB across 3 nodes)
→ ResourceReady: media-store
→ ResourceDeclared: api-server
→ ResourceProvisioning: api-server (scheduling on node pi-02)
→ ResourceReady: api-server
✓ photo-app deployed
```

**Rejected alternatives:**
- **Synchronous request-response** — incompatible with event-sourced architecture. Would require a separate synchronous state store.

---

## ADR-009: Standalone IAM — Users and Workload Identities

**Status:** Accepted

**Context:** Every operation in PiCloud requires an authenticated identity. Applications built on PiCloud need an IdP for user authentication. Requiring an external system (Authentik, Keycloak, Azure AD) would add infrastructure dependencies and complexity.

**Decision:** PiCloud is a standalone OIDC provider. It manages human identities, workload identities, token issuance, and JWKS. No external IdP integration in MVP. Products act as OIDC App Registrations.

**Rationale:**
- Zero external dependencies — the platform manages its own identity, consistent with single-binary goal
- Every application gets SSO and OIDC for free without additional infrastructure
- Workload identity is native — secrets are injected by the platform, workloads never handle credentials directly
- Platform IAM and application IAM are unified — one identity model for everything

**Consequences:**
- PiCloud must implement OIDC correctly — authorization endpoint, token endpoint, JWKS, refresh tokens
- Token signing keys must be managed by the platform and rotated safely
- This is the most security-critical component of the platform

**Rejected alternatives:**
- **External OIDC provider (Authentik, Keycloak)** — external infrastructure dependency. Requires another service to be running before PiCloud can function.
- **mTLS only (no OIDC)** — sufficient for workload-to-workload but does not cover human authentication or application-level user management.

---

## ADR-010: OCI Containers and Raw Binaries as Workload Primitives

**Status:** Accepted

**Context:** Workloads need to be schedulable on any node. OCI containers are the standard packaging format. Raw binaries are needed for native Rust services and lightweight workloads where container overhead is undesirable.

**Decision:** PiCloud supports two workload primitives: OCI containers (via youki) and raw ARM64 binaries. Both receive the same identity injection, secret injection, volume mount, and networking treatment.

**Rationale:**
- OCI containers are the standard — any existing containerized workload runs without modification
- Raw binaries enable PiCloud's own internal services to be deployed as Platform workloads (dogfooding)
- youki is a pure Rust OCI runtime — consistent with ADR-001, no external runtime dependency
- Unified resource model means containers and binaries are interchangeable from the scheduler's perspective

**Rejected alternatives:**
- **VMs** — too heavyweight for Pi5 hardware. Not suitable for the target environment.
- **WebAssembly** — interesting but tooling is immature for production workloads. Future consideration.
- **Containers only** — excludes lightweight native workloads and makes dogfooding harder.

---

## ADR-011: Block Storage Before RDF Application Storage

**Status:** Accepted

**Context:** Both block storage and RDF application storage (per-product Oxigraph) are in scope. Block storage is a dependency for RDF storage (Oxigraph needs a persistent block volume). Implementing both simultaneously adds unnecessary complexity to Phase 1.

**Decision:** Block storage is implemented in Phase 1. Per-product RDF storage is implemented in Phase 3.

**Rationale:**
- Block storage is a dependency of RDF storage — correct ordering
- Block storage is needed for containers in Phase 1 regardless
- Phasing reduces the surface area of Phase 1 to the minimum needed for a working cluster
- RDF application storage builds on the same block storage primitives — no rework required

---

## ADR-012: Mounted and Raw Block Device Support

**Status:** Accepted

**Context:** Different workloads have different storage access requirements. Databases typically want raw block devices to manage their own filesystems. Application containers typically want mounted filesystems.

**Decision:** PiCloud supports both mounted volumes (filesystem presented at a path) and raw block devices. Both are backed by the same distributed block storage pool.

**Rationale:**
- Mounted volumes cover the majority of use cases
- Raw block devices are required for databases (PostgreSQL, RocksDB) that manage their own storage layout
- Both types use the same allocation and replication mechanisms — no storage layer duplication

---

## ADR-013: Platform-Managed Replication Factor

**Status:** Accepted

**Context:** Distributed storage systems typically allow operators to specify a replication factor per volume. This adds operational complexity and creates risk (under-replicated volumes, operator error).

**Decision:** The platform manages replication factor automatically based on cluster size. In MVP, all data uses full-replication (replicated to every node). Operators declare storage intent (durability tier), not replication factor.

**Rationale:**
- Eliminates a class of operator error (forgetting to set replication, setting it too low)
- Consistent with the abstraction model — operators declare intent, platform decides implementation
- On a 5-node Pi cluster, full-replication is feasible and NVMe bandwidth is sufficient
- Full-replication in MVP simplifies the storage implementation significantly

**Future:** Additional durability tiers (quorum, local) will be added in Phase 4 as the storage implementation matures.

---

## ADR-014: Service Discovery and Internal DNS in MVP

**Status:** Accepted

**Context:** Workloads need to find each other by name. Without service discovery, container addresses are ephemeral and workloads must be reconfigured when peers restart or reschedule.

**Decision:** Internal DNS and service discovery are MVP features, not future phases. Every resource that accepts network traffic is automatically registered as `{resource}.{product}.picloud.internal`.

**Rationale:**
- Without service discovery, containers cannot find each other — the platform is not useful
- Internal DNS is a small implementation surface relative to its impact
- Automatic registration means operators never configure DNS manually

---

## ADR-015: Imperative API with Idempotent Execution

**Status:** Accepted

**Context:** Two approaches exist for IaC execution: declarative-convergent (platform continuously reconciles desired vs actual state, like Kubernetes) and imperative (operator runs a command, it executes once). Declarative requires a reconciliation loop and continuous state comparison. Imperative is simpler but risks partial application on failure.

**Decision:** The API is imperative from the operator's perspective — `picloud resource apply` deploys what is declared. Internally, every operation is idempotent via client-generated idempotency keys.

**Rationale:**
- No background reconciliation loop — the platform only acts when commanded
- Simpler implementation — no desired-state vs actual-state diffing engine required
- Idempotency via keys means re-running `apply` on unchanged files is safe and produces no effect
- This is how Azure ARM works — Bicep/ARM feels imperative but deployments are idempotent
- Failure recovery is explicit — the operator reruns `apply`, the platform deduplicates

**Consequences:**
- Drift detection (platform state diverges from declared files) is not automatic — the operator is responsible for reapplying when drift occurs
- A future `picloud resource diff` command could surface drift on demand

**Rejected alternatives:**
- **Declarative-convergent (Kubernetes model)** — requires a reconciliation loop, desired-state storage, and a diffing engine. Significant complexity for a system that prioritises simplicity.

---

## ADR-016: Product as Native Deployment Unit

**Status:** Accepted

**Context:** Workloads need a deployment boundary — a unit that groups related resources, provides an IAM scope, and has a lifecycle (deploy, update, delete). Without this, operators manage individual resources with no grouping concept.

**Decision:** Every workload in PiCloud is deployed as a Product. A Product is a versioned, hermetically sealed deployment boundary. It groups all resources needed for an application: containers, volumes, identities, RDF stores, event subscriptions, and ontologies. Deleting a Product cascades deletion to all its resources.

**Rationale:**
- Maps directly to how developers think about applications — "deploy the photo app", not "deploy container A and volume B and identity C"
- Versioning is built into the Product concept — a Product at version 1.0.0 is a distinct identity from 1.1.0
- IAM scoping per Product means access control is at the application level, not the resource level
- Cascading deletion prevents orphaned resources
- One active version per Product prevents version sprawl and simplifies the operational model

---

## ADR-017: Platform as Full OIDC Provider

**Status:** Accepted

**Context:** Applications built on PiCloud need user authentication. The platform manages identities (ADR-009). Extending the platform to a full OIDC provider means applications never need an external IdP.

**Decision:** PiCloud implements the OIDC authorization code flow. It exposes an authorization endpoint, token endpoint, and JWKS endpoint. Products act as OIDC clients (App Registrations). Users authenticate against their platform identity and receive Product-scoped tokens.

**Rationale:**
- Applications get SSO for free — no Keycloak, no Authentik, no Auth0 required
- The identity model is unified — the same identity a user uses for `picloud` CLI is the identity they use for applications
- Product-scoped tokens mean a user's permissions within an application are distinct from their platform permissions

**Security requirements:**
- Token signing keys are stored in the platform's encrypted secret store
- Key rotation must not invalidate active sessions (JWKS must serve both old and new keys during rotation)
- All OIDC endpoints must be served over TLS

---

## ADR-018: Product Event Bus as Only Inter-Product Interface

**Status:** Accepted

**Context:** Products need to react to events in other Products (e.g. "when a user is created in user-service, create a profile in photo-app"). Direct network calls between Products would couple them tightly and make the dependency graph opaque.

**Decision:** Products cannot make direct network calls to each other. The only interfaces between Products are: (1) events emitted to the platform event bus, and (2) SPARQL queries against an explicitly exposed product graph. Both are declared as resources.

**Rationale:**
- Enforces loose coupling at the platform level, not just by convention
- All inter-product dependencies are visible in resource files — the dependency graph is auditable
- Event-driven communication enables temporal decoupling — the subscribing Product does not need to be running when the event is emitted
- Consistent with the event-sourcing foundation of the platform

**Consequences:**
- Synchronous request-response between Products is not possible by design
- Cross-product data consistency is eventual, not immediate
- Teams building Products must design their domain events carefully — event schemas are a public API

---

## ADR-019: Per-Product SPARQL Endpoint and Ontology Exposure

**Status:** Accepted

**Context:** Products accumulate domain knowledge in their RDF stores. Other Products and operators need to query this knowledge. The schema of that knowledge needs to be discoverable without reading source code.

**Decision:** Every Product with an `rdf-store` resource gets an IAM-gated SPARQL 1.1 endpoint automatically. Every Product declares its ontology as a `.ttl` or `.shacl` file, which is bound to the Product version and served by the platform.

**Rationale:**
- SPARQL is the standard query language for RDF — no custom query API needed
- IAM-gating means SPARQL endpoints respect the same access control as all other resources
- Ontology files are the schema contract for a Product's graph — consumers can understand the domain before querying
- Binding ontology to Product version means consumers always know which schema they are querying

---

## ADR-020: Cluster Graph as Semantic Service Registry

**Status:** Accepted

**Context:** As the number of Products grows, operators need to discover what Products exist, what events they emit, what graphs they expose, and what ontologies they declare — without reading source files.

**Decision:** The cluster-level RDF graph is a semantic service registry. It contains all Products, their versions, their SPARQL endpoints, their subscribable event types, and their ontology declarations. All of this is queryable via SPARQL.

**Rationale:**
- The cluster is self-documenting by construction — no separate service catalog required
- LLMs can query the cluster graph to understand the deployed system before generating code
- New Products can discover existing Products' interfaces through graph queries
- Consistent with RDF as the universal data model for the platform

---

## ADR-021: One Active Version Per Product

**Status:** Accepted

**Context:** Products are versioned. A decision is needed on whether multiple versions can run simultaneously (for canary deployments, blue-green, etc.).

**Decision:** A Product has exactly one active version at any time. The version is part of the Product's identity. Multiple instances (implementations) of that version can run simultaneously, but they all run the same version.

**Rationale:**
- Eliminates version routing complexity — there is no traffic splitting, no canary percentage, no version-aware load balancing
- Simplifies the IAM model — Product-scoped tokens are always for the active version
- Ontology binding is unambiguous — there is always exactly one schema for a Product
- Consistent with the hermetic Product model — a Product is a well-defined, stable deployment unit

**Upgrade path:** Deploying a new Product version is an atomic cutover. The platform provisions all resources for the new version in full. Only when every resource reaches `ResourceReady` does the platform cut traffic over to the new version and tear down the old one. If any resource fails to reach `ResourceReady`, the deployment is aborted and the old version remains live. There is no partial cutover — the cluster is never in a state where two versions are simultaneously serving traffic.

---

## ADR-022: Inter-Product Event Subscriptions as First-Class Resources

**Status:** Accepted

**Context:** A Product that subscribes to another Product's events needs to declare that dependency somewhere. It could be implicit (subscribe at runtime) or explicit (declared as a resource).

**Decision:** Event subscriptions are declared as `event-subscription` resources in `.picloud` files. The platform provisions and manages the subscription lifecycle. Runtime subscriptions without a resource declaration are not permitted.

**Rationale:**
- All inter-product dependencies are visible in resource files — the dependency graph is auditable and version-controlled
- The platform can enforce that a subscription's source Product and event type exist before provisioning
- Consistent with the IaC-as-only-interface principle — everything exists in a file

---

## ADR-023: Ontology Files Bound to Product Version

**Status:** Accepted

**Context:** A Product's RDF graph has a schema. That schema may evolve as the Product evolves. Consumers need to know which schema they are querying.

**Decision:** Ontology files (`.ttl` or `.shacl`) are declared as `ontology` resources in the Product's resource file and bound to the Product version. The platform serves the ontology file from the cluster graph. When a new Product version is deployed, the ontology is updated atomically with the rest of the Product's resources.

**Rationale:**
- Schema and implementation are versioned together — no schema/implementation drift
- Consumers can discover the exact schema for any Product version from the cluster graph
- SHACL files provide validation shapes — the platform can optionally validate graph updates against them

---

## ADR-024: Storage Intent Model

**Status:** Accepted

**Context:** Products need storage with different characteristics. A write-intensive database needs different storage behaviour than a media archive. Traditional approaches require operators to specify replication factors and disk types directly.

**Decision:** Products declare storage intent semantically. The platform translates intent into implementation. Intent is declared as a durability tier and a performance tier on the `volume` resource.

**MVP durability tiers:**
- `full-replication` — replicated to all available nodes. Maximum durability. Only tier available in Phase 1.

**Future durability tiers (Phase 4):**
- `quorum` — replicated to majority of nodes
- `local` — single node, no replication
- `none` — ephemeral, lost on restart

**Future performance tiers (Phase 4):**
- `fast` — low-latency random read/write
- `standard` — balanced
- `archive` — sequential write optimised

**Rationale:**
- Operators express requirements, not implementation details — consistent with the cloud abstraction model
- Platform can make better placement decisions than operators (which nodes have capacity, which nodes are healthy)
- Adding new storage tiers in Phase 4 does not require changes to Product resource files — only the platform implementation changes

---

## ADR-025: Passkeys and FIDO2 as Sole Human Authentication Mechanism

**Status:** Accepted

**Context:** PiCloud is a full OIDC provider (ADR-017) and must authenticate human users. Traditional OIDC implementations use username and password. Passwords introduce credential storage risk, password reset complexity, and phishing vulnerability.

**Decision:** Human authentication uses passkeys (WebAuthn) and FIDO2 exclusively. There are no passwords in the platform. This applies to all human-facing flows: CLI authentication, platform administration, and application login via OIDC.

**Authentication modes:**
- **Browser-based** — WebAuthn ceremony via the platform's OIDC authorization endpoint. Works with any platform authenticator (Touch ID, Face ID, Windows Hello, hardware security key).
- **CLI device flow** — CLI initiates device authorization flow, operator completes passkey authentication in a browser on any device, CLI polls for token.
- **CLI FIDO2 direct** — for operators with a hardware security key, FIDO2 assertion completes directly in the terminal without a browser.

**Machine flows are unaffected:** App Registrations (OAuth client credentials) use client ID and client secret. mTLS certificates serve as workload identity credentials. Passkeys apply to human identities only.

**Rationale:**
- Eliminates password storage entirely — no credential database to breach
- Passkeys are phishing-resistant by construction — the credential is bound to the origin
- FIDO2 hardware key support means the platform works in fully headless, air-gapped environments
- Passkeys are now supported natively on all major platforms and browsers
- Consistent with a forward-looking security model — passwords are a solved problem we choose not to have

**Consequences:**
- The platform must implement the WebAuthn relying party correctly — ceremony initiation, challenge verification, authenticator registration
- Every human identity has one or more passkeys registered. Recovery uses a three-tier model: admin-initiated reset, enforced backup keys for admins, and physical node recovery as last resort (see ADR-026)
- Admin accounts are required to have a minimum of two passkeys registered — the platform enforces this constraint
- CLI device flow requires the platform to serve a browser-accessible enrollment page — this is the only browser-facing surface in Phase 1 CLI usage

**Rejected alternatives:**
- **Username and password** — credential storage risk, phishing risk, password reset complexity. Not acceptable.
- **SSH keys only** — suitable for CLI but does not cover browser-based OIDC flows for applications.
- **TOTP/OTP** — second factor only, still requires a primary credential. Adds complexity without eliminating passwords.

---

## ADR-026: Bootstrap Token Exchange and Three-Tier Passkey Recovery

**Status:** Accepted

**Context:** Two related problems require a consistent solution: (1) bootstrapping the first admin identity on a fresh cluster, and (2) recovering access when a passkey is lost. Both cases must be solvable without introducing passwords.

**Decision:** A single-use, time-limited token exchange mechanism handles both bootstrap and recovery. Three recovery tiers are defined in order of escalation:

**Bootstrap:** `picloud cluster init` generates a single-use bootstrap token with a 15-minute expiry. The operator opens the platform's enrollment endpoint in a browser and exchanges the token for a WebAuthn registration ceremony. Completing the ceremony creates the first admin identity. The token is invalidated immediately on use or expiry.

**Tier 1 — Admin-initiated reset:** An admin initiates a passkey reset for a user via `picloud identity reset-passkey {name}`. The platform generates a single-use re-enrollment token. The user registers a new authenticator via the enrollment endpoint. The previous passkey is revoked on successful re-enrollment.

**Tier 2 — Backup key enforcement:** Admin accounts must have a minimum of two passkeys registered. The platform enforces this — removing a passkey that would leave an admin with fewer than two is rejected. This ensures admins always have a fallback authenticator, typically a hardware security key stored offline.

**Tier 3 — Physical recovery:** If all admin accounts are inaccessible, an operator with physical access to any cluster node runs `picloud cluster recover` locally (non-network access only). This generates a new bootstrap token, identical in mechanism to the original `cluster init` flow. The recovery event is written to the platform event log as a high-severity audit entry.

**Rationale:**
- Every tier is password-free — recovery tokens are short-lived and single-use, not reusable credentials
- Physical recovery requires physical presence — an attacker cannot trigger recovery remotely
- Backup key enforcement ensures Tier 1 (admin reset) is always available as long as at least one admin is accessible
- The same token exchange mechanism is reused across bootstrap and all recovery tiers — one implementation, multiple use cases
- All recovery operations are auditable events in the platform event log

---

## ADR-027: mTLS for Workload-to-Platform and Direct Workload-to-SPARQL Communication

**Status:** Accepted

**Context:** Workloads need authenticated, encrypted communication with the platform event bus and with other Products' SPARQL endpoints. Two routing options exist: all traffic via platform, or direct connections where appropriate.

**Decision:** Two mTLS patterns are used:

1. **Workload → platform event bus** — routed via the platform. The platform enforces IAM on every event operation and maintains the full audit trail. Transport is mTLS with platform-issued workload certificates.

2. **Workload → product SPARQL endpoint** — direct connection from the querying workload to the target Product's endpoint over mTLS. The platform issues certificates to both parties at workload startup. IAM is enforced at the SPARQL endpoint by validating the caller's workload certificate against the platform's CA and checking the caller's permissions.

**Certificate lifecycle:** The platform's built-in CA issues certificates to workloads at runtime as part of workload startup. Certificates are short-lived and rotated automatically. Workloads never handle certificate generation — the platform injects them.

**Rationale:**
- Events are fire-and-forget — platform mediation adds audit trail and IAM enforcement with minimal latency cost
- SPARQL queries are request-response — the extra platform hop adds latency and creates a platform bottleneck for what could be a high-frequency read pattern
- Direct mTLS for SPARQL maintains security (mutual authentication, IAM at endpoint) without sacrificing performance
- All certificates are platform-issued — no external PKI, no operator certificate management

**Rejected alternatives:**
- **All traffic via platform** — creates a platform bottleneck for SPARQL queries. High-frequency graph reads would saturate the platform's routing layer.
- **Direct connections without mTLS** — unacceptable. All workload communication must be mutually authenticated and encrypted.

---

## ADR-028: Low Coupling, High Cohesion as a Structural Platform Constraint

**Status:** Accepted

**Context:** PiCloud's architecture makes many decisions that could be explained individually but share a common principle: the platform structurally prevents tight coupling between Products while ensuring each Product is internally cohesive. This principle is worth making explicit because it explains and justifies a large number of other decisions.

**Decision:** Low coupling and high cohesion are structural constraints enforced by the platform, not conventions left to developers. The platform's architecture makes tight coupling between Products impossible by construction.

**How the platform enforces low coupling:**
- Products cannot share resources — every resource belongs to exactly one Product (ADR-016)
- Direct network calls between Products are not routed by the platform — the only inter-product interfaces are the event bus and SPARQL endpoints
- Event subscriptions are declared resources — inter-product dependencies are explicit and auditable (ADR-022)
- The event bus and SPARQL graph are intentionally separate interfaces — events for temporal decoupling, graphs for read queries — preventing the conflation of communication patterns

**How the platform enables high cohesion:**
- A Product owns everything it needs — compute, storage, identity, graph, event bus, DNS, ontology
- No cross-product dependencies are implicit — all dependencies are declared in resource files
- The Product's ontology defines its domain boundary explicitly (ADR-023)

**Why this matters:**
- Teams building Products on PiCloud cannot accidentally couple their Products at the data layer
- The event log provides a complete audit of all inter-product communication
- Products can be deployed, updated, and deleted independently without affecting other Products
- The decoupling between the event bus (platform-routed) and SPARQL (direct mTLS) is a direct expression of this principle — different communication patterns have different coupling characteristics and are handled differently

**This principle is the architectural north star for PiCloud.** When a new feature or capability is being designed, the first question is: does this increase coupling between Products, or does it preserve their independence? If it increases coupling, the design should be reconsidered.

---

## ADR-030: Platform-Generated CA with BYO-CA Support

**Status:** Accepted

**Context:** All platform communication is TLS — node-to-node mTLS, workload certificates, and HTTPS for the IRI-based resource layer. A CA is required to issue these certificates. External clients need to trust this CA to connect to `picloud.local`.

**Decision:** On `picloud cluster init`, the platform generates its own root CA if none is specified. Operators may optionally provide an external CA (e.g. Smallstep, an existing corporate CA) via configuration. All certificate issuance, rotation, and revocation is managed by the platform regardless of which CA is used.

**Default behaviour — platform-generated CA:**
- `picloud cluster init` generates a root CA keypair, stored encrypted in the platform's secret store and replicated across nodes via Raft
- The CA certificate is exported via `picloud ca export` for installation into client OS trust stores
- Node certificates, workload certificates, and TLS certificates for `picloud.local` are all issued by this CA

**BYO-CA mode:**
- Operator provides a CA certificate and signing key (or an ACME/EST endpoint) in the bootstrap configuration
- The platform uses the provided CA for all certificate issuance
- Useful for integrating with an existing homelab PKI (e.g. Smallstep CA) or corporate PKI

**Rationale:**
- Zero-configuration default — the platform is fully operational without any external PKI
- BYO-CA means operators with existing trust infrastructure (Smallstep, internal CA) don't need to manage two PKIs or distribute a new CA certificate to all their devices
- All certificate lifecycle is platform-managed regardless of CA source — operators never manually issue or rotate certificates

**Consequences:**
- External clients (operator laptops, browsers, RDF tools) must trust the platform CA to connect to `picloud.local` over HTTPS — one-time operation via `picloud ca export`
- In BYO-CA mode, the external CA must be accessible during node join and certificate rotation operations
- The platform CA private key is the most sensitive secret in the cluster — its storage and replication must be treated with the highest security priority

**Status:** Accepted

**Context:** RDF is an HTTP-native technology. IRIs (Internationalized Resource Identifiers) are both the unique identifier and the dereferenceable location of every RDF resource. If the platform assigns opaque internal IDs to resources rather than IRIs, RDF tooling cannot navigate the graph by following links. The cluster graph becomes a closed system rather than a Linked Data platform.

**Decision:** Every resource in PiCloud has a canonical IRI rooted at the cluster domain (`picloud.local` by default). IRIs follow a path-based hierarchy that reflects the resource model. Every IRI is dereferenceable over HTTPS. The platform serves RDF representations at every resource IRI via HTTP content negotiation.

**IRI scheme — path-based (not subdomain-based):**
```
https://picloud.local/                                           # cluster root
https://picloud.local/nodes/{node-name}                         # node
https://picloud.local/products/{product-name}                   # product
https://picloud.local/products/{product-name}/{type}/{name}     # resource
https://picloud.local/products/{product-name}/graph             # SPARQL endpoint
https://picloud.local/products/{product-name}/ontology          # ontology
https://picloud.local/products/{product-name}/events            # event stream
```

**Content negotiation at every IRI:**
```
Accept: text/turtle            → Turtle RDF representation
Accept: application/ld+json    → JSON-LD representation
Accept: application/json       → Plain JSON representation
Accept: text/html              → Human-readable view (future portal)
```

**Why path-based over subdomain-based:**
- Aligned with Linked Data and REST conventions — the path hierarchy reflects the resource hierarchy
- Single TLS certificate per product scope — no wildcard certificates required
- IRIs are meaningful by inspection — the path encodes type and ownership
- Subdomain-per-resource would require a wildcard cert and would not convey hierarchy

**Rationale:**
- RDF tools, SPARQL clients, and LLMs can navigate the entire cluster by dereferencing IRIs and following links — the cluster is a Linked Data platform by construction
- The cluster root IRI returns a description of all Products and their IRI spaces — self-documenting without any additional service catalog
- DNS and HTTP are the lowest common denominator for interoperability — any client that speaks HTTP can interact with the platform
- IRI stability (resources keep their IRI when rescheduled) means RDF triples in external systems remain valid
- Content negotiation means the same IRI serves both machine consumers (Turtle, JSON-LD) and future human interfaces (HTML)

**Consequences:**
- The platform must run an HTTP server on every node serving the canonical IRI space
- The internal DNS resolver must resolve `picloud.local` to the cluster ingress
- TLS certificates must be issued for `picloud.local` by the platform's built-in CA — external clients need to trust this CA
- Resource IRIs must be assigned at declaration time and remain stable for the lifetime of the resource

---

## ADR-031: Event Schema Versioning via Schema IRIs

**Status:** Accepted

**Context:** The event log is append-only and permanent. As the platform evolves, event schemas will change — fields are added, renamed, restructured. Projectors must be able to interpret events written under any past schema. Schema evolution must be explicit, auditable, and consistent with the platform's IRI-everything model.

**Decision:** Every event carries a `schema` field containing the IRI of its schema definition. Schema IRIs are versioned and permanently dereferenceable. Projectors resolve the schema IRI to understand the event payload. Old schema IRIs are never removed — they resolve forever.

**Event envelope:**
```json
{
  "id": "uuid",
  "schema": "https://picloud.local/schemas/events/ResourceReady/v2",
  "type": "ResourceReady",
  "timestamp": "2025-01-01T00:00:00Z",
  "source": "https://picloud.local/products/photo-app",
  "payload": { ... }
}
```

**Schema resources:**
Schema definitions are served by the platform at their canonical IRI with HTTP content negotiation:
```
https://picloud.local/schemas/events/ResourceReady/v1   # original schema
https://picloud.local/schemas/events/ResourceReady/v2   # updated schema
```

Each schema IRI returns a JSON Schema or SHACL document describing the event payload structure. The platform maintains all schema versions in its RDF store — they are first-class resources, not documentation.

**Evolution rules:**
- Adding fields to a payload is always backwards-compatible — increment the minor version
- Renaming, removing, or restructuring fields requires a new major version IRI
- Projectors register handlers by schema IRI — a projector that handles `v1` and `v2` has two explicit handlers, each correct for its version
- The platform ships migration utilities for common projector patterns

**Rationale:**
- Schema IRIs are dereferenceable resources — any LLM, RDF tool, or projector can fetch the schema and understand any event without out-of-band documentation
- Consistent with the IRI-everything model (ADR-029) — schemas are part of the cluster's Linked Data surface
- Schema versioning is explicit in the event log — every event permanently records which schema it was written under
- Old schema IRIs resolve forever — the log remains fully interpretable at any point in the future without consulting external documentation
- An LLM building a new projector can fetch the schema IRI directly and generate a correct handler without needing the platform source code

**Consequences:**
- The platform must serve schema IRIs as part of its HTTP layer from Phase 1 — schema IRIs appear in the first events emitted
- Schema definitions must be written before the events that use them — schemas are deployed as part of platform releases
- Projectors accumulate handlers over time as schemas evolve — this is intentional and explicit rather than hidden

---

## ADR-032: Product Event Store as First-Class Storage Primitive

**Status:** Accepted

**Context:** Event sourcing is the platform's internal state model. Products built on PiCloud face the same state management challenges — they need durable, replayable, auditable state. Without platform support, every Product team would implement event sourcing independently, with inconsistent quality and no integration with the platform's RDF projection layer.

**Decision:** The platform exposes its event sourcing infrastructure as a managed `event-store` resource for Products. A Product declares aggregates and their event schemas. The platform provisions a replicated event log, manages aggregate streams, serves schema IRIs, and automatically projects aggregate events into the Product's RDF store.

**Resource model:**
```bicep
event-store 'photos' = {
  product: 'photo-app'
  aggregates: [
    { type: 'Photo', schema: 'schemas/photo-events.ttl' }
    { type: 'Album', schema: 'schemas/album-events.ttl' }
  ]
}
```

**Platform provisions:**
- Replicated event log scoped to the Product (same Raft-replicated infrastructure as platform log)
- Addressable aggregate streams: `https://picloud.local/products/{name}/event-store/{store}/{Type}/{id}/events`
- Schema IRIs permanently served from the platform HTTP layer
- Automatic RDF projection of aggregate events into the Product's Oxigraph named graph
- IAM-gated HTTP API for appending and reading events

**Schema contract:**
Event schemas are declared as `.ttl` or `.shacl` files deployed with the Product and bound to its version. Schemas are immutable within a Product version. Changing a schema requires a Product version bump. All past schema IRIs are served permanently — the event log remains interpretable forever.

**Rationale:**
- Products get event sourcing + RDF projection without implementing any infrastructure
- The platform eats its own cooking — Product event stores use the same mechanisms as platform state
- Schema IRIs are consistent with ADR-031 — one versioning model for all events, platform and Product alike
- Automatic RDF projection means the Product's SPARQL endpoint reflects aggregate state immediately — no custom projectors needed for standard cases
- The HTTP API is consistent with the IRI model (ADR-029) — no custom protocol

**Consequences:**
- The platform must support multi-tenant event log partitioning — platform events and Product events coexist but are scoped separately
- Custom projectors (for non-standard projection logic) are a future concern — Phase 3 ships automatic projection only
- Product event stores add to the Raft replication load — large, high-frequency event stores may require tuning

---

## ADR-033: Generated Multi-Language SDKs Published to Package Registries

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

---

## ADR-034: Vertical Slice Architecture with Stable Domain Dependency

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

**Consequences:**
- New shared types must go in `picloud-domain` — this is the right place for them
- Slices communicate via injected trait implementations, not direct calls
- The composition root in `src/main.rs` grows as slices are added — this is expected and correct
- LLMs can be given a single slice plus `picloud-domain` as context and make meaningful progress without understanding the full platform

---

## ADR-035: Event Replay as First-Class Platform and Product Capability

**Status:** Accepted

**Context:** The event log is append-only, permanent, and Raft-replicated. Every state change — platform and product — flows through it. This means the RDF graph (the read model) can always be rebuilt by re-running projectors against the log. However, replay is only useful if it is a deliberate, observable, controllable operation — not a recovery mechanism that operators have to engineer themselves.

Two categories of failure motivate this:
- **Projector bugs** — a bug in a projector writes incorrect triples into the RDF graph. The fix is deployed in a new Product version. The corrected projector must be run against historical events to repair the graph.
- **Subscriber inconsistency** — a downstream Product's projection has drifted because events were missed or misprocessed. Replaying the source Product's events from a known-good point re-establishes consistency.

**Decision:** Replay is a first-class capability available to both the platform itself and to every Product. It is accessible via the CLI and the SDK/HTTP API.

### Replay model

Replay reads events from the log between a `from` and optional `to` timestamp, re-runs them through the **currently deployed version's projectors**, and re-emits them to all active event subscribers. Replay always uses the current projectors — never the projectors from the version that originally emitted the events. This is the mechanism by which bugs in previous projector versions are corrected (see ADR-031 — schema IRIs ensure current projectors can interpret any historical event payload).

### Replay scope

Three scopes are supported:

**Platform replay** — replays the platform event log. Rebuilds the cluster-level RDF graph. Used when platform projector bugs corrupt cluster state.

```bash
picloud cluster replay --from "2025-06-01T00:00:00Z"
picloud cluster replay --from "2025-06-01T00:00:00Z" --to "2025-06-02T00:00:00Z"
```

**Product replay** — replays all events in a Product's event store. Rebuilds the Product's RDF graph.

```bash
picloud resource replay photo-app --from "2025-06-01T00:00:00Z"
```

**Aggregate replay** — replays one or more specific aggregates. Supports a single aggregate, a list, or a batch of up to N aggregates.

```bash
# Single aggregate
picloud resource replay photo-app \
  --aggregate Photo --id 123e4567-e89b-12d3-a456-426614174000 \
  --from "2025-06-01T00:00:00Z"

# Batch — up to 1000 aggregate IDs from a file
picloud resource replay photo-app \
  --aggregate Photo --ids-file ./photo-ids.txt \
  --from "2025-06-01T00:00:00Z"
```

### Replay always serves live traffic

The platform continues serving the current RDF graph during replay. The new projection is built in a **shadow graph** — a separate named graph in Oxigraph scoped to the replay operation. When the shadow projection reaches the `to` timestamp (or the present if no `to` is given), it is validated and atomically swapped with the live graph. The swap is itself an event in the log.

This is consistent with the atomic cutover model for Product upgrades (ADR-021) — state transitions are always atomic, never partial.

### Marked replay events

Replayed events are distinguishable from live events. Every replayed event envelope carries two additional fields:

```json
{
  "id": "uuid",
  "schema": "https://picloud.local/schemas/events/PhotoCreated/v1",
  "type": "PhotoCreated",
  "timestamp": "2025-06-01T10:00:00Z",
  "replay": {
    "is_replay": true,
    "replay_id": "uuid",
    "original_timestamp": "2025-06-01T10:00:00Z",
    "replayed_at": "2025-07-01T09:00:00Z"
  },
  ...
}
```

`replay_id` groups all events from a single replay operation. `original_timestamp` is when the event was first written. `replayed_at` is when it was re-emitted.

Subscribers receive replayed events on the same channels as live events. The `replay` field allows subscribers to make explicit decisions — for example, skipping email sends or payment charges on replay while still updating their RDF projections. Subscribers that are fully idempotent via the event `id` field require no changes — the platform deduplicates automatically.

**Platform contract:** all event subscribers should be idempotent by default. The `replay` field is additional information, not a crutch for non-idempotent implementations.

### Replay via SDK and HTTP API

Replay is available programmatically so Products can trigger self-healing workflows:

```
POST https://picloud.local/products/photo-app/event-store/photos/replay
{
  "from": "2025-06-01T00:00:00Z",
  "aggregate_type": "Photo",
  "aggregate_ids": ["uuid-1", "uuid-2", ...],  // omit for full store replay
  "to": "2025-06-02T00:00:00Z"                 // omit for replay to present
}
```

Returns a `replay_id`. The replay operation emits a `ReplayStarted` event and a `ReplayCompleted` or `ReplayFailed` terminal event — subscribable via the standard event stream.

### Replay lifecycle events

```
ReplayRequested   — operator or API triggered a replay
ReplayStarted     — shadow projection is building
ReplayProgress    — periodic progress (events processed / total)
ReplayCompleted   — shadow graph swapped with live graph
ReplayFailed      — replay aborted, live graph unchanged, reason attached
```

All replay events are written to the platform log and projected into the cluster RDF graph. A replay operation is fully auditable.

**Rationale:**
- Replay is the correctness guarantee of event sourcing — without it, a projector bug is permanent damage rather than a recoverable state
- Shadow projection with atomic swap means replay never degrades live service
- Marked replay events give subscribers the information to make correct decisions without mandating a specific behaviour
- Using current projectors against historical events (via schema IRIs) is the mechanism by which bugs are fixed retroactively — this is the core value of the ADR-031 schema versioning decision
- Batch aggregate replay (up to 1000) covers the common operational case of targeted repair without requiring a full store replay
- CLI, HTTP API, and SDK access means replay can be scripted, automated, or triggered by monitoring systems

**Consequences:**
- The shadow graph mechanism requires Oxigraph to support multiple named graphs simultaneously — it does (ADR-006)
- Batch replay of 1000 aggregates is a resource-intensive operation — the platform should enforce concurrency limits (one active replay per Product at a time)
- `ReplayProgress` events should be emitted frequently enough to be useful but not so frequently that they flood the event log — every 100 events processed is a reasonable default
- Subscribers that perform irreversible side effects (email, payment, external API calls) must inspect the `replay.is_replay` field — this should be documented prominently in the SDK

---

## ADR-036: Universal Tagging System

**Status:** Accepted

**Context:** Resources across the platform — nodes, products, users, groups, containers, volumes — need a lightweight, flexible labelling mechanism. Tags are the primary input to SPARQL CONSTRUCT inference rules (ADR-038), particularly for IAM group membership rules (ADR-037). Tags must be queryable in the RDF graph and travel through the event log.

**Decision:** Every platform resource supports an arbitrary set of tags. A tag is a `key:value` string pair. Tags are declared in resource definition files and can be added or removed via CLI and API. Tag changes are events — `TagAdded` and `TagRemoved` — projected into the RDF graph immediately.

**Tag syntax:**
```bicep
container 'api-server' = {
  product: 'photo-app'
  image: 'photo-api:1.0.0'
  tags: {
    'team': 'backend'
    'environment': 'production'
    'tier': 'api'
  }
}
```

**RDF representation:**
```turtle
<https://picloud.local/products/photo-app/containers/api-server>
    picloud:tag [
        picloud:tagKey "team" ;
        picloud:tagValue "backend"
    ] ;
    picloud:tag [
        picloud:tagKey "environment" ;
        picloud:tagValue "production"
    ] .
```

**CLI:**
```bash
picloud tag add photo-app/containers/api-server team=backend
picloud tag remove photo-app/containers/api-server team=backend
picloud tag list photo-app/containers/api-server
picloud tag find environment=production          # all resources with this tag
```

**Rationale:**
- Tags are a universal primitive — one mechanism for labelling any resource type
- RDF representation makes tags immediately queryable via SPARQL across all resource types
- Event-driven — `TagAdded`/`TagRemoved` trigger inference rule evaluation instantly (ADR-037, ADR-038)
- Key:value pairs are the simplest model that supports meaningful inference patterns

**Consequences:**
- `Tag` becomes a domain type in `picloud-domain` used by all resource types
- Tag events must be emitted whenever tags change, including on initial resource creation
- Tag keys should be namespaced by convention (`team:`, `environment:`, `tier:`) to avoid collisions — enforced by documentation, not by the platform

---

## ADR-037: Groups as IAM Resource with SPARQL CONSTRUCT Membership Rules

**Status:** Accepted

**Context:** Managing individual user role assignments does not scale. When a new team member joins, an operator should not need to manually assign every role. Groups provide a level of indirection — assign roles to a group, users inherit them. Membership should be automatic where possible, driven by tags and inference rules rather than manual assignment.

**Decision:** `Group` is a new IAM resource. A group has roles assigned to it. Users in a group inherit all roles assigned to that group. Group membership is managed via SPARQL CONSTRUCT rules that evaluate on every relevant event and on a 10-minute reconciliation schedule.

**Group resource:**
```bicep
group 'backend-developers' = {
  description: 'Backend engineering team'
  roles: ['product-developer', 'log-viewer']
  tags: {
    'team': 'backend'
  }
}
```

**Membership rule resource:**
```bicep
inference-rule 'backend-group-membership' = {
  description: 'Add users tagged team:backend to backend-developers group'
  scope: 'platform'
  trigger: 'event'             // run on TagAdded, TagRemoved, IdentityCreated
  reconciliation: true         // also run every 10 minutes
  construct: '''
    CONSTRUCT {
      <https://picloud.local/groups/backend-developers>
          picloud:hasMember ?user .
    }
    WHERE {
      ?user a picloud:HumanIdentity ;
            picloud:tag [
                picloud:tagKey "team" ;
                picloud:tagValue "backend"
            ] .
    }
  '''
}
```

**How membership works:**
1. A `TagAdded` event fires (user gets tag `team:backend`)
2. The inference engine evaluates all rules triggered by `TagAdded`
3. CONSTRUCT query runs — produces `picloud:hasMember` triples
4. New triples are written to the platform graph
5. A `GroupMembershipChanged` event is emitted
6. IAM token issuance reads group memberships from the graph — next token the user receives includes the inherited roles

**Removal:** When a tag is removed, the CONSTRUCT query no longer produces the membership triple. The inference engine detects the retraction and emits `GroupMembershipChanged`. The triple is removed from the graph.

**RDF representation:**
```turtle
<https://picloud.local/groups/backend-developers>
    a picloud:Group ;
    picloud:hasRole <https://picloud.local/platform/roles/product-developer> ;
    picloud:hasRole <https://picloud.local/platform/roles/log-viewer> ;
    picloud:hasMember <https://picloud.local/platform/identities/alice> ;
    picloud:hasMember <https://picloud.local/platform/identities/bob> .
```

**Rationale:**
- Groups decouple role assignment from individual users — one group change affects all members
- SPARQL CONSTRUCT rules make membership declarative and auditable — the rule is a resource in the graph
- Event-driven evaluation gives immediate effect — a tag change cascades to group membership to token permissions within one event cycle
- 10-minute reconciliation catches any drift between events
- The graph is always the source of truth — no separate membership database

**Consequences:**
- Token issuance in `picloud-iam` must read group memberships from the RDF graph before assembling claims
- `GroupMembershipChanged` must be a platform event so downstream systems can react
- A user can be in multiple groups — role sets are additive
- Circular group membership (group A contains group B contains group A) must be detected and rejected

---

## ADR-038: SPARQL CONSTRUCT Inference Rules as Platform Resources

**Status:** Accepted

**Context:** PiCloud needs a mechanism to derive new knowledge from existing graph state — for IAM group membership (ADR-037), for operational alerts, and for any future rule-based automation. The mechanism must be declarative, auditable, version-controlled, and consistent with the platform's resource model.

**Decision:** SPARQL CONSTRUCT queries are a first-class resource type (`inference-rule`). Rules are declared in `.picloud` files, deployed with the platform or a product, stored in the RDF graph, and evaluated by the platform's inference engine. Rules produce triples that are written back to the graph. When produced triples are new (assertions) or removed (retractions), the engine emits events.

**Rule resource:**
```bicep
inference-rule 'high-memory-alert' = {
  description: 'Alert when node memory exceeds 85%'
  scope: 'platform'                    // 'platform' or product name
  trigger: 'event'                     // evaluate on matching events
  trigger-events: ['MetricRecorded']   // which events trigger evaluation
  reconciliation: true                 // also run on 10-minute schedule
  construct: '''
    CONSTRUCT {
      ?node a picloud:Alert ;
            picloud:alertType "HighMemoryUsage" ;
            picloud:alertSeverity "warning" ;
            picloud:alertMessage "Node memory above 85%" ;
            picloud:alertResource ?node ;
            picloud:alertTimestamp ?now .
    }
    WHERE {
      ?node a picloud:Node ;
            picloud:memoryUsedMb ?used ;
            picloud:memoryTotalMb ?total .
      BIND(NOW() AS ?now)
      FILTER(?used / ?total > 0.85)
    }
  '''
}
```

**Evaluation model:**
1. A triggering event arrives (e.g. `MetricRecorded`)
2. The engine identifies all rules triggered by that event type
3. Each rule's CONSTRUCT query runs against the current graph
4. New triples are asserted — retracted triples are removed
5. For each **new** `picloud:Alert` triple: an `AlertFired` event is emitted
6. For each **removed** `picloud:Alert` triple: an `AlertResolved` event is emitted
7. For each **new** `picloud:hasMember` triple: a `GroupMembershipChanged` event is emitted
8. All produced triples are projected into the appropriate named graph

**Reconciliation pass:**
Every 10 minutes, all rules with `reconciliation: true` are evaluated regardless of events. This catches any drift — for example, a rule that should have fired but did not because an event was missed during a node restart. The reconciliation pass is itself an event (`ReconciliationCompleted`) in the platform log.

**Rule scoping:**
- `scope: 'platform'` — rule runs against the cluster-level graph, available to platform operators only
- `scope: 'photo-app'` — rule runs against the product's named graph, available to the product's IAM scope

**Rationale:**
- SPARQL CONSTRUCT is the natural fit — rules are graph pattern queries that produce graph facts
- Rules are resources — versioned, auditable, IRI-addressable, deployed via `picloud resource apply`
- Event-driven evaluation gives fast cascading effects across the cluster
- 10-minute reconciliation is the safety net — eventual consistency with a bounded staleness window
- Scoping means products can define their own inference rules without platform operator involvement
- Alert lifecycle (fired/resolved) as events means any product can subscribe and build notification workflows

**Consequences:**
- The inference engine needs to track which triples were produced by which rule to detect retractions
- Rule evaluation must be idempotent — running the same rule twice produces the same triples
- Expensive CONSTRUCT queries on large graphs must be bounded — rule authors should use graph scoping and LIMIT where appropriate
- One active reconciliation pass at a time — concurrent passes are not permitted

---

## ADR-039: Embedded RDFS/OWL Inference via Oxigraph

**Status:** Accepted

**Context:** Beyond SPARQL CONSTRUCT rules (ADR-038), structural knowledge about the platform's ontology should be automatically materialised. Class hierarchies, property inheritance, and equivalences declared in `.ttl` ontology files should produce inferred triples without requiring explicit CONSTRUCT rules for each case.

**Decision:** Oxigraph's built-in RDFS inference is enabled on the platform graph and all product named graphs. OWL 2 RL axioms declared in ontology files are automatically applied. Inferred triples are materialised alongside asserted triples and are queryable via SPARQL.

**What this gives for free:**

*RDFS subclass inference:*
```turtle
picloud:ProductionContainer rdfs:subClassOf picloud:Container .
```
Any SPARQL query for `picloud:Container` automatically includes `picloud:ProductionContainer` instances — no CONSTRUCT rule needed.

*OWL property transitivity:*
```turtle
picloud:dependsOn rdf:type owl:TransitiveProperty .
```
If `photo-app dependsOn user-service` and `user-service dependsOn auth-service`, the reasoner infers `photo-app dependsOn auth-service`.

*Ontology-driven IAM:*
```turtle
picloud:AdminRole rdfs:subClassOf picloud:OperatorRole .
```
Any permission check for `picloud:OperatorRole` automatically applies to admins.

**Scope:** RDFS inference + OWL 2 RL profile. Full OWL DL reasoning is explicitly out of scope — it is computationally intractable for a live platform.

**Rationale:**
- Zero additional infrastructure — Oxigraph handles this natively (ADR-006)
- Ontology files already deployed with products (ADR-023) — RDFS/OWL axioms are declared there
- Structural inference is always live — no schedule, no trigger, no rule to maintain
- Complements ADR-038 — RDFS/OWL handles structural facts, CONSTRUCT handles operational rules

**Consequences:**
- Product ontology authors must understand RDFS/OWL 2 RL — this is documented in the SDK
- Inferred triples increase graph size — Oxigraph's materialisation must be monitored
- Ontology changes (new subclass declarations) take effect immediately on deployment

---

## ADR-040: Platform Metrics Agent — Hardware Telemetry as Events

**Status:** Accepted

**Context:** Operational alert rules (ADR-038) need hardware metrics — CPU usage, memory usage, disk usage, and CPU temperature — to be present in the RDF graph so SPARQL CONSTRUCT queries can reason over them. These metrics do not arrive naturally as resource lifecycle events. A collection mechanism is needed that is consistent with the platform's event-sourcing model.

**Decision:** Every node runs a platform metrics agent as a built-in capability (not a separate process — it is part of the `picloud-server` binary). The agent samples hardware metrics on a configurable interval (default: 15 seconds) and emits `MetricRecorded` events to the platform event log. These events are projected into the cluster RDF graph as time-stamped metric triples on each node's IRI.

**Metrics collected per node:**
- CPU usage (%) — per core and aggregate
- Memory used / total (MB)
- Disk used / total / read rate / write rate — per NVMe device
- CPU temperature (°C)
- Network bytes in/out per interface

**Event shape:**
```json
{
  "schema": "https://picloud.local/schemas/events/MetricRecorded/v1",
  "type": "MetricRecorded",
  "source": "https://picloud.local/nodes/pi-node-01",
  "payload": {
    "node_iri": "https://picloud.local/nodes/pi-node-01",
    "metrics": [
      { "name": "cpu_usage_percent",     "value": 42.3, "unit": "percent" },
      { "name": "memory_used_mb",        "value": 8192, "unit": "mb" },
      { "name": "memory_total_mb",       "value": 16384, "unit": "mb" },
      { "name": "disk_used_gb",          "value": 312,  "unit": "gb" },
      { "name": "disk_total_gb",         "value": 1000, "unit": "gb" },
      { "name": "cpu_temp_celsius",      "value": 58.1, "unit": "celsius" }
    ]
  }
}
```

**RDF projection — latest value only:**
The projector writes the latest metric values as triples on the node IRI, overwriting previous values. Historical values live in the event log — the graph holds only the current state:
```turtle
<https://picloud.local/nodes/pi-node-01>
    picloud:cpuUsagePercent 42.3 ;
    picloud:memoryUsedMb 8192 ;
    picloud:memoryTotalMb 16384 ;
    picloud:diskUsedGb 312 ;
    picloud:diskTotalGb 1000 ;
    picloud:cpuTempCelsius 58.1 ;
    picloud:metricsUpdatedAt "2025-07-01T12:00:00Z"^^xsd:dateTime .
```

**Product metrics:**
Workloads emit domain metrics (request count, error rate, latency) as events to the product event bus. The platform does not collect these — workloads are responsible for emitting them. The SDK provides helpers for common metric event shapes.

**Rationale:**
- Built into `picloud-server` — no separate agent process, consistent with single-binary model
- Events are the collection mechanism — metrics flow through the same infrastructure as all other platform state
- Latest-value-only projection keeps the graph lean — historical analysis uses event log replay
- 15-second default interval is sufficient for alert rules while not flooding the event log
- `MetricRecorded` events trigger inference rule evaluation (ADR-038) — alert rules fire within seconds of a threshold breach

**Consequences:**
- At 15-second intervals across 5 nodes, `MetricRecorded` generates ~20 events/minute — well within Raft throughput
- The metrics collection interval is configurable per deployment
- Temperature collection requires reading `/sys/class/thermal/` — Linux-specific, consistent with target platform (ADR-004)
- Metric projection overwrites previous triples — the projector must handle this correctly (upsert, not append)

---

## ADR-041: Alert Rules as SPARQL CONSTRUCT Queries with AlertFired Events

**Status:** Accepted

**Context:** Operators need to know when something is wrong — a node is overheating, a product's error rate is spiking, memory is exhausted. Alerts must be declarative, auditable, and consistent with the platform's event model. Alert lifecycle (fired, resolved) must be observable by any subscriber.

**Decision:** Alerts are produced by SPARQL CONSTRUCT rules (ADR-038) that match `picloud:Alert` typed triples. The inference engine detects when alert triples are asserted or retracted and emits `AlertFired` and `AlertResolved` events respectively. No built-in notification targets — alerts are events, and products built on PiCloud handle delivery.

**Alert triple shape (produced by CONSTRUCT rules):**
```turtle
_:alert a picloud:Alert ;
    picloud:alertType "HighCpuTemperature" ;
    picloud:alertSeverity "critical" ;           // info | warning | critical
    picloud:alertMessage "CPU temperature above 80°C on pi-node-02" ;
    picloud:alertResource <https://picloud.local/nodes/pi-node-02> ;
    picloud:alertTimestamp "2025-07-01T12:00:00Z"^^xsd:dateTime .
```

**Built-in platform alert rules (shipped with the platform):**

| Rule | Threshold | Severity |
|---|---|---|
| High CPU temperature | > 80°C | critical |
| High CPU temperature | > 70°C | warning |
| High memory usage | > 90% | critical |
| High memory usage | > 80% | warning |
| High disk usage | > 90% | critical |
| Node unreachable | Raft heartbeat missed | critical |
| Product workload failed | `ResourceStatus = Failed` | critical |

**Custom alert rules** are declared as `inference-rule` resources in product or platform `.picloud` files. Any CONSTRUCT query that produces `picloud:Alert` triples is an alert rule.

**Example — product request error rate alert:**
```bicep
inference-rule 'high-error-rate' = {
  scope: 'photo-app'
  trigger: 'event'
  trigger-events: ['MetricRecorded']
  construct: '''
    CONSTRUCT {
      ?product a picloud:Alert ;
               picloud:alertType "HighErrorRate" ;
               picloud:alertSeverity "warning" ;
               picloud:alertMessage "Error rate above 5%" ;
               picloud:alertResource ?product .
    }
    WHERE {
      ?product a picloud:Product ;
               picloud:errorRatePercent ?rate .
      FILTER(?rate > 5.0)
    }
  '''
}
```

**AlertFired event shape:**
```json
{
  "type": "AlertFired",
  "payload": {
    "alert_type": "HighCpuTemperature",
    "severity": "critical",
    "message": "CPU temperature above 80°C on pi-node-02",
    "resource_iri": "https://picloud.local/nodes/pi-node-02",
    "rule_iri": "https://picloud.local/inference-rules/high-cpu-temp-critical",
    "fired_at": "2025-07-01T12:00:00Z"
  }
}
```

**AlertResolved event** is emitted when the alert triple is retracted — i.e. the CONSTRUCT query no longer matches. Resolution is automatic and event-driven.

**Querying active alerts:**
```sparql
SELECT ?resource ?type ?severity ?message ?timestamp
WHERE {
  ?alert a picloud:Alert ;
         picloud:alertResource ?resource ;
         picloud:alertType ?type ;
         picloud:alertSeverity ?severity ;
         picloud:alertMessage ?message ;
         picloud:alertTimestamp ?timestamp .
}
ORDER BY DESC(?timestamp)
```

**Rationale:**
- Alert rules are resources — versioned, auditable, deployed via `picloud resource apply`
- `AlertFired` and `AlertResolved` as events means any product can subscribe and build notification, escalation, or auto-remediation workflows
- No built-in notification targets — consistent with the platform's composability philosophy (ADR-018). A notification product built on PiCloud handles Slack, email, PagerDuty etc.
- Active alerts are queryable from the RDF graph at any time — `picloud graph query` gives the current alert state
- Alert resolution is automatic — when the condition clears, the event fires. No manual acknowledgement needed (though products can implement that on top)

**Consequences:**
- The inference engine must efficiently diff produced triples between evaluations to detect assertions and retractions
- Alert storms (rapid fire/resolve cycles) should be dampened — a minimum 60-second hold-off before re-firing the same alert on the same resource
- Built-in platform alert rules are shipped as `.ttl` files in the platform binary and loaded at startup
- `picloud:Alert` becomes a well-known class in the platform ontology — documented in the SDK

---

## ADR-042: Tenant Identity — Domain and Cluster ID as Dual Boundary

**Status:** Accepted

**Context:** PiCloud is designed to run as a single tenant by default (`picloud.local`) but must support multiple isolated tenants — either on the same network or across different networks. Two clusters on the same local network must not accidentally merge, even if misconfigured. The tenant boundary must be both human-readable and cryptographically enforced.

**Decision:** Every PiCloud cluster has two identifiers established at `cluster init`:

1. **Cluster domain** — the human-readable tenant identity. Defaults to `picloud.local`. Configurable at init time. All IRIs, mDNS advertisements, and TLS certificates are scoped to this domain.

2. **Cluster ID** — a UUID generated at `cluster init` and stored in the cluster's Raft state. Cryptographically bound to the cluster CA. All node-join bootstrap tokens are signed by the cluster CA and carry the cluster ID. A node cannot join a cluster unless its bootstrap token was issued by that cluster's CA.

**The dual boundary:**
- The domain prevents accidental mDNS cross-discovery — a node advertising `company-a.local` is invisible to a node listening for `company-b.local`
- The cluster ID + CA prevents deliberate or accidental cross-join — even if two clusters share a domain name, a node cannot join without a valid bootstrap token from that cluster's CA

**Installation:**
```bash
# Default tenant
picloud cluster init

# Named tenant
picloud cluster init --domain acme.local

# Custom domain (external CA, BYO-CA mode — ADR-030)
picloud cluster init --domain cloud.acme.com --ca-cert ./acme-ca.pem
```

**Cluster identity stored in Raft:**
```rust
pub struct ClusterIdentity {
    pub cluster_id: Uuid,
    pub domain: ClusterDomain,
    pub created_at: DateTime<Utc>,
    /// Fingerprint of the cluster CA — all node certs must chain to this
    pub ca_fingerprint: String,
}
```

**mDNS scoping:**
Nodes advertise their cluster domain as the mDNS service type. Discovery filters strictly by service type — a node only responds to discovery from peers advertising the same domain. Two clusters on the same network are mutually invisible.

**Node join validation:**
When a node attempts to join:
1. It presents a bootstrap token
2. The cluster leader verifies the token was signed by the cluster CA (CA fingerprint match)
3. The leader verifies the cluster ID in the token matches the cluster's own cluster ID
4. Only then is the node admitted to Raft

A node that passes mDNS discovery but fails token validation is rejected and logged as a `NodeJoinRejected` event.

**IRI namespace:**
The cluster domain is the root of the IRI namespace. Every resource IRI is scoped to the domain, which is scoped to the cluster:
```
https://picloud.local/...     ← default tenant
https://acme.local/...        ← named tenant
https://cloud.acme.com/...    ← custom domain tenant
```

**Future multi-tenancy:**
When running multiple tenants, each cluster is fully independent — separate event log, separate RDF graph, separate IAM, separate storage pool. There is no cross-tenant resource sharing. This is consistent with ADR-028 (low coupling) applied at the cluster level.

**Rationale:**
- Domain alone is insufficient — two operators could accidentally use the same `.local` name on the same network and partially merge clusters
- Cluster ID alone is insufficient — it is not human-readable, making operations error-prone
- The dual boundary provides defence in depth: human-readable discrimination via domain, cryptographic enforcement via cluster CA
- Defaulting to `picloud.local` means zero configuration for the common single-tenant home lab case
- The cluster identity is established at `cluster init` and never changes — it is permanent for the lifetime of the cluster

**Consequences:**
- `ClusterIdentity` must be the first thing written to Raft state on `cluster init` — before any other operation
- The cluster domain must be embedded in the cluster CA certificate (SAN field) — this is how mTLS clients verify they are talking to the right cluster
- Changing a cluster's domain after init is not supported — the domain is part of the cluster's cryptographic identity
- `picloud cluster init` output must clearly display the cluster ID, domain, and CA fingerprint so operators can verify they are managing the right cluster

---

## ADR-043: Product Configuration Store

**Status:** Accepted

**Context:** Applications need runtime configuration — connection strings, feature endpoints, tuning parameters — that should not be baked into container images or resource definition files. Azure App Configuration solves this with a central, tagged key-value store. PiCloud needs the same capability, consistent with its event-driven and RDF-native model.

**Decision:** Every Product has a managed configuration store. Configuration entries are typed key-value pairs with tags. Workloads can declare their own configuration that merges over the product config — workload values win on conflict. Configuration changes emit `ConfigChanged` events. Workloads receive live updates via event subscription without restarting.

**Configuration resource:**
```bicep
config 'app-config' = {
  product: 'photo-app'
  entries: [
    { key: 'storage.max-upload-mb',  value: '50',                    type: 'int',    tags: { tier: 'storage' } }
    { key: 'api.base-url',           value: 'https://api.acme.local', type: 'string', tags: { tier: 'network' } }
    { key: 'cache.ttl-seconds',      value: '300',                   type: 'int',    tags: { tier: 'cache'   } }
    { key: 'feature.maintenance',    value: 'false',                  type: 'bool',   tags: { tier: 'ops'     } }
  ]
}
```

**Workload config override:**
```bicep
container 'worker' = {
  product: 'photo-app'
  image:   'photo-worker:1.0.0'
  config: {
    // Overrides product-level value for this workload only
    'cache.ttl-seconds': '60'
  }
}
```

**Effective config resolution — merge, workload wins:**
```
effective_config = product_config ∪ workload_config
                   (workload values override on key collision)
```

**Value types (Phase 1: flat strings. Types are metadata for SDK deserialisation):**

| Type | Description |
|---|---|
| `string` | Raw string value |
| `int` | Integer — SDK deserialises to i64 |
| `float` | Floating point — SDK deserialises to f64 |
| `bool` | Boolean — `"true"` / `"false"` |
| `json` | JSON string — SDK deserialises to typed object (future) |

**HTTP API:**
```
GET  https://picloud.local/products/photo-app/config              → all entries
GET  https://picloud.local/products/photo-app/config/storage.max-upload-mb
POST https://picloud.local/products/photo-app/config              → set entry
DEL  https://picloud.local/products/photo-app/config/storage.max-upload-mb
```

Workload-effective config (merged view):
```
GET https://picloud.local/products/photo-app/containers/worker/config
```

**Live reload:**
When a config entry changes, the platform emits `ConfigChanged`. Workloads subscribed to the product event bus receive the update. The SDK handles the subscription and invalidates its local cache automatically — workloads call `config.get("key")` and always get the current value without restarting.

**RDF representation:**
```turtle
<https://picloud.local/products/photo-app/config/storage.max-upload-mb>
    a picloud:ConfigEntry ;
    picloud:configKey   "storage.max-upload-mb" ;
    picloud:configValue "50" ;
    picloud:configType  "int" ;
    picloud:tag [ picloud:tagKey "tier" ; picloud:tagValue "storage" ] .
```

**Rationale:**
- Central config store decouples runtime values from deployment artifacts — change config without redeployment
- Workload override with merge-and-win gives fine-grained control without duplicating the full product config
- Live reload via events is consistent with the platform's event-driven model — no polling, no restart
- Tags on config entries enable SPARQL queries across config — e.g. "all config entries tagged `environment:production`"
- Typed values let the SDK deserialise correctly without the workload parsing strings manually

**Consequences:**
- `config` is a new Product-scoped resource type
- `ConfigChanged` is a new platform event
- The SDK config client maintains a local cache and subscription — see ADR-044 for SDK integration
- Secrets are not config entries — sensitive values use the existing secret resource (they are injected, not polled)

---

## ADR-044: Feature Flags as First-Class Product Resources

**Status:** Accepted

**Context:** Feature flags control which capabilities are active in a running system without redeployment. In PiCloud, flags are bound to Product versions — a flag targets a version expression, and only Products running a matching version see the flag as active. This enables progressive feature rollout across Product versions with explicit version intent.

**Decision:** Feature flags are a first-class Product resource. A flag declares a name, a version expression, and an enabled state. The platform evaluates flags against the running Product version. Workloads query flags via HTTP or the SDK. The SDK caches flags locally and subscribes to `FeatureFlagChanged` events for invalidation.

**Feature flag resource:**
```bicep
feature-flag 'new-upload-flow' = {
  product: 'photo-app'
  description: 'Enables the redesigned upload flow'
  enabled: true
  version: '= 2'            // exact match
}

feature-flag 'dark-mode' = {
  product: 'photo-app'
  description: 'Dark mode UI'
  enabled: true
  version: '>= 2'           // version 2 and above
}

feature-flag 'legacy-api' = {
  product: 'photo-app'
  description: 'Legacy v1 API compatibility shim'
  enabled: true
  version: '< 2'            // versions before 2
}

feature-flag 'beta-search' = {
  product: 'photo-app'
  description: 'Experimental search'
  enabled: true
  version: '2..4'           // versions 2, 3, and 4 inclusive
}
```

**Version expression operators:**

| Operator | Meaning | Example |
|---|---|---|
| `= N` | Exact version | `= 2` |
| `> N` | Greater than | `> 2` |
| `>= N` | Greater than or equal | `>= 2` |
| `< N` | Less than | `< 2` |
| `<= N` | Less than or equal | `<= 2` |
| `N..M` | Inclusive range | `2..4` |

Version numbers are the integer major version of the Product. `photo-app` version `2.1.0` has major version `2`.

**MVP flag value:** on/off only. Variant flags (percentage rollout, string variants) are a future phase.

**Flag evaluation:**
The platform evaluates a flag as active when:
1. `enabled: true`
2. The running Product version satisfies the version expression

A workload running in `photo-app@2.1.0` asking for `new-upload-flow` (version `= 2`) → **active**.
A workload running in `photo-app@1.5.0` asking for `new-upload-flow` (version `= 2`) → **inactive**.

**HTTP API:**
```
# All flags for the running version (evaluated)
GET https://picloud.local/products/photo-app/flags

# Single flag evaluation
GET https://picloud.local/products/photo-app/flags/new-upload-flow

# Response
{ "name": "new-upload-flow", "active": true, "version": "= 2" }
```

**SDK evaluation model:**
```rust
// Rust SDK
let flags = picloud.flags("photo-app").await?;
if flags.is_active("new-upload-flow") {
    // new flow
}
```

```typescript
// TypeScript SDK
const flags = await picloud.flags("photo-app");
if (flags.isActive("new-upload-flow")) {
    // new flow
}
```

```csharp
// .NET SDK
var flags = await picloud.Flags("photo-app");
if (flags.IsActive("new-upload-flow")) {
    // new flow
}
```

The SDK fetches all flags on startup, caches them locally, and subscribes to `FeatureFlagChanged` events. Flag evaluation is synchronous and in-process after initial load — zero network round-trips in the hot path.

**Live updates:**
When a flag changes (`enabled` toggled, version expression updated), the platform emits `FeatureFlagChanged`. The SDK receives this event, updates its local cache, and the next call to `is_active()` reflects the new state. No restart required.

**RDF representation:**
```turtle
<https://picloud.local/products/photo-app/flags/new-upload-flow>
    a picloud:FeatureFlag ;
    picloud:flagName        "new-upload-flow" ;
    picloud:flagEnabled     true ;
    picloud:flagVersion     "= 2" ;
    picloud:flagDescription "Enables the redesigned upload flow" .
```

**Rationale:**
- Version-bound flags make the intent explicit — "this feature exists from version 2" is a first-class declaration, not a comment
- Binding to Product version means flags are naturally cleaned up — when the minimum supported version exceeds a flag's expression, the flag is dead and should be removed
- SDK-local evaluation with event invalidation gives zero-latency flag checks in the hot path
- On/off MVP is the right starting point — variant flags add complexity that is not needed for Phase 1
- `FeatureFlagChanged` as an event means monitoring products can observe flag lifecycle across the cluster

**Consequences:**
- `feature-flag` is a new Product-scoped resource type
- `FeatureFlagChanged` is a new platform event
- Version expression parsing must handle all six operators and validate at deploy time — invalid expressions are rejected by the platform before the resource is created
- The SDK flag client must know the running Product version to evaluate expressions — this is injected by the platform as an environment variable at workload startup (`PICLOUD_PRODUCT_VERSION`)
- When a Product version changes (upgrade), `FeatureFlagChanged` events are emitted for all flags whose active state changes as a result of the version change

---

## ADR-045: OpenTelemetry as the Observability Standard

**Status:** Accepted

**Context:** Both the platform and Products need observability — traces, metrics, and logs. The standard for this is OpenTelemetry (OTel). The platform must produce OTel signals for its own operations and provide a path for workloads to emit theirs. OTel data must feed the alert system without overwhelming the Raft-replicated event log or Oxigraph.

**Decision:** OTel is the observability standard for the platform and all Products. The platform runs an OTel event stream — a high-throughput, non-Raft-replicated channel separate from the platform event log. Raw OTel flows through this stream to subscribers. A platform aggregator samples it every 15 seconds, computes summaries, and emits `MetricRecorded` events into the Raft log. Those summaries land in Oxigraph and feed the alert inference rules (ADR-041). Raw OTel spans and metrics are stored in the time-series layer (ADR-046).

### Signal coverage

All three OTel signals are in scope from day one:

- **Traces** — every CLI command produces a root span. Platform operations (Raft append, RDF projection, workload scheduling, inference rule evaluation) produce child spans. Workload traces are correlated to platform traces via W3C trace context propagation.
- **Metrics** — hardware metrics (ADR-040) and workload-emitted domain metrics flow through the OTel metrics pipeline. Aggregated summaries feed Oxigraph.
- **Logs** — structured logs from `picloud-server` and workloads are emitted as OTel log records with trace context attached.

### The OTel event stream

A dedicated pub/sub channel inside `picloud-server` — not Raft-replicated, not written to the event log. High throughput, bounded buffer, drop-on-overflow for non-critical signals. Subscribers register at runtime:

```
OTel signal produced
  → OTel event stream (in-process pub/sub)
    → Time-series store (ADR-046)       ← raw spans, metrics, logs
    → Platform aggregator               ← every 15s
      → MetricRecorded event            ← Raft log → Oxigraph → alerts
    → External OTel exporter (optional) ← OTLP to Grafana, Tempo, etc.
```

### CLI trace propagation

Every CLI command creates a root OTel span. The correlation ID on the command event carries the trace context. Platform operations that process that command create child spans under the same trace. The result is a complete trace from CLI invocation through Raft append, projection, scheduling, and workload startup.

```
picloud resource apply ./photo-app/
└── [trace] resource.apply
    ├── [span] raft.append
    ├── [span] rdf.project
    └── [span] workload.schedule
        └── [span] container.start
```

### Workload OTel configuration

The platform injects OTel configuration into every workload as environment variables at startup:

```
OTEL_SERVICE_NAME=photo-app.api-server
OTEL_SERVICE_VERSION=2.1.0
OTEL_EXPORTER_OTLP_ENDPOINT=https://picloud.local/otel
OTEL_RESOURCE_ATTRIBUTES=picloud.product=photo-app,picloud.node=pi-node-01
```

Workloads configure their OTel SDK using these variables — no hardcoding. Additional configuration can be set per-workload in the resource file or via the SDK at startup:

```bicep
container 'api-server' = {
  product: 'photo-app'
  otel: {
    traces: true
    metrics: true
    logs: true
    sampleRate: 1.0
  }
}
```

### Trace correlation — platform to workload

When a platform event causes a workload to receive traffic (e.g. `ResourceReady` triggers a health check, or an event subscription delivers an event), the platform attaches W3C trace context headers. The workload's OTel SDK picks these up automatically and creates child spans under the platform's trace. This gives end-to-end traces from CLI command through platform operations through workload execution.

### Aggregation into Oxigraph

The platform aggregator reads from the OTel stream every 15 seconds and computes per-resource summaries:

- Request rate (req/s)
- Error rate (%)
- P50/P95/P99 latency (ms)
- Active span count

These are written as `MetricRecorded` events — identical in structure to hardware metrics (ADR-040). Inference rules treat them identically. An alert rule for "product error rate above 5%" uses the same SPARQL CONSTRUCT pattern as a CPU temperature alert.

### External OTel export

Operators can configure an OTLP endpoint to forward raw OTel data to external systems (Grafana, Tempo, Jaeger, Prometheus). This is optional — the platform works without it. When configured, the external exporter is a subscriber on the OTel event stream.

```bicep
# Platform-level config
otel-export 'grafana' = {
  endpoint: 'https://grafana.acme.local:4317'
  protocol: 'grpc'
  signals: ['traces', 'metrics', 'logs']
}
```

**Rationale:**
- OTel is the industry standard — workloads instrumented with any OTel SDK work out of the box
- Separating the OTel stream from the Raft event log prevents high-volume telemetry from starving platform operations
- Aggregation before writing to Oxigraph solves the cardinality problem — the graph holds current-state summaries, not individual spans
- Injecting OTel config as environment variables means workloads need zero platform-specific code to be observable
- W3C trace context propagation is standard — no custom headers, any OTel SDK handles it
- Unifying hardware metrics (ADR-040) and product metrics at the `MetricRecorded` event level means one alert rule syntax for all metric types

**Consequences:**
- `picloud-http` must serve an OTLP endpoint at `https://picloud.local/otel` — workloads export here
- The OTel event stream is a new in-process component — not Raft-replicated, bounded buffer, not persistent
- Raw OTel data is stored in the time-series layer (ADR-046) — not in the event log
- The aggregator must handle metric cardinality carefully — aggregate by resource IRI, not by individual request
- `PICLOUD_PRODUCT_VERSION` (ADR-044) and OTel resource attributes are injected together at workload startup

---

## ADR-046: Apache Arrow + Parquet + DataFusion for Time-Series Storage

**Status:** Accepted

**Context:** Raw OTel spans, metrics, and logs are high-volume, high-cardinality, time-bounded data. They cannot go into the Raft event log (volume) or Oxigraph (cardinality). A dedicated time-series storage layer is needed that is pure Rust, embeds into `picloud-server`, runs on ARM64, and supports efficient time-range queries for the aggregator and for operator inspection.

**Decision:** Raw OTel data is stored as Apache Parquet files on each node's NVMe. Apache Arrow is the in-memory columnar format. DataFusion provides SQL query execution over the Parquet files. All three crates are pure Rust and compile to ARM64 with no external dependencies.

### Storage layout

```
/home/ubuntu/picloud/data/telemetry/
├── traces/
│   ├── 2025-07-01T00/    ← hourly partitions
│   │   ├── part-0001.parquet
│   │   └── part-0002.parquet
│   └── 2025-07-01T01/
├── metrics/
│   ├── 2025-07-01T00/
│   └── 2025-07-01T01/
└── logs/
    ├── 2025-07-01T00/
    └── 2025-07-01T01/
```

Partitioned by hour. Each partition is one or more Parquet files, rotated when they reach a configurable size (default: 128MB). Old partitions are deleted by a retention policy (default: 7 days for traces, 30 days for metrics, 7 days for logs).

### Parquet schema — traces

```
trace_id:       Utf8
span_id:        Utf8
parent_span_id: Utf8 (nullable)
operation_name: Utf8
service_name:   Utf8
product:        Utf8 (nullable)
node_id:        Utf8
start_time:     Timestamp(Nanosecond)
end_time:       Timestamp(Nanosecond)
duration_ms:    Float64
status:         Utf8   (ok | error | unset)
attributes:     Utf8   (JSON)
```

### Parquet schema — metrics

```
timestamp:      Timestamp(Nanosecond)
resource_iri:   Utf8
metric_name:    Utf8
metric_value:   Float64
unit:           Utf8
product:        Utf8 (nullable)
node_id:        Utf8
attributes:     Utf8   (JSON)
```

### Parquet schema — logs

```
timestamp:      Timestamp(Nanosecond)
trace_id:       Utf8 (nullable)
span_id:        Utf8 (nullable)
severity:       Utf8
body:           Utf8
service_name:   Utf8
product:        Utf8 (nullable)
node_id:        Utf8
attributes:     Utf8  (JSON)
```

### Querying via DataFusion

The platform exposes a OTLP-compatible query API and a SQL endpoint over DataFusion:

```bash
# CLI query
picloud telemetry query \
  --signal traces \
  --from "2025-07-01T00:00:00Z" \
  --to   "2025-07-01T01:00:00Z" \
  --sql  "SELECT operation_name, AVG(duration_ms) FROM traces
          WHERE product = 'photo-app'
          GROUP BY operation_name
          ORDER BY AVG(duration_ms) DESC"
```

### Aggregator reads from Parquet

The metric aggregator (ADR-045) queries Parquet files via DataFusion every 15 seconds to compute summaries:

```sql
SELECT
  resource_iri,
  AVG(metric_value)                                    AS avg_value,
  PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY metric_value) AS p95_value
FROM metrics
WHERE metric_name = 'http.request.duration'
  AND timestamp > NOW() - INTERVAL '15 seconds'
GROUP BY resource_iri
```

Results become `MetricRecorded` events → Oxigraph → alert rules.

### Retention policy

Configurable per signal type. Default:

| Signal | Retention |
|---|---|
| Traces | 7 days |
| Metrics | 30 days |
| Logs | 7 days |

A background task runs hourly, deletes partition directories older than the retention window.

### Why not Delta Lake

Delta Lake is built on Parquet and adds ACID transactions, schema evolution, and time-travel. These are valuable for analytical workloads but add a Spark or DuckDB dependency for writes. A future Rust-native Delta Lake implementation would be a natural evolution of this storage layer — the Parquet files produced here would be compatible with Delta Lake with the addition of a transaction log.

**Rationale:**
- Pure Rust — arrow, parquet, datafusion crates all compile to ARM64 with no external dependencies (ADR-001)
- Single binary stays intact — no separate time-series daemon
- Columnar Parquet is highly efficient for time-range and aggregation queries over metric data
- DataFusion SQL is accessible to LLMs and operators without specialised knowledge
- Hourly partitioning means retention cleanup is O(1) — delete a directory, no compaction needed
- Parquet is self-describing and portable — files can be analysed off-node with any Arrow-compatible tool
- Natural upgrade path to Delta Lake when a Rust-native implementation is available

**Consequences:**
- `picloud-storage` gains a `TelemetryStore` implementation backed by Parquet
- The telemetry store is local to each node — not distributed across the cluster
- For cluster-wide telemetry queries, the aggregated summaries in Oxigraph are the right layer — raw Parquet queries are per-node
- Write throughput must be benchmarked on Pi5 NVMe — Parquet writes are batched, not per-span
- The `arrow`, `parquet`, and `datafusion` crates add significant compile time — acceptable given the capability they provide
