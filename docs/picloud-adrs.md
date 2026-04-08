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

## ADR-035: Delta Tables as Optional Product Analytical Storage Resource

**Status:** Proposed (Phase 3+)

**Context:** Products may need to store and query large analytical datasets — telemetry, time-series data, audit logs, or event-derived aggregations — where the append-only event store (ADR-032) and SPARQL projection (ADR-005) are not the right fit. These workloads benefit from columnar storage with ACID transactions, time travel, and predicate pushdown. Delta Lake (via `delta-rs`) provides these capabilities on top of Parquet files.

**Decision:** Offer Delta Tables as a declarable product-level storage resource for analytical workloads. Delta Tables are **not** used for the platform event log or product event stores — those remain append-only JSON Lines replicated via Raft (ADR-004, ADR-032). Delta Tables are a separate, opt-in storage primitive.

Products declare delta table resources in their manifest:
```bicep
delta-table 'telemetry' = {
  product: 'photo-app'
  schema: 'schemas/telemetry.parquet-schema'
  partition_columns: ['date', 'source']
  retention_days: 90
}
```

Addressable via IRI:
```
https://picloud.local/products/photo-app/delta-tables/telemetry
```

**Rationale:**
- The platform event log is optimized for sequential append, Raft replication, and real-time broadcast — not analytical queries. Delta Tables fill a complementary niche for batch/analytical access patterns.
- `delta-rs` is a pure Rust implementation with no JVM dependency, consistent with ADR-001.
- Delta's time travel and ACID transactions map well to the platform's auditability requirements.
- Parquet's columnar format enables efficient range scans and aggregations that SPARQL is not optimized for on large numerical datasets.
- Product isolation (ADR-016, ADR-018) is maintained — each product's delta tables are scoped to that product's storage allocation.

**Rejected alternatives:**
- **Using Delta Tables for the platform event log** — granularity mismatch. The event log appends single events in real-time; Delta Tables operate at file/batch granularity. Buffering events into batches would break the real-time SSE subscription model. The Arrow/Parquet dependency weight is also unjustified for sequential replay workloads.
- **DuckDB as embedded analytical engine** — viable alternative, but Delta Tables integrate better as a storage format that can be read by external tools. DuckDB could be offered as a query engine on top of Delta Tables in a future ADR.
- **Raw Parquet files without Delta** — loses ACID transactions, time travel, and schema evolution. Delta's transaction log adds minimal overhead while providing significant correctness guarantees.

**Consequences:**
- `delta-rs`, `arrow`, and `parquet` become optional dependencies — only compiled when the delta-table storage resource is enabled. Feature-gated to avoid binary size impact on minimal deployments.
- The `picloud-storage` crate gains a Delta Table backend implementing a new `DeltaTableStore` trait defined in `picloud-domain::traits`.
- Products can project event store data into Delta Tables via platform-provided projectors, enabling analytical queries over event-sourced data without querying the event log directly.
- Storage allocation (ADR-024) must account for Parquet file sizes and Delta transaction log overhead.
- Phase 4 compaction and retention policies apply to delta tables via Delta's built-in `VACUUM` semantics.

## ADR-036: User Groups, Resource Tagging, and Tag-Based Group Membership Inference

**Status:** Accepted (Phase 2)

**Context:** PiCloud's IAM model supports individual human identities with platform roles and permissions. As clusters grow, managing permissions per-user becomes unwieldy. Operators need a way to organize users into groups and assign permissions at the group level. Additionally, automatic group membership based on resource attributes would reduce manual administration — for example, "all users tagged `department=engineering` should be in the `engineering-team` group."

**Decision:** Introduce three interconnected capabilities:

1. **User Groups** — A `Group` is a first-class platform resource with its own IRI (`https://picloud.local/groups/{name}` for platform groups, or product-scoped via `https://picloud.local/products/{product}/groups/{name}`). Groups carry permissions identical in structure to roles. Users can be explicitly added to groups. A user's effective permissions are the union of their direct permissions and all group permissions.

2. **Resource Tagging** — All platform resources gain a `tags` field: a set of key-value pairs (`Vec<Tag>` on `ResourceMeta`). Tags are projected into the RDF graph as structured triples. Tags are free-form strings — the platform does not enforce a tag taxonomy.

3. **Tag-Based Group Membership Rules** — A `GroupMembershipRule` is a resource associated with a group. Each rule specifies a set of tag conditions (key-value pairs, AND logic). The RDF projector materializes group membership by evaluating rules: when a user is tagged or a rule is created/deleted, the projector runs SPARQL queries to find all identities matching the rule's tag conditions and inserts/removes `picloud:memberOf` triples accordingly. This gives operators declarative, self-maintaining group membership.

Event flow:
```
ResourceTagged → projector stores tag triples → evaluates all rules → materializes memberOf
GroupMembershipRuleCreated → projector stores rule triples → evaluates rule → materializes memberOf
```

RDF model:
```turtle
# Tags on resources
<resource> picloud:hasTag <resource#tag-department> .
<resource#tag-department> picloud:tagKey "department" .
<resource#tag-department> picloud:tagValue "engineering" .

# Group
<group> a picloud:Group .
<group> picloud:name "engineering-team" .
<group> picloud:permission "products/photo-app/*:read" .

# Explicit membership
<identity> picloud:memberOf <group> .

# Membership rule
<rule> a picloud:GroupMembershipRule .
<rule> picloud:targetGroup <group> .
<rule> picloud:requiresTag <rule#cond-0> .
<rule#cond-0> picloud:tagKey "department" .
<rule#cond-0> picloud:tagValue "engineering" .
```

**Rationale:**
- Groups are the standard RBAC primitive for managing permissions at scale. Every serious IAM system has them.
- Tags are the natural metadata mechanism for cloud platforms. They enable automation, cost tracking, and policy without schema changes.
- RDF inference via SPARQL is a natural fit for PiCloud's architecture — the graph is already the read model, and SPARQL is expressive enough to evaluate tag-matching rules without custom inference engines.
- Materializing membership at projection time (rather than computing at query time) keeps authorization checks fast — a simple `picloud:memberOf` lookup.
- AND logic for tag conditions covers the vast majority of use cases. OR logic can be achieved by creating multiple rules for the same group.

**Rejected alternatives:**
- **OPA/Rego policy engine** — adds an external dependency and a separate policy language. SPARQL-based rules keep everything in the existing RDF model.
- **Lazy evaluation at query time** — would make every authorization check run a SPARQL query against all rules. Materialization is O(events) not O(requests).
- **Hierarchical groups (nested groups)** — deferred. Flat groups cover the primary use case. Nesting can be added later via transitive `picloud:memberOf` inference if needed.

**Consequences:**
- `ResourceMeta` gains a `tags: Vec<Tag>` field — all existing resource types become taggable without code changes.
- New event types: `GroupCreated`, `GroupDeleted`, `GroupMemberAdded`, `GroupMemberRemoved`, `GroupMembershipRuleCreated`, `GroupMembershipRuleDeleted`, `ResourceTagged`, `ResourceUntagged`.
- The RDF projector gains handlers for all new events plus inference logic for rule evaluation.
- Authorization checks must union direct permissions with group permissions — this affects `picloud-iam` token issuance.
- Tag keys and values are free-form strings. Operators are responsible for consistency (the platform may add tag key validation in a future ADR).
