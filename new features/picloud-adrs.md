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

**Test coverage:**

Scenario tests:
- `binary_compiles.rs` — `cargo build --release --target aarch64-unknown-linux-gnu` completes with zero errors and zero warnings. The resulting binary is a single ELF file with no dynamic library dependencies beyond `libc`.
- `no_runtime_panics.rs` — the full scenario harness runs to completion. Any Rust `panic!` in the binary is captured by the test runner and counted as a test failure.

Invariants:
- The binary has no dynamic dependencies other than `libc`. Verified by `ldd picloud` — any line other than `linux-vdso` or `libc` is a failure.
- No `unsafe` block in the codebase triggers undefined behaviour. Verified by running the full test suite under AddressSanitizer in CI on each PR.

Exit criteria:
- `cargo build --release` completes in < 15 minutes on a Raspberry Pi 5 (cold cache).
- Binary size < 100 MB (stripped).
- Zero `panic` calls reachable from production code paths (enforced via `#![deny(clippy::unwrap_used)]` in CI).
- `ldd` reports zero unexpected dynamic dependencies.

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

**Test coverage:**

Scenario tests:
- `raft_leader_election.rs` — bootstrap a two-node cluster. Assert exactly one node carries `picloud:hasRole picloud:Leader` in the RDF graph within 10 seconds of init.
- `raft_leader_failover.rs` — kill the current Raft leader process via SIGKILL. Assert a new leader is elected and the `picloud:Leader` triple updated within 5 seconds. Assert the cluster continues accepting commands.
- `raft_learner_join.rs` — add a third node as a Raft learner. Assert it appears in the RDF graph as `picloud:Learner` before being promoted to voter.

Invariants:
- Exactly one node holds `picloud:hasRole picloud:Leader` at all times. Checked every 1 second during Chaos runs. Any 2-second window with zero or two leaders is a failure.
- The Raft log index is strictly monotonically increasing on the leader. Checked by querying the internal Raft state API on all nodes every 5 seconds.

Chaos scenarios:
- Kill leader → assert re-election in < 5 seconds.
- Kill follower → assert cluster continues operating, leader unchanged.
- Kill both followers in a three-node cluster → assert leader steps down (no quorum), resigns `picloud:Leader` triple within 10 seconds.

Exit criteria:
- Leader election after SIGKILL: < 5 seconds, measured across 20 consecutive kill cycles.
- Zero split-brain events (two simultaneous leaders) across 20 kill cycles.
- Log index never decreases — verified across 100 consecutive Raft appends.

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

**Test coverage:**

Scenario tests:
- `dns_resolution.rs` — after `cluster init`, assert `picloud.local` resolves to a cluster node IP from an external client on the same broadcast domain within 2 seconds.
- `node_join_dns.rs` — after a third node joins, assert its hostname (`{node-id}.picloud.local`) resolves within 60 seconds of the `NodeJoined` event appearing in the RDF graph.
- `product_fqdn_dns.rs` — after `resource apply` for a Product, assert the product FQDN resolves correctly from a client that was connected before the product was deployed (tests cache invalidation path).

Invariants:
- `picloud.local` must resolve to an active node at all times. Probed every 5 seconds during Chaos runs. An unresolvable gap > 30 seconds is a test failure.
- mDNS responses must conform to RFC 6762: PTR record present, TTL ≥ 4500 seconds, no truncated responses. Verified via `dns-sd` capture.

Protocol probes:
- RFC 6762 compliance: query `picloud.local A` from macOS (native resolver), Linux (avahi-daemon), and Windows 10+ (native mDNS). Assert all three return the same IP.
- Assert no CNAME loops and no NXDOMAIN on any resource that has been applied and confirmed via the event stream.

Exit criteria:
- `picloud.local` resolves within 2 seconds of `cluster init` completing.
- New node hostname resolves within 60 seconds of `NodeJoined` event.
- Zero DNS resolution failures during a 5-minute Chaos run with one node killed and restored every 60 seconds (30-second gap tolerance applies).

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

**Test coverage:**

Scenario tests:
- `event_log_replay.rs` — apply a set of resources, record the RDF graph state via SPARQL, wipe the Oxigraph projection, replay the event log from index 0, assert the resulting graph is byte-identical to the recorded snapshot.
- `projection_consistency.rs` — after every `resource apply`, assert that the resulting RDF state (SPARQL ASK) matches the declared resource definition within the projection latency budget.
- `event_ordering.rs` — apply 50 resources in parallel from two CLI clients, assert the event log index is strictly monotonic and the final RDF graph reflects all 50 resources with no duplicates or gaps.

Invariants:
- The event log index must be strictly monotonically increasing on all nodes at all times. Any non-monotonic index is an immediate test failure.
- The RDF graph triple count must be identical on all nodes within 60 seconds of quorum being restored after a partition. Checked by running `SELECT (COUNT(*) AS ?n) WHERE { ?s ?p ?o }` on all nodes and comparing.
- No event must be lost during a Raft leader change. Verified by writing a sentinel event immediately before killing the leader and asserting the sentinel appears in the graph after the new leader is elected.

Exit criteria:
- Event-to-projection latency: p50 < 200 ms, p99 < 2000 ms under normal conditions.
- Full log replay (empty graph → current state) completes in < 30 seconds for a log of 10,000 events.
- Zero events lost across 20 consecutive leader-kill-and-restore cycles.
- RDF graph convergence after a 60-second network partition heals: < 60 seconds.

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

**Test coverage:**

Scenario tests:
- `rdf_projection_roundtrip.rs` — apply a product with containers, volumes, and identities. Assert every declared resource appears as typed triples in the graph via SPARQL ASK. Wipe Oxigraph, replay the event log, assert the graph is identical.
- `sparql_query_types.rs` — execute SELECT, ASK, CONSTRUCT, and DESCRIBE queries against the platform graph. Assert correct result formats and non-empty results for known-populated graphs.
- `graph_isolation.rs` — assert that a SPARQL query against the platform graph does not return triples from a product named graph, and vice versa (named graph isolation).

Invariants:
- All state reads are served from the RDF projection, never from the raw event log. Verified by asserting that disabling Oxigraph read access causes all status queries to fail — no fallback to raw log replay.
- Triple count on all nodes is identical within 60 seconds after any network partition heals.

Exit criteria:
- SPARQL ASK for any applied resource returns `true` within the projection latency budget (p99 < 2 s).
- Graph triple count consistent across all cluster nodes within 60 seconds of partition recovery.
- Named graph isolation: zero cross-contamination between platform and any product graph, verified by 100 randomised SPARQL queries.

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

**Test coverage:**

Scenario tests:
- `oxigraph_sparql_compliance.rs` — execute a representative set of SPARQL 1.1 queries (SELECT with FILTER, ASK, CONSTRUCT, DESCRIBE, SPARQL Update INSERT, DELETE) against the embedded Oxigraph instance. Assert correct results for each.
- `named_graph_isolation.rs` — write triples to three named graphs, assert that each named graph query returns only its own triples, and that the default graph union query returns all.
- `oxigraph_persistence.rs` — write triples, restart the `picloud-server` process, assert triples are still present (verifies persistence across process restart).

Protocol probes:
- SPARQL 1.1 Protocol: POST to the SPARQL endpoint with correct Content-Type. Assert 200 with `application/sparql-results+json`. Submit malformed SPARQL, assert 400.

Exit criteria:
- All SPARQL 1.1 query types return correct results with zero errors.
- Named graph isolation verified — zero cross-graph leakage.
- Triples survive process restart (persistent storage working).
- SPARQL protocol: malformed query returns 400, not 500.

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

**Test coverage:**

Scenario tests:
- `picloud_syntax_parse.rs` — parse a representative set of `.picloud` files covering all resource types. Assert zero parse errors on valid inputs.
- `invalid_syntax_rejection.rs` — submit `.picloud` files with deliberate syntax errors (missing braces, invalid property names, wrong types). Assert each returns a human-readable error, not a panic or 500.
- `symbolic_reference_resolution.rs` — declare a container that references a volume by symbolic name. Assert the compiler resolves the reference and produces correct Turtle with the volume IRI.

Exit criteria:
- Parsing all valid `.picloud` files in the test corpus: zero errors.
- Invalid syntax: each error case returns a non-empty, human-readable error message within 1 second (no timeout, no panic).
- Symbolic references compile to correct Turtle IRIs deterministically — same input always produces the same output.

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

**Test coverage:**

Scenario tests:
- `command_correlation.rs` — emit `picloud resource apply` with a known correlation ID. Subscribe to the result stream. Assert the terminal event (`ResourceReady` or `ResourceFailed`) carries the same correlation ID and arrives within 30 seconds.
- `progress_streaming.rs` — apply a multi-resource product. Assert that intermediate progress events (`ResourceDeclared`, `ResourceProvisioning`) stream to the CLI before the terminal event.
- `concurrent_commands.rs` — emit 10 `resource apply` commands concurrently from separate CLI processes. Assert all 10 terminal events arrive with matching correlation IDs and no events are cross-contaminated.

Invariants:
- Every emitted command event must have a corresponding terminal result event within 60 seconds under normal conditions. Any command without a terminal event after 60 seconds is a test failure.

Exit criteria:
- CLI-to-terminal-event latency: p50 < 500 ms, p99 < 5 s for single-resource apply under normal conditions.
- Concurrent 10-command burst: all 10 terminal events received with correct correlation IDs, zero cross-contamination.

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

**Test coverage:**

Scenario tests:
- `human_identity_lifecycle.rs` — create a human identity, issue a token via CLI device flow, decode the JWT, assert `iss`, `sub`, `aud`, `exp`, `iat` claims are present and correct.
- `workload_identity_injection.rs` — deploy a container with a workload identity. Assert the container process receives an injected credential and can use it to request a token from the IAM endpoint. Assert the token `sub` matches the workload identity IRI.
- `token_expiry_enforcement.rs` — issue a token, wait for it to expire, present it to an IAM-gated endpoint, assert 401 with `WWW-Authenticate` header.

Protocol probes:
- OIDC `.well-known/openid-configuration`: assert all required fields present (`issuer`, `authorization_endpoint`, `token_endpoint`, `jwks_uri`). Assert `issuer` matches the cluster domain.
- JWKS endpoint: assert `kid` present, `alg` is RS256 or ES256, no `none` algorithm in the key set.

Invariants:
- Tokens issued before a Raft leader change are still cryptographically valid after the change. Verified by issuing a token, killing the leader, then validating the token against the JWKS of the new leader.

Exit criteria:
- Token issuance: < 500 ms p99.
- JWT claims: all required claims present and correct on 100 consecutive issuances.
- OIDC `.well-known` response: all required fields present, validates against OpenID Connect Discovery spec.
- Expired token: 100% rejection rate at IAM-gated endpoints.

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

**Test coverage:**

Scenario tests:
- `container_schedule.rs` — apply a container resource. Assert `ResourceReady` event emitted, container running (via `youki state`), and RDF graph reflects `picloud:status picloud:Running`.
- `binary_workload.rs` — schedule a raw ARM64 binary. Assert it starts, receives injected `PICLOUD_WORKLOAD_IDENTITY` environment variable, and is reachable via its internal DNS name.
- `workload_identity_injection.rs` — assert that both container and binary workloads receive the same identity injection, secret injection, and volume mount treatment. Compare environment variables between the two workload types.

Invariants:
- Scheduled workloads remain in `picloud:Running` state through a Raft leader change. Checked by polling the RDF graph every 5 seconds during a leader-kill scenario — any `picloud:Failed` state that is not subsequently recovered is a test failure.

Exit criteria:
- OCI container startup: < 30 seconds from `resource apply` to `ResourceReady`.
- Raw binary startup: < 10 seconds from `resource apply` to `ResourceReady`.
- Identity injection verified in both workload types: 100% of test runs.

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

**Test coverage:**

Scenario tests:
- `phase_dependency_order.rs` — assert that the block storage scenario suite (`volume_mount.rs`, `replication_coverage.rs`) passes before the RDF store scenario suite (`product_sparql.rs`) is executed. The test runner enforces this ordering and fails the RDF store suite immediately if any block storage test has failed in the same run.

Exit criteria:
- Phase gate enforced: RDF store tests do not run if block storage tests are failing. Zero exceptions to this ordering rule.

---

## ADR-012: Mounted and Raw Block Device Support

**Status:** Accepted

**Context:** Different workloads have different storage access requirements. Databases typically want raw block devices to manage their own filesystems. Application containers typically want mounted filesystems.

**Decision:** PiCloud supports both mounted volumes (filesystem presented at a path) and raw block devices. Both are backed by the same distributed block storage pool.

**Rationale:**
- Mounted volumes cover the majority of use cases
- Raw block devices are required for databases (PostgreSQL, RocksDB) that manage their own storage layout
- Both types use the same allocation and replication mechanisms — no storage layer duplication

**Test coverage:**

Scenario tests:
- `mounted_volume.rs` — allocate a mounted volume, attach it to a container at `/data`, write a sentinel file inside the container, restart the container, assert the sentinel file is present.
- `raw_block_device.rs` — allocate a raw block device volume. Assert the block device node (e.g. `/dev/xvdb`) is present inside the container. Write a known pattern to the device, read it back, assert byte-identical.
- `volume_mount_restart.rs` — restart the `picloud-server` process on the node hosting the volume. Assert the volume remains mounted and the sentinel file is still readable after restart.

Invariants:
- Mounted volumes survive Raft leader change without data loss. Verified by writing sentinel files before and reading after a leader kill cycle.

Exit criteria:
- Zero data loss for mounted volumes across 10 node-kill-restore cycles.
- Raw block device accessible inside container: verified on 100% of test runs.
- Volume remounted after node restart: sentinel file readable within 60 seconds of process restart.

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

**Test coverage:**

Scenario tests:
- `full_replication_coverage.rs` — allocate a `full-replication` volume on a three-node cluster. Write known data from node A. Assert the data is readable from node B and node C without contacting node A.
- `replication_on_node_join.rs` — allocate a volume on a two-node cluster. Add a third node. Assert the volume is replicated to the new node within 120 seconds of the `NodeJoined` event.

Invariants:
- A `full-replication` volume must be readable from every node in the cluster at all times. Verified during Chaos runs by reading from a different node than the writer after each kill event.

Exit criteria:
- Zero data loss when any single node is killed, for a `full-replication` volume. Verified across 10 kill-restore cycles on a three-node cluster.
- Replication to a new node completes within 120 seconds of `NodeJoined`.

---

## ADR-014: Service Discovery and Internal DNS in MVP

**Status:** Accepted

**Context:** Workloads need to find each other by name. Without service discovery, container addresses are ephemeral and workloads must be reconfigured when peers restart or reschedule.

**Decision:** Internal DNS and service discovery are MVP features, not future phases. Every resource that accepts network traffic is automatically registered as `{resource}.{product}.picloud.internal`.

**Rationale:**
- Without service discovery, containers cannot find each other — the platform is not useful
- Internal DNS is a small implementation surface relative to its impact
- Automatic registration means operators never configure DNS manually

**Test coverage:**

Scenario tests:
- `internal_dns_resolution.rs` — deploy two containers in the same product. From container A, resolve `{resource-B}.{product}.picloud.internal`. Assert it resolves to container B's IP within 10 seconds of `ResourceReady`.
- `cross_product_isolation.rs` — assert that a container in `product-A` cannot resolve `{resource}.{product-B}.picloud.internal` (internal DNS is scoped to the product namespace).

Invariants:
- Every `ResourceReady` event must result in a resolvable internal DNS name within 10 seconds. Verified by probing DNS from a sibling workload after each scheduling event.

Exit criteria:
- Internal DNS resolution: < 10 seconds after `ResourceReady` event, 100% of test runs.
- Cross-product name isolation: NXDOMAIN for cross-product `.internal` queries, 100% of test runs.

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

**Test coverage:**

Scenario tests:
- `idempotent_apply.rs` — apply the same resource file twice in succession. Assert the second apply produces zero new events in the event log (idempotency key deduplicated).
- `partial_failure_reapply.rs` — kill the cluster midway through a `resource apply`. Re-apply after recovery. Assert the final state is correct and no resources are duplicated.
- `idempotency_key_uniqueness.rs` — assert that two different apply operations (different files) produce distinct idempotency keys and are not deduplicated.

Invariants:
- Re-running `resource apply` on an unchanged file must never produce new events in the event log.

Exit criteria:
- Zero new events on second apply of unchanged file: 100% of test runs.
- Post-failure reapply reaches correct final state: 100% of 20 kill-midway tests.

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

**Test coverage:**

Scenario tests:
- `product_full_lifecycle.rs` — apply a product with container, volume, and identity. Assert `ProductReady` event. Delete the product. Assert `ProductDeleted` event and all child resources removed from the RDF graph within 60 seconds.
- `cascading_delete.rs` — apply a product with 5 child resources. Delete the product. Assert all 5 child resource IRIs return SPARQL `ASK { ?s ?p ?o }` = false within 60 seconds.
- `orphan_prevention.rs` — delete a product. Query the RDF graph for any resource whose IRI contains the deleted product's path. Assert the result set is empty.

Invariants:
- No resource can exist in the RDF graph with a product IRI that has been deleted. Checked after every cascading delete.

Exit criteria:
- Cascading delete of a 10-resource product completes within 60 seconds.
- Zero orphaned resources in the graph after product deletion: 100% of test runs.

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

**Test coverage:**

Scenario tests:
- `oidc_authorization_code.rs` — initiate OIDC authorization code flow against a deployed Product. Complete passkey authentication. Assert ID token received with correct `iss`, `aud`, `sub`, and `exp` claims.
- `oidc_client_credentials.rs` — execute client credentials grant for an App Registration. Assert access token received, token type is Bearer, and `expires_in` is present.
- `jwks_key_rotation.rs` — trigger key rotation. Assert JWKS endpoint serves both old and new keys during the rotation window. Assert tokens issued under the old key are still valid during the window.

Protocol probes:
- `GET /.well-known/openid-configuration` — assert all required OpenID Connect Discovery fields present. Assert `issuer` value matches cluster domain exactly.
- JWKS endpoint — assert `kid` present and matches token header, `alg` is RS256 or ES256, `none` algorithm absent.
- Token endpoint — assert `access_token`, `token_type: Bearer`, `expires_in` in response. Assert missing `client_secret` returns 401.

Exit criteria:
- All required OIDC Discovery fields present: 100% of probes.
- Key rotation: tokens issued before rotation remain valid throughout the rotation window.
- Token issuance via client credentials: < 500 ms p99.

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

**Test coverage:**

Scenario tests:
- `inter_product_event_delivery.rs` — product A emits an event to the platform bus. Product B has a declared `event-subscription` resource for that event type. Assert product B's workload receives the event within 5 seconds. Assert event appears in the RDF graph.
- `direct_network_blocked.rs` — attempt a direct TCP connection from a container in product A to a container in product B on any port other than the declared ingress. Assert the connection is refused (no route exists).
- `event_bus_burst.rs` — product A emits 1000 events in a 10-second burst. Assert all 1000 are received by product B's subscriber with zero loss. Assert event IDs match.

Invariants:
- The only inter-product network paths are the platform event bus and declared SPARQL endpoints. Verified by asserting that `picloud.internal` names for product B's resources are NXDOMAIN from product A's containers.

Exit criteria:
- Inter-product event delivery: < 5 seconds latency, zero loss in 1000-event burst test.
- Direct network blocking: 100% rejection rate for non-bus, non-SPARQL cross-product connections.

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

**Test coverage:**

Scenario tests:
- `product_sparql_endpoint.rs` — deploy a product with `rdf-store`. Run a SPARQL SELECT against the product's SPARQL endpoint with a valid workload token. Assert 200 and correct results.
- `sparql_iam_enforcement.rs` — query the product SPARQL endpoint with no token (assert 401), with an expired token (assert 401), with a token for a different product (assert 403), and with a valid scoped token (assert 200).
- `ontology_served.rs` — GET the product's ontology IRI. Assert 200 with `text/turtle` content type and non-empty Turtle body containing the declared ontology.

Protocol probes:
- SPARQL 1.1 Protocol at the product endpoint: SELECT returns `application/sparql-results+json`, CONSTRUCT returns `text/turtle`, malformed SPARQL returns 400.

Exit criteria:
- IAM enforcement: 100% of no-token and wrong-token requests rejected, 100% of valid-token requests accepted.
- SPARQL protocol compliance at product endpoint matches platform graph compliance.

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

**Test coverage:**

Scenario tests:
- `cluster_registry_discovery.rs` — deploy three products with different event types, SPARQL endpoints, and ontologies. Query the cluster-level SPARQL endpoint for all products, their event schemas, and their ontology IRIs. Assert all three products discoverable in a single query.
- `registry_version_binding.rs` — deploy product v1, then upgrade to v2. Assert the cluster graph reflects the new version and the old version's resources are no longer present.

Exit criteria:
- All deployed products discoverable via a single cluster-level SPARQL query within 30 seconds of deployment.
- Version change reflected in cluster registry within one projection cycle (< 2 seconds).

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

**Test coverage:**

Scenario tests:
- `atomic_version_cutover.rs` — deploy product v1, then apply v2. Monitor the RDF graph and the product's ingress throughout the upgrade. Assert there is no window where both v1 and v2 containers are simultaneously tagged `picloud:Running` under the product IRI.
- `failed_upgrade_rollback.rs` — deploy product v1, then apply v2 where one required resource is deliberately misconfigured. Assert v2 deployment fails, v1 resources remain `picloud:Running`, and no v2 resources are left in the graph.
- `one_active_version_invariant.rs` — query `SELECT DISTINCT ?version WHERE { <product-iri> picloud:activeVersion ?version }` after any deployment. Assert the result always contains exactly one row.

Invariants:
- At no point during an upgrade does the RDF graph show two active versions for the same product.

Exit criteria:
- Atomic cutover: zero windows with two simultaneous active versions across 20 upgrade cycles.
- Failed upgrade: v1 remains live in 100% of deliberate-failure tests.

---

## ADR-022: Inter-Product Event Subscriptions as First-Class Resources

**Status:** Accepted

**Context:** A Product that subscribes to another Product's events needs to declare that dependency somewhere. It could be implicit (subscribe at runtime) or explicit (declared as a resource).

**Decision:** Event subscriptions are declared as `event-subscription` resources in `.picloud` files. The platform provisions and manages the subscription lifecycle. Runtime subscriptions without a resource declaration are not permitted.

**Rationale:**
- All inter-product dependencies are visible in resource files — the dependency graph is auditable and version-controlled
- The platform can enforce that a subscription's source Product and event type exist before provisioning
- Consistent with the IaC-as-only-interface principle — everything exists in a file

**Test coverage:**

Scenario tests:
- `event_subscription_provisioning.rs` — declare an `event-subscription` resource in a product file. Apply it. Assert the subscription IRI appears in the RDF graph and events from the source product are delivered.
- `undeclared_subscription_rejection.rs` — attempt to subscribe to a product's events at runtime via the SDK without a declared `event-subscription` resource. Assert the platform returns 403.
- `subscription_lifecycle.rs` — delete the `event-subscription` resource. Assert events from the source product are no longer delivered and the subscription IRI is removed from the graph.

Exit criteria:
- Declared subscription: events flow within 5 seconds of provisioning.
- Undeclared runtime subscription: rejected with 403, 100% of attempts.

---

## ADR-023: Ontology Files Bound to Product Version

**Status:** Accepted

**Context:** A Product's RDF graph has a schema. That schema may evolve as the Product evolves. Consumers need to know which schema they are querying.

**Decision:** Ontology files (`.ttl` or `.shacl`) are declared as `ontology` resources in the Product's resource file and bound to the Product version. The platform serves the ontology file from the cluster graph. When a new Product version is deployed, the ontology is updated atomically with the rest of the Product's resources.

**Rationale:**
- Schema and implementation are versioned together — no schema/implementation drift
- Consumers can discover the exact schema for any Product version from the cluster graph
- SHACL files provide validation shapes — the platform can optionally validate graph updates against them

**Test coverage:**

Scenario tests:
- `ontology_version_binding.rs` — deploy product v1 with an ontology resource. Assert the ontology IRI is versioned (`/ontology/v1`) and resolves with the correct Turtle body. Deploy v2 with an updated ontology. Assert the v2 IRI resolves with the new body and the v1 IRI still resolves with the original body.
- `ontology_shacl_validation.rs` — add a triple that violates the product's SHACL ontology to the product graph. Assert the platform rejects the update with a SHACL validation error.

Exit criteria:
- Ontology IRI resolves within 5 seconds of product deployment.
- Old ontology IRIs remain resolvable after version upgrade: 100% of probes.
- SHACL violations rejected: 100% of deliberately invalid graph updates.

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

**Test coverage:**

Scenario tests:
- `storage_intent_full_replication.rs` — declare a volume with `durability: full-replication`. Apply it. Query the RDF graph and assert the volume's replication state shows N replicas for an N-node cluster.
- `intent_translated_to_implementation.rs` — query `picloud:replicationFactor` and `picloud:replicationNodes` on the volume IRI after allocation. Assert both match the cluster's current node count.

Exit criteria:
- `full-replication` volume replicated to all N nodes within 60 seconds of allocation, verified via SPARQL.
- Operator never needs to specify a replication factor — zero such fields accepted in the volume resource definition.

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

**Test coverage:**

Scenario tests:
- `passkey_registration.rs` — bootstrap a fresh cluster. Complete the WebAuthn registration ceremony using a hardware FIDO2 key. Assert the admin identity is created, the passkey is registered, and no password is present anywhere in the platform event log or RDF graph.
- `fido2_cli_auth.rs` — authenticate the CLI using a FIDO2 hardware key via the device flow. Assert a valid token is issued with the correct `sub` and `iss` claims. Assert the token payload contains no password-derived fields.
- `webauthn_challenge_replay_rejection.rs` — capture a WebAuthn challenge response, attempt to replay it. Assert the platform rejects the replayed assertion.

Protocol probes:
- WebAuthn Level 2 ceremony: assert the challenge is random (≥ 16 bytes), the origin is bound to the cluster domain, and the `rpId` matches the cluster domain.
- Assert no `password` field in any token, identity resource, or event in the platform log.

Exit criteria:
- Passkey registration and first login: completes within 60 seconds of `cluster init`.
- Challenge replay attack: rejected 100% of the time.
- Zero passwords in any platform-managed data structure: verified by full RDF graph scan.

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

**Test coverage:**

Scenario tests:
- `bootstrap_token_single_use.rs` — use a bootstrap token to register the first admin. Attempt to reuse the same token. Assert the second use returns 401.
- `bootstrap_token_expiry.rs` — generate a bootstrap token with a 1-minute TTL. Wait 90 seconds. Attempt to use the token. Assert rejection with a clear expiry error.
- `tier1_admin_reset.rs` — admin A initiates a passkey reset for user B via `picloud identity reset-passkey`. User B re-enrolls. Assert old passkey is revoked (old credential rejected), new passkey accepted.
- `tier3_physical_recovery.rs` — simulate all admin accounts being inaccessible. Run `picloud cluster recover` directly on a node (local-only, no network). Assert a new bootstrap token is generated and the recovery event appears as a high-severity audit entry in the platform event log.

Exit criteria:
- Bootstrap token: single-use enforced 100%, expiry enforced 100%.
- Tier 1 reset: old credential rejected within 5 seconds of re-enrollment completion.
- Tier 3 recovery: recovery event present in log with severity `critical`, new bootstrap token works exactly once.

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

**Test coverage:**

Scenario tests:
- `mtls_enforcement.rs` — attempt to connect to the platform API with no client certificate: assert TLS handshake fails with `certificate_required` alert. Attempt with a self-signed cert not issued by the cluster CA: assert rejection. Connect with a valid platform-issued certificate: assert 200.
- `workload_cert_injection.rs` — start a container workload. Assert the workload receives its mTLS certificate as an injected file. Assert the certificate chains to the cluster CA.
- `sparql_direct_mtls.rs` — query a product SPARQL endpoint directly from a workload using its injected mTLS certificate (no platform proxy hop). Assert 200 and correct query results.

Protocol probes:
- RFC 8446 TLS 1.3: assert mutual authentication required on all inter-node connections. Assert TLS version is 1.3 (1.2 rejected). Assert cipher suite is acceptable (no RC4, no export ciphers).
- Assert `certificate_required` alert on no-cert connections, not a generic TLS error.

Exit criteria:
- No-cert connection: rejected 100% of attempts.
- Wrong-CA cert: rejected 100% of attempts.
- Valid cert: accepted 100% of attempts.
- mTLS enforcement verified on all three connection types: node-to-node, workload-to-platform, workload-to-SPARQL.

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

**Test coverage:**

Scenario tests:
- `slice_dependency_enforcement.rs` — for each slice crate, run `cargo build -p {crate}` with all other slices removed from the workspace. Assert each slice compiles independently with only `picloud-domain` as an internal dependency.
- `no_cross_slice_imports.rs` — run `cargo deny` or a custom lint that scans `Cargo.toml` for any `picloud-*` dependency in any slice other than `picloud-domain`. Assert zero violations.

Invariants:
- `cargo tree -p picloud-{slice}` shows `picloud-domain` as the only internal dependency for every non-server slice. This is run in CI on every PR.

Exit criteria:
- Zero cross-slice imports detected across the full workspace.
- Every slice compiles independently within 5 minutes on Pi5 hardware.

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

**Test coverage:**

Scenario tests:
- `platform_ca_export.rs` — run `picloud ca export`. Trust the exported CA in a test client's OS trust store. Connect to `https://picloud.local`. Assert 200 with no TLS warning.
- `byo_ca.rs` — init a cluster with `--ca-cert ./test-ca.pem --ca-key ./test-ca-key.pem`. Verify all issued node certificates chain to the provided CA, not a platform-generated one.
- `cert_chain_validation.rs` — extract a node certificate and verify the full chain: leaf → cluster CA (or BYO CA). Assert the chain is valid and the CA fingerprint in the `Issuer` field matches the cluster identity.

Protocol probes:
- X.509 chain validation: leaf cert → CA cert → verify signature at each step.
- Assert no self-signed leaf certificates — all certs must chain to the platform CA.
- Assert cert SANs match the node hostname and IP.
- Assert cert expiry > 30 days from test run date (catches near-expiry before it becomes an outage).

Exit criteria:
- Exported CA trusted by external client within 60 seconds of `cluster init`.
- BYO-CA: all certs chain to provided CA, zero certs issued by a platform-generated CA.
- Full chain validation: passes for 100% of issued certificates.

---

## ADR-029: IRI-Based Resource Addressing

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

**Test coverage:**

Scenario tests:
- `iri_dereferencing.rs` — GET the IRI of every known resource type (cluster root, node, product, container, volume, identity). Assert 200 and non-empty body for each content type.
- `content_negotiation.rs` — GET a resource IRI with `Accept: text/turtle`, then with `Accept: application/ld+json`, then with `Accept: application/json`. Assert correct Content-Type in each response and that the body is valid for the declared type.
- `iri_stability.rs` — apply a container resource, record its IRI. Reschedule the container to a different node. Assert the IRI is unchanged and still dereferenceable.

Protocol probes:
- HTTP content negotiation per RFC 7231: assert `Accept: text/turtle` returns `Content-Type: text/turtle`, assert `Accept: application/ld+json` returns `Content-Type: application/ld+json`.
- Assert all resource IRIs are path-based and rooted at the cluster domain — no subdomains, no opaque IDs.

Exit criteria:
- All resource IRIs dereferenceable: 100% of applied resources.
- Content negotiation correct for all four Accept types: 100% of probes.
- IRI stable across workload reschedule: verified on 20 reschedule cycles.

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

**Test coverage:**

Scenario tests:
- `schema_iri_resolution.rs` — emit a platform event (e.g. `ResourceReady`). Extract the `schema` field from the event envelope. GET the schema IRI. Assert 200 and a valid JSON Schema body.
- `schema_evolution.rs` — emit 100 events under schema v1. Deploy a v2 projector that handles both v1 and v2. Replay the log. Assert the v2 projector correctly processes all v1 events.
- `schema_iri_permanence.rs` — deploy a new platform version that introduces schema v2 for an event type. Assert the v1 schema IRI still resolves and returns the original v1 schema body.

Exit criteria:
- All schema IRIs resolve to valid JSON Schema: 100% of emitted event types.
- Old schema IRIs remain valid after platform version upgrade: verified on every platform release.
- V1 events correctly processed by a v2 projector: 100% of 100 replayed events.

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

**Test coverage:**

Scenario tests:
- `event_store_append_read.rs` — declare an `event-store` resource with a Photo aggregate. Append 10 `PhotoCreated` events. Read the aggregate stream. Assert all 10 events returned in order with correct payloads.
- `event_store_rdf_projection.rs` — append aggregate events. Assert the product's SPARQL endpoint reflects the projected aggregate state within the projection latency budget.
- `event_store_replay.rs` — deploy a product with a deliberate projector bug that projects incorrect triples. Fix the projector in v2. Deploy v2 and replay the event store. Assert the RDF graph now reflects the correct state.
- `event_store_survivor.rs` — append 100 events, kill the Raft leader, assert all 100 events readable after leader failover.

Invariants:
- The event store log index is monotonically increasing. Verified continuously during Chaos runs.

Exit criteria:
- Append latency: < 10 ms p99 under normal conditions.
- Event store replay of 1000 events: < 30 seconds.
- Zero events lost across 10 leader-kill-restore cycles.

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

**Test coverage:**

Scenario tests:
- `sdk_generation.rs` — run `picloud sdk generate` against a live cluster. Assert the generated Rust crate compiles (`cargo build`), the TypeScript package compiles (`tsc`), and the .NET package builds (`dotnet build`).
- `sdk_publish.rs` — run `picloud sdk publish` against a live cluster configured with a local test registry. Assert packages appear in the test registry within 5 minutes.
- `sdk_ontology_sync.rs` — add a new resource type to the platform ontology. Re-run `picloud sdk generate`. Assert the new type appears in all three generated SDKs with correct property types.

Exit criteria:
- All three SDKs compile without errors after generation from a live cluster ontology.
- SDK generation completes within 5 minutes on Pi5 hardware.
- Ontology changes reflected in SDK within one generation cycle.

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

**Test coverage:**

Scenario tests:
- `per_slice_build.rs` — for each slice in the workspace, build it independently: `cargo build -p picloud-{slice}`. Assert each compiles without requiring other slices to be present.
- `composition_root_only.rs` — assert that only `picloud-server/src/main.rs` references more than one non-domain slice crate. Any other crate referencing multiple slices is a dependency violation.

Invariants:
- `cargo deny` configuration blocks any `picloud-*` dependency in any slice `Cargo.toml` other than `picloud-domain`. Checked on every PR.

Exit criteria:
- Every slice compiles independently: 100% of slices, verified on every PR.
- Zero cross-slice imports detected: verified by `cargo deny` and `cargo tree` on every PR.

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

**Test coverage:**

Scenario tests:
- `platform_replay_full.rs` — emit 500 known events, record the RDF graph state. Clear Oxigraph. Trigger `picloud cluster replay --from epoch`. Assert the resulting graph is byte-identical to the recorded snapshot.
- `shadow_swap_live_traffic.rs` — trigger a platform replay while the cluster is serving live SPARQL queries (load: 10 queries/second). Assert zero query errors during replay. Assert the shadow swap is atomic — no queries return partial state.
- `replay_marked_flag.rs` — replay 100 events. Inspect the re-emitted events. Assert every replayed event carries `replay.is_replay: true` and a `replay.replay_id` that groups all events from the same replay operation.
- `aggregate_replay.rs` — replay a single aggregate (Photo ID `abc123`) from a product event store. Assert only that aggregate's events are re-emitted. Assert other aggregates are unaffected.

Invariants:
- The live graph is unchanged during replay until the atomic swap. Verified by comparing `SELECT (COUNT(*) AS ?n) WHERE { ?s ?p ?o }` before and during replay (count must not change until the swap).

Exit criteria:
- Full platform replay (10,000 events): < 30 seconds.
- Shadow swap: zero live query errors during replay, verified across 10 replay runs.
- Aggregate-scoped replay: only the target aggregate's events re-emitted, 100% of test runs.

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

**Test coverage:**

Scenario tests:
- `tag_add_event.rs` — add a tag to a resource via `picloud tag add`. Assert a `TagAdded` event appears in the event log with the correct key, value, and resource IRI.
- `tag_rdf_projection.rs` — add a tag. Query the resource IRI via SPARQL. Assert the `picloud:tag` triple with correct `picloud:tagKey` and `picloud:tagValue` is present within the projection latency budget.
- `tag_remove.rs` — remove a tag. Assert `TagRemoved` event in log and tag triple absent from graph within projection latency budget.
- `tag_sparql_queryable.rs` — run `picloud tag find environment=production`. Assert all tagged resources returned. Run the equivalent SPARQL query directly and assert identical results.

Exit criteria:
- Tag added to graph within projection latency budget (p99 < 2 s).
- Tag removal reflected in graph within projection latency budget.
- `picloud tag find` and direct SPARQL query return identical result sets: 100% of test runs.

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

**Test coverage:**

Scenario tests:
- `group_membership_via_inference.rs` — create a user, add tag `team:backend`. Assert a `GroupMembershipChanged` event is emitted and the user appears as `picloud:hasMember` on the `backend-developers` group within one event cycle (< 2 seconds).
- `group_membership_removal.rs` — remove the `team:backend` tag. Assert the membership triple is retracted from the graph and the user's next issued token lacks the `product-developer` role.
- `circular_group_rejection.rs` — attempt to create a group membership rule where group A contains group B and group B contains group A. Assert the platform rejects the cycle at resource apply time.
- `group_role_inheritance.rs` — assign a group to a role, assert all group members receive the role's permissions in their tokens.

Invariants:
- Token issuance always reflects the current group membership from the RDF graph. Any token issued after a `GroupMembershipChanged` event must reflect the new state.

Exit criteria:
- Tag → group membership propagation: < 2 seconds (one event cycle).
- Token reflects new roles within 5 seconds of `GroupMembershipChanged`.
- Circular membership: rejected at apply time, 100% of attempts.

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

**Test coverage:**

Scenario tests:
- `inference_rule_lifecycle.rs` — deploy an `inference-rule` resource. Trigger the condition (inject a matching `MetricRecorded` event). Assert produced triples appear in the graph and the correct assertion event is emitted.
- `inference_retraction.rs` — clear the condition. Assert the produced triples are retracted from the graph and the corresponding resolved event is emitted within 2 seconds.
- `reconciliation_pass.rs` — deliberately skip the triggering event during a 10-minute window. Assert the reconciliation pass fires and the inferred triples appear within 10 minutes ± 30 seconds. Assert `ReconciliationCompleted` event in log.
- `rule_idempotency.rs` — trigger the same inference rule 3 times with the same graph state. Assert only one set of triples is produced — no duplicates.

Exit criteria:
- Event-triggered inference evaluation: < 2 seconds from triggering event to produced triples.
- Reconciliation pass: runs every 10 minutes ± 30 seconds, verified over a 2-hour observation window.
- Idempotency: zero duplicate triples produced across 100 repeated rule evaluations with identical inputs.

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

**Test coverage:**

Scenario tests:
- `rdfs_subclass_inference.rs` — declare `picloud:ProductionContainer rdfs:subClassOf picloud:Container` in an ontology. Query `SELECT ?x WHERE { ?x a picloud:Container }`. Assert instances of `picloud:ProductionContainer` are returned.
- `owl_transitivity.rs` — declare `picloud:dependsOn rdf:type owl:TransitiveProperty`. Assert that if `A dependsOn B` and `B dependsOn C`, then `A dependsOn C` is inferred and queryable.
- `ontology_deploy_immediate.rs` — deploy a product with a new subclass declaration. Assert the inference is materialised and queryable within 5 seconds of `ProductDeployed` event.

Exit criteria:
- RDFS subclass inference active immediately after ontology deployment.
- OWL transitive closure inferred correctly for depth-3 chains.
- Inference materialised within 5 seconds of ontology deployment: 100% of test runs.

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

**Test coverage:**

Scenario tests:
- `metrics_collection_interval.rs` — start a node. Wait 30 seconds. Assert at least 2 `MetricRecorded` events in the log for that node, each containing CPU usage, memory usage, disk usage, and CPU temperature.
- `metrics_rdf_projection.rs` — after a `MetricRecorded` event, query the node IRI via SPARQL. Assert `picloud:cpuUsagePercent`, `picloud:memoryUsedMb`, `picloud:memoryTotalMb`, `picloud:cpuTempCelsius`, and `picloud:metricsUpdatedAt` are present.
- `metrics_upsert.rs` — wait for two consecutive `MetricRecorded` events from the same node. Assert the graph holds only the latest metric values (not a growing list of historical values).

Invariants:
- `MetricRecorded` events emitted every 15 seconds ± 2 seconds from every live node. A gap > 20 seconds from any live node is a test failure.

Exit criteria:
- First `MetricRecorded` within 20 seconds of node join.
- Consistent 15-second interval maintained over a 30-minute observation window: zero gaps > 20 seconds.
- Latest-value-only projection: graph holds exactly one set of metric triples per node, verified after 10 consecutive metric events.

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

**Test coverage:**

Scenario tests:
- `alert_fired.rs` — inject a `MetricRecorded` event with `cpu_temp_celsius: 85.0` (above the 80°C critical threshold). Assert `AlertFired` event emitted within 30 seconds. Assert an `picloud:Alert` triple present in the graph with correct `alertType`, `alertSeverity`, and `alertResource`.
- `alert_resolved.rs` — after `AlertFired`, inject a subsequent `MetricRecorded` event with `cpu_temp_celsius: 65.0` (below threshold). Assert `AlertResolved` event emitted and `picloud:Alert` triple retracted within 30 seconds.
- `alert_dampening.rs` — fire an alert, resolve it, re-fire within 60 seconds. Assert the second `AlertFired` is suppressed (dampening window enforced). Wait 60 seconds, re-trigger. Assert `AlertFired` now emitted.
- `all_builtin_rules.rs` — for each built-in alert rule (CPU temp, memory, disk, node unreachable, workload failed), trigger the threshold condition and assert the correct `AlertFired` event type and severity.

Exit criteria:
- `AlertFired` within 30 seconds of threshold breach across all built-in rules.
- `AlertResolved` within 30 seconds of threshold clearing.
- Dampening: second `AlertFired` within 60-second window suppressed, 100% of test runs.

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

**Test coverage:**

Scenario tests:
- `dual_cluster_mDNS_isolation.rs` — init two clusters on the same network with different domains (`picloud.local` and `lab.local`). Assert that nodes from cluster A do not appear in cluster B's node list (SPARQL query), and vice versa.
- `cross_cluster_join_rejection.rs` — generate an enrollment token from cluster A. Attempt to use it to join cluster B. Assert a `NodeEnrollmentRejected` event in cluster B's log and the node is not added to cluster B's Raft.
- `iri_namespace_uniqueness.rs` — assert that every resource IRI in cluster A contains `picloud.local` and no IRI contains `lab.local`, and vice versa.

Invariants:
- Zero cross-cluster node discovery in a 5-minute observation window with two clusters on the same network.
- Cross-cluster join always results in `NodeEnrollmentRejected`, never `NodeEnrolled`.

Exit criteria:
- Cross-cluster isolation: zero false discoveries across 50 mDNS query cycles.
- Cross-cluster join: rejected 100% of 20 attempts.

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

**Test coverage:**

Scenario tests:
- `config_api_lifecycle.rs` — apply a `config` resource with 5 entries. GET each entry via the HTTP API. Assert correct key, value, and type for each.
- `config_live_reload.rs` — update a config entry via the API. Assert `ConfigChanged` event emitted. Assert the workload SDK reflects the new value within 5 seconds without a process restart.
- `workload_config_override.rs` — declare a product-level config entry and a workload-level override for the same key. Assert the workload's effective config (via the merged endpoint) returns the workload value, not the product value.
- `config_secret_separation.rs` — assert that secret values are never stored in the config store. Attempt to set a config entry with the key `password`. Assert the platform rejects any config key flagged as sensitive.

Exit criteria:
- `ConfigChanged` event delivered to workload SDK within 5 seconds, no workload restart required.
- Workload-level override: effective config resolution correct in 100% of test runs.

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

**Test coverage:**

Scenario tests:
- `flag_version_evaluation.rs` — deploy flag `new-upload-flow` with `version: = 2`. Deploy workload at version 2. Assert flag evaluates as active. Deploy another workload at version 1. Assert flag evaluates as inactive.
- `flag_live_update.rs` — toggle a flag from `enabled: true` to `enabled: false` via `resource apply`. Assert `FeatureFlagChanged` event emitted and SDK reflects the new state within 5 seconds without workload restart.
- `flag_version_range.rs` — deploy flag with `version: 2..4`. Assert active for versions 2, 3, 4 and inactive for versions 1 and 5.
- `flag_in_process_evaluation.rs` — after SDK initialisation, measure flag evaluation latency. Assert all evaluations are in-process (zero network round-trips) after initial load.

Exit criteria:
- Flag evaluation latency after SDK init: < 1 ms (in-process cache, no network call).
- `FeatureFlagChanged` propagation to SDK: < 5 seconds.
- Version range evaluation correct across all six operators: 100% of test cases.

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

**Test coverage:**

Scenario tests:
- `otlp_trace_ingestion.rs` — POST a valid OTLP trace payload to `https://picloud.local/otel/v1/traces`. Assert 200. Assert the trace appears in the Parquet time-series store within 30 seconds (verified via DataFusion query).
- `cli_trace_propagation.rs` — run `picloud resource apply`. Query the Parquet store for the trace ID from the CLI output. Assert end-to-end spans: CLI root → Raft append → RDF projection → workload start.
- `otel_does_not_starve_raft.rs` — generate 10,000 OTel spans per second for 60 seconds. During this burst, measure Raft append latency. Assert Raft p99 append latency does not increase by more than 20% compared to baseline.

Protocol probes:
- OTLP/HTTP: POST with `Content-Type: application/x-protobuf` → assert 200. POST with wrong `Content-Type` → assert 415. POST with invalid protobuf → assert 400.

Exit criteria:
- OTLP ingestion latency: < 5 ms p99.
- Traces appear in Parquet within 30 seconds of ingestion.
- OTel burst does not degrade Raft append latency beyond 20%: verified across 3 burst runs.

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

**Test coverage:**

Scenario tests:
- `parquet_write_read.rs` — ingest 1,000 OTel spans via the OTLP endpoint. Wait for Parquet flush. Run a DataFusion SQL query: `SELECT COUNT(*) FROM traces WHERE service_name = 'test-service'`. Assert count = 1000.
- `retention_enforcement.rs` — write Parquet partitions with timestamps older than the configured retention window. Run the hourly retention cleanup task. Assert old partition directories are deleted and newer ones remain.
- `datafusion_time_range.rs` — query traces for a known 1-hour window using `WHERE start_time BETWEEN ? AND ?`. Assert only traces within the window are returned. Measure query time on 7 days of data.
- `parquet_portability.rs` — copy a Parquet file off-node. Open it with `pyarrow` on an external machine. Assert the schema and data are readable without any PiCloud tools.

Exit criteria:
- 1,000-span write and flush: < 2 seconds.
- DataFusion query over 7-day trace window on Pi5: < 5 seconds.
- Retention cleanup: old partitions deleted within 5 minutes of the hourly cleanup tick.

---

## ADR-047: Volume Snapshots and Offsite Backup as Storage Intent Primitives

**Status:** Accepted

**Context:** Replication across cluster nodes (ADR-013, ADR-024) protects against hardware failure. It does not protect against accidental deletion, data corruption, logical failures, or physical disasters affecting all nodes simultaneously (fire, flood, theft). For irreplaceable data — family photos, personal documents, application state — point-in-time snapshots and offsite backup are essential additional layers.

**The three failure scenarios and their mitigations:**

| Scenario | Replication | Snapshots | Offsite |
|---|---|---|---|
| Node hardware failure | ✓ | ✓ | ✓ |
| Accidental deletion | ✗ | ✓ | ✓ |
| Data corruption / bug | ✗ | ✓ | ✓ |
| Total cluster loss (fire/flood/theft) | ✗ | ✗ | ✓ |

**Decision:** Volume snapshots and offsite backup are first-class storage intent primitives declared in the volume resource definition. Snapshots are stored on a local NAS (fast recovery). Offsite backup targets S3-compatible endpoints (disaster recovery). Both are configured declaratively — the platform manages scheduling, retention, and transfer.

### Volume declaration with snapshots and backup

```bicep
volume 'family-photos' = {
  product: 'photo-app'
  size: '500GB'
  storageIntent: {
    durability:  'full-replication'
    performance: 'standard'
    snapshots: {
      enabled:  true
      schedule: 'daily'           // hourly | daily | weekly
      storage:  secret('nas-snapshot-config')
      retention: {
        daily:   30               // keep 30 daily snapshots
        weekly:  26               // keep 26 weekly snapshots
        monthly: 0                // 0 = keep forever
      }
    }
    offsite: {
      enabled:   true
      target:    secret('s3-backup-config')
      frequency: 'daily'          // daily | weekly
      encryption: true            // always encrypt before upload
    }
  }
}
```

### Snapshot storage — local NAS

Snapshots are point-in-time, immutable copies of a volume stored on a local NAS. The NAS is referenced via a secret containing connection details (NFS mount path or SMB share). Snapshots are not stored on cluster NVMe — this preserves the full NVMe capacity for live data.

**Snapshot secret format:**
```json
{
  "type":   "nfs",
  "host":   "192.168.1.200",
  "path":   "/volume1/picloud-snapshots",
  "options": "vers=4,rsize=1048576,wsize=1048576"
}
```

**Snapshot naming convention:**
```
{volume-name}/{product}/{date}T{time}Z.snapshot
family-photos/photo-app/2025-07-01T02:00:00Z.snapshot
```

**Snapshot schedule:**
The platform runs a snapshot job according to the declared schedule. Snapshots are crash-consistent — the volume is quiesced briefly during the snapshot operation. The snapshot job emits `SnapshotCreated` and `SnapshotFailed` events.

**Retention enforcement:**
After each snapshot, the platform evaluates retention policy and deletes snapshots outside the policy window. Deletion emits `SnapshotDeleted` events. The retention policy is evaluated per category:
- `daily: 30` — keep the most recent 30 daily snapshots
- `weekly: 26` — keep the most recent 26 weekly snapshots (Sunday snapshots are promoted to weekly)
- `monthly: 0` — keep all monthly snapshots forever (first snapshot of each month promoted to monthly)

**Recovery:**
```bash
# List available snapshots for a volume
picloud volume snapshots family-photos

# Restore a volume to a point in time
picloud volume restore family-photos \
  --snapshot "2025-07-01T02:00:00Z" \
  --target family-photos-restored
```

### Offsite backup — S3-compatible endpoint

Offsite backup uploads encrypted volume data to any S3-compatible endpoint. Recommended providers for home use: Backblaze B2 (cheapest per GB), Cloudflare R2 (no egress fees), or a self-hosted MinIO instance at a family member's location.

**S3 backup secret format:**
```json
{
  "type":     "s3",
  "endpoint": "https://s3.us-west-000.backblazeb2.com",
  "bucket":   "picloud-backup-emil",
  "region":   "us-west-000",
  "access_key_id":     "...",
  "secret_access_key": "..."
}
```

**Encryption:** All data is encrypted client-side before upload using a platform-managed key stored in the cluster's secret store. The S3 provider never sees plaintext data. The encryption key is itself backed up to the NAS snapshot store — losing the key means losing the backup.

**Backup format:** Volumes are uploaded as chunked, deduplicated, compressed archives. Chunks that have not changed since the last backup are not re-uploaded. This makes incremental backups efficient even for large volumes.

**Backup schedule:**
The platform runs offsite backup jobs according to the declared frequency. Backup jobs emit `BackupStarted`, `BackupCompleted`, and `BackupFailed` events. Backup duration and bytes transferred are recorded in the platform metrics (ADR-040) and queryable via the RDF graph.

**Recovery from offsite:**
```bash
# List available offsite backups
picloud volume backups family-photos --offsite

# Restore from offsite (slower — downloads from S3)
picloud volume restore family-photos \
  --offsite \
  --date "2025-07-01" \
  --target family-photos-restored
```

### Snapshot and backup status in the RDF graph

Current snapshot and backup state is projected into the RDF graph:

```turtle
<https://picloud.local/products/photo-app/volumes/family-photos>
    a picloud:Volume ;
    picloud:lastSnapshotAt     "2025-07-01T02:00:00Z"^^xsd:dateTime ;
    picloud:lastSnapshotStatus "success" ;
    picloud:snapshotCount      47 ;
    picloud:lastBackupAt       "2025-07-01T03:00:00Z"^^xsd:dateTime ;
    picloud:lastBackupStatus   "success" ;
    picloud:lastBackupSizeGb   312.4 .
```

This means alert rules can fire on backup failures:

```bicep
inference-rule 'backup-failed-alert' = {
  scope: 'platform'
  trigger: 'event'
  trigger-events: ['BackupFailed']
  construct: '''
    CONSTRUCT {
      ?volume a picloud:Alert ;
              picloud:alertType     "BackupFailed" ;
              picloud:alertSeverity "critical" ;
              picloud:alertMessage  "Offsite backup failed — data at risk" ;
              picloud:alertResource ?volume .
    }
    WHERE {
      ?volume a picloud:Volume ;
              picloud:lastBackupStatus "failed" .
    }
  '''
}
```

**Rationale:**
- Snapshots and backup are declared in the volume resource — versioned, auditable, consistent with IaC-as-only-interface (ADR-010)
- NAS for snapshots keeps recovery fast and local — no internet dependency for common recovery scenarios
- S3 for offsite keeps disaster recovery simple — any S3-compatible provider works, including self-hosted
- Client-side encryption before upload means the backup is secure regardless of provider security posture
- Separating snapshot storage from cluster NVMe preserves full cluster storage capacity for live data
- Backup failures emit events and fire alert rules — operators are notified before they discover data loss the hard way
- Secrets for NAS and S3 credentials follow the existing secret injection model (ADR-009) — no new credential management needed

**Consequences:**
- `picloud-storage` gains NFS/SMB mount capability for snapshot storage
- `picloud-storage` gains an S3-compatible client (`aws-sdk-s3` or `opendal` crate) for offsite backup
- The encryption key for S3 backups must be backed up — losing it means losing all offsite backups. The platform should warn loudly if the encryption key has no backup.
- Snapshot quiescing requires coordination with the workload — containers receive `SIGTSTP` during snapshot, `SIGCONT` after. Duration should be milliseconds.
- A volume with both snapshots and offsite backup enabled uses three storage locations: cluster NVMe (live), NAS (snapshots), S3 (offsite). All three are declared in one resource definition.

**Test coverage:**

Scenario tests:
- `snapshot_create_verify.rs` — declare a volume with daily snapshots and a NAS target. Trigger a snapshot (via `picloud volume snapshot now`). Assert `SnapshotCreated` event emitted and the snapshot file is present on the NAS at the expected path.
- `snapshot_restore.rs` — write a known sentinel file to a volume. Take a snapshot. Overwrite the sentinel. Restore from snapshot. Assert the original sentinel is present.
- `snapshot_retention.rs` — take 35 daily snapshots (accelerated in CI with a short schedule). Run the retention enforcement. Assert exactly 30 daily snapshots remain (retention policy: `daily: 30`).
- `offsite_backup_complete.rs` — declare a volume with S3 offsite backup. Trigger a backup. Assert `BackupCompleted` event, backup metadata in RDF graph, and backup object present in the configured S3 bucket.
- `backup_failure_alert.rs` — configure an invalid S3 endpoint. Trigger a backup. Assert `BackupFailed` event and `AlertFired` (type: `BackupFailed`, severity: `critical`) within 30 seconds.

Exit criteria:
- Snapshot completes within 5 seconds of scheduled time.
- Snapshot restore completes within 60 seconds for a 10 GB volume.
- `BackupFailed` → `AlertFired` within 30 seconds.
- Retention policy enforcement: exact snapshot count matches policy after 35-snapshot test.

---

## ADR-048: Native Ingress Router in picloud-http

**Status:** Accepted

**Context:** Applications built on PiCloud need HTTP routing — TLS termination, hostname-based routing, path-based routing, and port multiplexing — without depending on nginx, traefik, or any external reverse proxy. PiCloud owns the full stack: every workload, every certificate, every IRI. This means the routing table is the RDF graph, certificates come from the platform CA, and routing updates are events — not config file reloads.

**Decision:** `picloud-http` implements a native ingress router using `hyper` (already in the stack) for proxying and `rustls` (already in the stack) for TLS termination. No external proxy dependency. The router's state is rebuilt from Oxigraph on every `IngressCreated`, `IngressUpdated`, and `IngressDeleted` event. Internal ports are routed over the cluster mTLS mesh and never exposed externally.

### Why this is simpler than nginx/traefik

nginx and traefik solve routing for arbitrary external infrastructure they do not control. PiCloud controls everything — workload addresses, certificates, and routing intent are all platform state. This eliminates the hard parts:

| nginx/traefik concern | PiCloud answer |
|---|---|
| Dynamic config reload | Events update the routing table live — no config files |
| SSL certificate management | Platform CA issues all certs (ADR-030) |
| Upstream discovery | Scheduler knows every container's node and port |
| Load balancing | One upstream per ingress — scheduler handles placement |
| Access logs / metrics | OTel handles everything (ADR-045) |
| Multiple upstreams | Not needed — containers are scheduled, not pooled |

### Router state

The router maintains an in-memory routing table rebuilt from Oxigraph on ingress resource events:

```rust
/// The complete router state — rebuilt from RDF graph on every ingress event.
/// Lookups are O(1) — HashMap keyed by (host, internal) then matched by path prefix.
pub struct IngressRouter {
    /// External routes — TLS terminated, publicly reachable
    external: HashMap<String, Vec<RouteEntry>>,   // keyed by hostname
    /// Internal routes — mTLS mesh only, not externally reachable
    internal: Vec<RouteEntry>,
    /// TLS config per hostname — SNI-based certificate selection
    tls:      Arc<rustls::ServerConfig>,
}

pub struct RouteEntry {
    pub path_prefix:  String,
    pub upstream:     Upstream,
    pub product:      String,
    pub workload_iri: ResourceIri,
}

pub struct Upstream {
    /// Internal cluster address — known from scheduler state in RDF graph
    pub address: String,
    pub port:    u16,
    /// mTLS client cert for internal upstream connections
    pub client_cert: Arc<rustls::ClientConfig>,
}
```

### Request lifecycle

```
Client → TLS handshake (SNI hostname extracted)
       → Route lookup: hostname → path prefix match → Upstream
       → Proxy request via hyper client (mTLS to upstream)
       → Stream response back to client
       → OTel span closed with status and duration
```

### Routing rules

**Host-based routing** — `photos.picloud.local` routes to the `web-frontend` container:
```bicep
ingress 'photos-web' = {
  product: 'photo-app'
  target:  'web-frontend'
  port:    3000
  host:    'photos.picloud.local'
  tls:     true
}
```

**Path-based routing** — automatic for all platform resources under `picloud.local/products/...`. No ingress resource needed.

**Internal ports** — exposed only within the cluster mTLS mesh:
```bicep
ingress 'api-metrics' = {
  product:  'photo-app'
  target:   'api-server'
  port:     9090
  internal: true           // mTLS mesh only — never externally reachable
}
```

**Multiple ingresses per container** — each port gets its own ingress resource. The platform registers each independently.

### TLS — SNI-based certificate selection

Every hostname declared in an ingress resource gets a TLS certificate issued by the platform CA. The router uses SNI to select the correct certificate per connection. Certificate issuance happens at `IngressCreated` time — the router never serves a request without a valid certificate.

```rust
// SNI resolver — selects certificate based on hostname in TLS handshake
impl rustls::server::ResolvesServerCert for SniResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        let hostname = client_hello.server_name()?;
        self.certs.get(hostname).cloned()
    }
}
```

### Live routing updates — event-driven

The router subscribes to the platform event stream. On relevant events it rebuilds the affected route entries from Oxigraph:

```rust
match event.event_type.as_str() {
    "IngressCreated" | "IngressUpdated" => {
        let upstream = graph.query_upstream(&event.payload)?;
        router.upsert_route(upstream);
        tls.issue_cert_if_needed(&upstream.host);
    }
    "IngressDeleted" => {
        router.remove_route(&event.payload.ingress_iri);
    }
    "WorkloadRescheduled" => {
        // Container moved to a different node — update upstream address
        router.update_upstream_address(&event.payload);
    }
    _ => {}
}
```

No config reload, no process restart. The routing table is always consistent with the RDF graph.

### Connection draining (Phase 4)

In Phase 1, when a workload reschedules, existing connections are closed and clients retry. Phase 4 adds graceful draining:
- On `WorkloadRescheduling` event, mark upstream as draining — accept no new connections
- Allow in-flight requests up to 30 seconds to complete
- On `WorkloadRescheduled` event, update upstream address, resume routing

### Implementation size

The complete ingress router fits in approximately 500 lines across three files in `picloud-http`:

```
picloud-http/src/
├── router.rs       ~200 lines  — RouteTable, lookup, upsert, remove
├── proxy.rs        ~150 lines  — hyper reverse proxy, request forwarding
├── tls.rs          ~100 lines  — SNI resolver, cert issuance, rustls config
└── ingress.rs      ~50  lines  — event subscription, router update handler
```

**Rationale:**
- No external proxy dependency — consistent with single-binary goal (ADR-001)
- The routing table is the RDF graph — no separate config format, no drift between declared intent and runtime state
- Event-driven updates mean routing is always consistent with workload state — when a container starts, it is immediately routable
- SNI-based certificate selection handles multiple hostnames on a single port cleanly
- Internal port isolation via `internal: true` solves the metrics/health/debug port exposure problem without firewall rules
- hyper and rustls are already in the dependency stack — zero new dependencies required
- ~500 lines is a well-understood, testable surface area — not a framework, just a router

**Consequences:**
- `picloud-http` gains `router.rs`, `proxy.rs`, `tls.rs`, `ingress.rs`
- The router must handle the case where an upstream is temporarily unreachable (container restarting) — return 503 with a `Retry-After` header
- WebSocket and HTTP/2 proxying require explicit support in the hyper proxy layer — add in Phase 2
- The router runs on every node — requests are handled locally where possible, forwarded to the correct node when the target container runs elsewhere

**Test coverage:**

Scenario tests:
- `ingress_host_routing.rs` — declare an ingress resource with `host: photos.picloud.local`. Send an HTTP request to that host. Assert it is routed to the correct container and the correct response is returned.
- `workload_reschedule_routing.rs` — reschedule the container targeted by an ingress to a different node. Assert HTTP requests continue succeeding within 30 seconds of the `WorkloadRescheduled` event (routing table updated without manual intervention).
- `internal_port_isolation.rs` — declare an ingress with `internal: true`. From an external client, attempt to connect to the ingress hostname. Assert connection refused. From a workload inside the cluster, assert connection succeeds.
- `sni_cert_selection.rs` — declare two ingresses for two different hostnames. Connect to each hostname. Assert each connection receives the correct TLS certificate (SNI-based selection working).

Protocol probes:
- TLS SNI: assert `photos.picloud.local` and `api.picloud.local` receive different certificates, both issued by the platform CA.
- Assert 503 with `Retry-After` header when upstream container is restarting (not a hard failure).

Exit criteria:
- Routing update after workload reschedule: < 30 seconds, verified across 10 reschedule cycles.
- Internal port: unreachable from external client, 100% of attempts.
- SNI certificate selection: correct cert per hostname, 100% of TLS handshakes.

---

## ADR-049: .picloud Format as Compiler Target — Turtle as Canonical IaC

**Status:** Accepted  
**Supersedes:** ADR-007 (Bicep-inspired syntax — retained as the `.picloud` surface format only)

**Context:** ADR-007 established a Bicep-inspired `.picloud` syntax for resource definitions. The platform already uses RDF/Turtle natively for all state, ontologies, and event schemas. Introducing a custom language with a custom parser and custom validation creates a gap between how resources are *declared* and how they are *stored and queried*. The platform ontology and SHACL shapes already define every resource type and its constraints — they should be the single source of truth for validation, documentation, and tooling as well.

**Decision:** `.picloud` files are a developer-friendly surface format that compiles to Turtle. The platform only ever sees Turtle. Both `.picloud` and `.ttl` files are accepted by the CLI. The platform ontology and SHACL shapes are the canonical type system — all validation, documentation, and SDK generation derive from them. No blank nodes are produced in compiled output — every nested structure gets a stable, dereferenceable IRI generated by the compiler.

### The pipeline

```
Developer workflow:
  .picloud files  ←  human-authored, checked into version control
       ↓
  picloud-compiler
       ↓  validates each file against platform ontology (offline)
       ↓  generates stable IRIs for all nested structures (no blank nodes)
       ↓  merges all files into one deployment graph
       ↓  SHACL validates merged graph
       ↓  optional: validates cross-resource refs against live cluster
       ↓
  deployment.ttl  ←  canonical, submitted to cluster
       ↓
  picloud resource apply
```

### .picloud format (surface syntax)

The `.picloud` format is a simplified, readable surface over Turtle. The compiler translates it to valid Turtle with stable IRIs.

```
// photo-app/resources.picloud

product "photo-app" {
  version     = "1.0.0"
  description = "Photo sharing application"
}

volume "media-store" {
  product    = "photo-app"
  size       = "500GB"
  durability = "full-replication"

  snapshots {
    enabled  = true
    schedule = "daily"
    storage  = secret("nas-config")
    retention {
      daily   = 30
      weekly  = 26
      monthly = 0
    }
  }

  offsite {
    enabled    = true
    target     = secret("backblaze-config")
    frequency  = "daily"
    encryption = true
  }
}

container "api-server" {
  product  = "photo-app"
  image    = "photo-api:1.0.0"
  identity = "api-worker"

  mount {
    volume = "media-store"
    path   = "/data"
  }

  tag { key = "team";        value = "backend"     }
  tag { key = "environment"; value = "production"  }
}

feature-flag "new-upload-flow" {
  product     = "photo-app"
  description = "Redesigned upload flow"
  enabled     = true
  version     = ">= 2"
}
```

### Compiled Turtle output — no blank nodes

The compiler produces a merged `deployment.ttl` with stable IRIs for every nested structure:

```turtle
@prefix pc:  <https://picloud.local/ontology#> .
@prefix app: <https://picloud.local/products/photo-app/> .

# Product
app:
    a pc:Product ;
    pc:version     "1.0.0" ;
    pc:description "Photo sharing application" .

# Volume
app:volumes/media-store
    a pc:Volume ;
    pc:product  app: ;
    pc:sizeGb   500 ;
    pc:durability pc:FullReplication ;
    pc:snapshots  app:volumes/media-store/snapshots ;
    pc:offsite    app:volumes/media-store/offsite .

# Snapshot config — stable IRI, not a blank node
app:volumes/media-store/snapshots
    a pc:SnapshotConfig ;
    pc:enabled       true ;
    pc:schedule      pc:Daily ;
    pc:storageSecret "nas-config" ;
    pc:retention     app:volumes/media-store/snapshots/retention .

app:volumes/media-store/snapshots/retention
    a pc:SnapshotRetention ;
    pc:dailyCount   30 ;
    pc:weeklyCount  26 ;
    pc:monthlyCount 0 .

# Container
app:containers/api-server
    a pc:Container ;
    pc:product  app: ;
    pc:image    "photo-api:1.0.0" ;
    pc:identity app:identities/api-worker ;
    pc:mount    app:containers/api-server/mounts/media-store ;
    pc:tag      app:containers/api-server/tags/team ;
    pc:tag      app:containers/api-server/tags/environment .

# Mount — stable IRI
app:containers/api-server/mounts/media-store
    a pc:VolumeMount ;
    pc:volume app:volumes/media-store ;
    pc:path   "/data" .

# Tags — stable IRIs
app:containers/api-server/tags/team
    a pc:Tag ;
    pc:key   "team" ;
    pc:value "backend" .

app:containers/api-server/tags/environment
    a pc:Tag ;
    pc:key   "environment" ;
    pc:value "production" .

# Feature flag
app:flags/new-upload-flow
    a pc:FeatureFlag ;
    pc:product      app: ;
    pc:description  "Redesigned upload flow" ;
    pc:enabled      true ;
    pc:versionExpr  ">= 2" .
```

### IRI generation rules for nested structures

The compiler generates nested IRIs deterministically from the parent IRI and property name:

```
{parent-iri}/{property-name}             for singleton nested objects
{parent-iri}/{property-name}/{key}       for keyed collections (tags, mounts)
{parent-iri}/{property-name}/{index}     for ordered lists
```

This means compiled output is deterministic — the same `.picloud` files always produce the same IRIs. Diffs are meaningful. SPARQL queries against nested structures always work.

### Validation — two modes

**Offline validation** (no cluster required):
```bash
picloud resource validate ./photo-app/
```
- Parses `.picloud` and `.ttl` files
- Validates each file against the platform ontology
- Merges into deployment graph
- Runs SHACL validation against merged graph
- Reports human-readable errors (translated from SHACL violations)

**Online validation** (requires live cluster):
```bash
picloud resource validate ./photo-app/ --online
```
- Everything in offline mode, plus:
- Validates cross-resource references against live cluster state
- Checks referenced secrets exist
- Checks referenced identities exist
- Warns on version conflicts with currently deployed product

**Human-readable error translation:**

SHACL violations are translated to developer-friendly messages:

| SHACL violation | Human-readable message |
|---|---|
| `sh:minCount` on `pc:image` | `Container 'api-server': required property 'image' is missing` |
| `sh:datatype` on `pc:sizeGb` | `Volume 'media-store': 'size' must be a number in GB` |
| `sh:in` on `pc:durability` | `Volume 'media-store': 'durability' must be one of: full-replication, quorum, local, none` |
| `sh:pattern` on `pc:versionExpr` | `FeatureFlag 'new-upload-flow': 'version' must be a valid expression (e.g. '>= 2', '2..4')` |

### Documentation generation

```bash
picloud docs generate --format markdown  # → docs/
picloud docs generate --format jsonschema # → schema.json
picloud docs generate --format openapi   # → openapi.yaml
```

All three derive from the same SHACL shapes and platform ontology. Adding a new resource type to the ontology automatically updates all three documentation formats on the next generation pass.

**Markdown output** — one page per resource type, property table with types and constraints, examples in both `.picloud` and Turtle.

**JSON Schema output** — one schema per resource type, suitable for IDE validation plugins and code generation.

**OpenAPI output** — describes the platform HTTP API surface derived from the resource types, suitable for client generation in any language.

### Version control workflow

`.picloud` files are checked into version control. The compiled `deployment.ttl` is generated at deploy time — not checked in. This keeps the repository clean while maintaining Turtle as the canonical format.

```
photo-app/
├── resources.picloud        ✓ checked in — human-authored
├── schemas/
│   ├── photo-events.ttl     ✓ checked in — event schemas
│   └── album-events.ttl     ✓ checked in — event schemas
└── ontology/
    └── photo-app.ttl        ✓ checked in — domain ontology
```

### The new slice: picloud-compiler

A new crate handles compilation, validation, and documentation generation:

```
picloud-compiler/
├── src/
│   ├── parser.rs      — .picloud surface format parser
│   ├── compiler.rs    — .picloud → Turtle with stable IRI generation
│   ├── validator.rs   — SHACL validation + human-readable error translation
│   ├── merger.rs      — merges multiple files into one deployment graph
│   ├── docs.rs        — Markdown / JSON Schema / OpenAPI generation
│   └── lib.rs
```

**Rationale:**
- The platform ontology is already the type system — SHACL validation is not an addition, it is the removal of a parallel custom validation layer
- Turtle as canonical format means resource declarations and resource state use the same data model — no impedance mismatch
- Named IRIs for all nested structures means every part of a resource definition is dereferenceable and SPARQL-queryable — blank nodes are not
- Documentation and JSON Schema generated from SHACL shapes means the docs are always accurate — they cannot drift from the actual validation rules
- `.picloud` surface format preserves developer ergonomics without requiring every developer to learn Turtle
- Offline validation means CI/CD pipelines can validate without cluster access
- Both `.picloud` and `.ttl` accepted means power users and LLMs can write Turtle directly when appropriate

**Consequences:**
- `picloud-compiler` is a new crate added to the workspace (depends only on `picloud-domain`)
- ADR-007 is partially superseded — the `.picloud` syntax remains but is now a compiler input, not the platform's native format
- The platform ontology (`platform.ttl`) becomes the most important file in the repository — it defines every valid resource type and constraint
- All resource type additions require updating the platform ontology and SHACL shapes before implementation
- The compiler's IRI generation rules must be stable — changing them would break existing deployments

**Test coverage:**

Scenario tests:
- `compiler_roundtrip.rs` — compile a representative set of `.picloud` files covering all resource types. Assert the output is valid Turtle (parseable by an RDF library), passes SHACL validation, and contains zero blank nodes.
- `iri_determinism.rs` — compile the same `.picloud` file twice. Assert the two compiled Turtle outputs are byte-identical (deterministic IRI generation).
- `shacl_validation_errors.rs` — submit `.picloud` files with deliberate violations (missing required field, wrong type, invalid version expression). Assert each returns a human-readable error message matching the SHACL violation translation table.
- `offline_validation.rs` — run `picloud resource validate` on a valid deployment with no cluster connection. Assert exit code 0 and zero errors.

Exit criteria:
- Offline validation of any `.picloud` file: < 1 second.
- IRI generation deterministic: same output across 1000 compilation runs of the same input.
- SHACL validation errors: human-readable message returned for 100% of deliberate violations.
- Zero blank nodes in compiled Turtle: verified on full resource type corpus.

---

## ADR-050: Builder Pattern CLI for Resource Generation

**Status:** Accepted

**Context:** Developers need to create new `.picloud` resource files without memorising syntax. The platform ontology and SHACL shapes define every valid resource type and property — the CLI can use this knowledge to guide developers interactively and generate valid files automatically.

**Decision:** `picloud new {resource-type}` generates a `.picloud` file for a new resource. It accepts flags for all properties — fully specified invocations produce the file with no prompts. Partially specified invocations prompt for required fields only. After generation, `picloud compile validate` runs automatically. Generated files are never overwritten unless `--overwrite` is specified.

**Behaviour:**

```bash
# Fully specified — no prompts, CI/CD friendly
picloud new container \
  --product photo-app \
  --name api-server \
  --image photo-api:1.0.0 \
  --identity api-worker \
  --mount media-store:/data \
  --tag team=backend \
  --tag environment=production \
  --output ./photo-app/containers/api-server.picloud

# Partially specified — prompts for missing required fields only
picloud new container --product photo-app
? Container name: api-server
? Image: photo-api:1.0.0
? Workload identity: api-worker
✓ Generated: ./containers/api-server.picloud
✓ Validation passed

# Overwrite existing file
picloud new container --product photo-app --name api-server --overwrite
```

**Supported resource types:**
`product`, `container`, `binary`, `volume`, `feature-flag`, `config`, `inference-rule`, `event-store`, `rdf-store`, `ingress`, `group`, `event-subscription`, `ontology`

**Output flag:** `--output` specifies the file path. If omitted, defaults to `./{resource-type}s/{name}.picloud` relative to the current directory.

**Overwrite protection:** If the output file already exists and `--overwrite` is not set, the CLI refuses with a clear error. This prevents accidental overwrite of hand-edited files.

**Post-generation validation:** After writing the file, `picloud compile validate` runs automatically against the generated file. If validation fails (e.g. a referenced volume does not exist in the same directory), the error is reported with the human-readable messages from ADR-049.

**Flag naming:** All flags match the `.picloud` property names exactly — `--image`, `--identity`, `--mount`, `--tag`. This makes the CLI self-documenting and consistent with the resource files developers read and edit.

**Rationale:**
- Flags-first means CI/CD pipelines and LLMs can use `picloud new` non-interactively
- Interactive fallback for required fields means humans get guidance without remembering syntax
- Auto-validation closes the feedback loop — the developer knows the file is valid immediately
- Overwrite protection prevents accidental data loss on hand-edited files
- Flag names matching property names means one mental model for CLI and file format
- Generated files are plain `.picloud` text — developers can open and edit them immediately

**Consequences:**
- `picloud new` is implemented in `picloud-cli` using the `picloud-compiler` crate for generation and validation
- The builder must know which fields are required vs optional for each resource type — derived from SHACL `sh:minCount` constraints
- The interactive prompt library must handle Ctrl+C gracefully and not leave partial files

**Test coverage:**

Scenario tests:
- `new_resource_flags.rs` — run `picloud new container` with all required flags specified. Assert a `.picloud` file is generated, is valid (auto-validation passes), and the content matches the specified flags.
- `new_resource_partial.rs` — run `picloud new container --product photo-app` without other required flags. Assert the CLI prompts for missing required fields only. Provide values. Assert a valid file is generated.
- `overwrite_protection.rs` — generate a file. Run `picloud new container` targeting the same output path without `--overwrite`. Assert the CLI refuses with a clear error and the original file is unchanged.
- `auto_validation_failure.rs` — generate a resource that references a non-existent volume (deliberate). Assert post-generation validation reports the cross-reference error clearly.

Exit criteria:
- Fully-specified `picloud new`: generates a valid file with zero prompts, 100% of test runs.
- Overwrite protection: original file unchanged in 100% of overwrite-without-flag attempts.
- Post-generation validation: always runs and reports errors before the user can accidentally apply an invalid file.

---

## ADR-051: Product IAM — Roles, Custom Claims, Scopes, and Audience

**Status:** Accepted

**Context:** Products act as OIDC App Registrations (ADR-017). A token issued by the platform for a product must carry the roles, permissions, and custom claims specific to that product. Without roles and scopes, the token is structurally valid but semantically empty. Without audience validation, tokens can be reused across products. Four capabilities are needed: role definitions with inheritance, custom static claims, product-defined OAuth scopes, and audience-bound tokens.

**Decision:** Products declare roles, scopes, and custom claims as resources in their `.picloud` files. The platform IAM engine resolves roles (including inheritance via OWL subclass inference), evaluates scope-to-claim mappings, and issues tokens with product-scoped audience. Three token flows are supported: user authentication, on-behalf-of (user delegating to a product acting against another product), and M2M client credentials.

### Token anatomy

Every token issued for a product carries:

```json
{
  "iss": "https://picloud.local",
  "aud": "https://picloud.local/products/photo-app",
  "sub": "https://picloud.local/platform/identities/alice",
  "exp": 1735689600,
  "iat": 1735686000,
  "scope": "photos:read photos:write",
  "roles": ["editor"],
  "permissions": ["photos:read", "photos:write", "albums:manage"],
  "department": "engineering"
}
```

- `iss` — always the platform IRI (cluster domain)
- `aud` — the product IRI. A token for `photo-app` is rejected by `user-service`
- `sub` — the user's platform identity IRI
- `scope` — space-separated OAuth scopes granted in this token
- `roles` — product roles assigned to this user
- `permissions` — flattened permission set from all assigned roles
- Custom claims — static key-value pairs declared on roles or scopes

### Role declaration

```bicep
role "viewer" = {
  product:     "photo-app"
  description: "Can view photos and albums"
  permissions: [
    "photos:read"
    "albums:read"
  ]
  claims: {
    "access_level": "read-only"
  }
}

role "editor" = {
  product:     "photo-app"
  description: "Can view and manage photos"
  inherits:    "viewer"        // inherits all viewer permissions and claims
  permissions: [
    "photos:write"
    "albums:manage"
  ]
  claims: {
    "access_level": "read-write"
  }
}

role "admin" = {
  product:     "photo-app"
  description: "Full product access"
  inherits:    "editor"        // transitive — inherits viewer and editor
  permissions: [
    "photos:delete"
    "albums:delete"
    "users:manage"
  ]
  claims: {
    "access_level": "admin"
  }
}
```

**Role inheritance** uses `rdfs:subClassOf` in the RDF graph — the OWL inference engine (ADR-039) resolves the full permission set transitively. `admin` inherits `editor` which inherits `viewer` — token issuance reads the inferred permission closure, not just the declared permissions.

### Scope declaration

```bicep
scope "photos:read" = {
  product:     "photo-app"
  description: "Read access to photos and albums"
  claims: {
    "photos_access": "read"
  }
  permissions: ["photos:read", "albums:read"]
}

scope "photos:write" = {
  product:     "photo-app"
  description: "Write access to photos and albums"
  claims: {
    "photos_access": "write"
  }
  permissions: ["photos:read", "photos:write", "albums:manage"]
}
```

Scopes and roles both contribute claims to the token. When a scope and a role declare the same claim key, the role value wins — roles are more specific.

### Token flows

**Flow 1 — User authentication (standard OIDC)**

User authenticates with passkey → platform issues token scoped to the product:

```
User → OIDC authorization endpoint
     → passkey authentication
     → platform resolves user's roles in this product
     → platform resolves requested scopes
     → token issued with aud = product IRI
```

**Flow 2 — On-behalf-of (RFC 8693 token exchange)**

`photo-app` needs to call `user-service` on Alice's behalf. Alice has already authenticated against `photo-app`:

```
photo-app → POST /token
  grant_type: urn:ietf:params:oauth:grant-type:token-exchange
  subject_token: <alice's photo-app token>
  audience: https://picloud.local/products/user-service
  scope: users:read

Platform:
  1. Validates subject_token (aud = photo-app ✓)
  2. Checks photo-app has permission to act on behalf of users in user-service
  3. Resolves Alice's roles in user-service
  4. Issues new token:
     aud: user-service
     sub: alice
     act: { sub: photo-app }    ← actor claim — who is acting on Alice's behalf
     scope: users:read
```

The `act` claim preserves the full delegation chain — `user-service` knows both that Alice authorised the request and that `photo-app` is acting for her.

**Flow 3 — M2M client credentials**

A container in `photo-app` calls `user-service`'s SPARQL endpoint using its workload identity:

```
photo-app/api-server → POST /token
  grant_type: client_credentials
  client_id: photo-app
  client_secret: <app registration secret>
  scope: users:read
  audience: https://picloud.local/products/user-service

Platform:
  1. Validates client credentials (App Registration)
  2. Checks photo-app M2M permissions for user-service
  3. Issues token:
     aud: user-service
     sub: https://picloud.local/products/photo-app
     scope: users:read
```

M2M tokens have `sub` set to the product IRI, not a user IRI. `user-service` can distinguish M2M from delegated user access by checking `sub` type.

### M2M permission declaration

Products declare which other products they allow M2M access from:

```bicep
m2m-permission "allow-photo-app-read" = {
  product:      "user-service"
  client:       "photo-app"
  scopes:       ["users:read"]
  description:  "photo-app may read user profiles via M2M"
}
```

This resource must exist in `user-service`'s deployment before `photo-app` can request M2M tokens. This is consistent with ADR-022 (inter-product dependencies are declared resources) and ADR-028 (low coupling enforced structurally).

### Audience validation in the SDK

The SDK validates `aud` automatically on every incoming token:

```rust
// Rust SDK — token validation
let claims = picloud.iam().validate_token(token, expected_audience)?;
// Fails if aud != https://picloud.local/products/user-service
```

```typescript
// TypeScript SDK
const claims = await picloud.iam().validateToken(token, expectedAudience);
```

```csharp
// .NET SDK
var claims = await picloud.Iam().ValidateTokenAsync(token, expectedAudience);
```

### RDF representation

```turtle
<https://picloud.local/products/photo-app/roles/editor>
    a pc:Role ;
    pc:product    <https://picloud.local/products/photo-app> ;
    rdfs:subClassOf <https://picloud.local/products/photo-app/roles/viewer> ;
    pc:permission "photos:write" ;
    pc:permission "albums:manage" ;
    pc:claim [ pc:claimKey "access_level" ; pc:claimValue "read-write" ] .
```

Role inheritance is `rdfs:subClassOf` — the OWL inference engine materialises the full permission closure automatically. Token issuance queries the inferred graph, not the raw triples.

**Rationale:**
- Audience binding (`aud`) prevents token reuse across products — a fundamental JWT security property that is cheap to implement and expensive to lack
- Role inheritance via `rdfs:subClassOf` reuses the inference engine already in the platform — no custom inheritance logic
- On-behalf-of (RFC 8693) is the standard OAuth pattern for delegated access — no proprietary token exchange mechanism needed
- M2M client credentials are standard OAuth — workloads already have App Registration credentials (ADR-017)
- M2M permission declarations are resources in the target product — consistent with ADR-022, target product controls who can access it
- Static custom claims cover 90% of real use cases without the token issuance latency of dynamic SPARQL claims (dynamic claims are Phase 3)
- Custom scopes give API consumers a standard OAuth surface for requesting specific access

**Consequences:**
- `role`, `scope`, and `m2m-permission` are new product-scoped resource types
- Token issuance in `picloud-iam` must query the inferred RDF graph for the full permission closure
- `picloud-iam` must implement RFC 8693 token exchange endpoint
- The SDK `validateToken` method must check `aud` — this is the most critical SDK method from a security perspective
- Role inheritance creates a dependency ordering problem at deployment — if `editor` inherits `viewer`, `viewer` must exist before `editor` is created. The platform resolves this via the dependency graph at deploy time.
- M2M permission resources must exist in the target product before M2M tokens can be issued — cross-product declaration, target wins

**Test coverage:**

Scenario tests:
- `role_inheritance_claims.rs` — assign the `editor` role (which inherits `viewer`). Issue a token. Assert the token's `permissions` array contains both `editor`-level and `viewer`-level permissions (transitive inheritance via OWL inference).
- `audience_enforcement.rs` — issue a token for `photo-app`. Present the token to `user-service`'s SPARQL endpoint. Assert 403 (wrong audience).
- `token_exchange_on_behalf_of.rs` — execute RFC 8693 token exchange: `photo-app` acts on behalf of Alice against `user-service`. Assert the new token has `aud: user-service`, `sub: alice`, and an `act` claim containing `photo-app`.
- `m2m_permission_required.rs` — attempt M2M client credentials from `photo-app` to `user-service` without an `m2m-permission` resource in `user-service`. Assert 403 and a clear error.

Protocol probes:
- JWT claims: assert `iss`, `aud`, `sub`, `exp`, `iat`, `scope`, `roles`, `permissions` all present and correctly typed in issued tokens.
- RFC 8693 token exchange: assert `act` claim present, `aud` updated to target product.

Exit criteria:
- Transitive role permissions: inferred correctly via OWL in 100% of test runs.
- Audience mismatch: rejected with 403 in 100% of attempts.
- Token exchange (RFC 8693): `act` claim present and correct in 100% of exchange responses.

---

## ADR-052: Integrated DNS Server — Authoritative for Tenant Domain

**Status:** Accepted

**Context:** Every workload, ingress hostname, node, and product in PiCloud has a canonical IRI and a known network address — all of it already projected into the Oxigraph RDF graph. Clients on the local network need to resolve these hostnames without manual DNS record management. The platform is the authoritative source of truth for its own domain — the DNS server is just a query interface over data that already exists.

**Decision:** Every `picloud-server` node runs an authoritative DNS server on port 53 (UDP and TCP). It is authoritative for the cluster's tenant domain only (e.g. `picloud.local` or a custom domain configured at `cluster init`). It answers queries from the RDF graph. It does not recurse, forward, or resolve external names. External DNS resolution is delegated to the operator's existing infrastructure (Pi-hole + Unbound in the reference setup). Clients are configured to forward the tenant domain to any PiCloud node — one conditional forwarding rule in Pi-hole.

### Integration with existing DNS infrastructure

```
Client device
  → Pi-hole + Unbound (handles all external resolution)
    → *.picloud.local → forwarded to PiCloud DNS (192.168.x.x:53)
    → everything else → resolved normally via Unbound

# Pi-hole conditional forwarding — one rule:
picloud.local → 192.168.1.101  # any cluster node
```

PiCloud DNS only ever answers for its own domain. Pi-hole never needs to know about PiCloud internals.

### Records served

**A records** — IPv4 address for a hostname:

| Query | Answer | Source in graph |
|---|---|---|
| `picloud.local` | Cluster leader ingress IP | `pc:isLeader true` node |
| `pi-node-01.picloud.local` | Node IP | `pc:nodeAddress` on `pc:Node` |
| `photo-app.picloud.local` | Product ingress IP | `pc:ingressAddress` on `pc:Product` |
| `photos.picloud.local` | Ingress target IP | `pc:hostname` on `pc:Ingress` |
| `staging.photo-api.picloud.local` | Staging ingress IP | Ephemeral ingress resource |

**SRV records** — service discovery by type:

| Query | Answer |
|---|---|
| `_sparql._tcp.photo-app.picloud.local` | SPARQL endpoint port and host |
| `_events._tcp.photo-app.picloud.local` | Event stream SSE endpoint |
| `_https._tcp.picloud.local` | Cluster HTTPS ingress |

**TXT records** — semantic metadata for a service:

| Query | Answer |
|---|---|
| `photo-app.picloud.local` | `"ontology=https://picloud.local/products/photo-app/ontology version=1.0.0"` |
| `picloud.local` | `"cluster-id={uuid} platform-version=0.1.0"` |

**PTR records** — reverse DNS (IP → hostname):
Registered for node addresses and ingress IPs so tools like `nmap` and `traceroute` show meaningful names.

**NXDOMAIN** — for any hostname not found in the graph. No fallthrough, no recursion.

### Query model

Every DNS query resolves in two steps:

1. **Cache lookup** — in-memory cache keyed by `(qtype, qname)`. If present and not expired, return immediately.
2. **Graph query** — if cache miss, query Oxigraph with a SPARQL SELECT. Cache the result with TTL = 30 seconds.

```sparql
# A record lookup for an ingress hostname
SELECT ?address WHERE {
  {
    # Ingress hostname match
    ?ingress a pc:Ingress ;
             pc:hostname "{qname}" ;
             pc:targetAddress ?address .
  } UNION {
    # Node hostname match
    ?node a pc:Node ;
          pc:hostname "{qname}" ;
          pc:nodeAddress ?address .
  } UNION {
    # Product hostname match
    ?product a pc:Product ;
             pc:hostname "{qname}" ;
             pc:ingressAddress ?address .
  }
}
LIMIT 1
```

### TTL and cache invalidation

**TTL: 30 seconds** — short enough that clients re-query frequently, long enough to avoid hammering Oxigraph on every request.

**Event-driven cache invalidation** — the DNS server subscribes to platform events and invalidates affected cache entries immediately, without waiting for TTL expiry:

| Event | Cache entries invalidated |
|---|---|
| `WorkloadRescheduled` | All A records for that workload's hostname |
| `IngressCreated` | New entry added immediately |
| `IngressUpdated` | A, SRV, TXT records for that ingress hostname |
| `IngressDeleted` | Entry removed, subsequent queries return NXDOMAIN |
| `NodeJoined` | New PTR and A record for node hostname |
| `NodeLeft` | A and PTR records for that node removed |
| `ProductDeployed` | TXT record updated with new version |
| `StagingDeploymentReady` | Ephemeral staging A record added |
| `StagingTeardownCompleted` | Ephemeral staging A record removed |

This means workload reschedules are visible to DNS clients within seconds — the TTL is a fallback, not the primary invalidation mechanism.

### Multi-node consistency

Every node runs its own DNS server with its own in-memory cache. Caches are not synchronised across nodes — each node independently queries Oxigraph, which is consistent across the cluster via Raft. Since all nodes read from the same RDF graph, responses are consistent. Cache entries expire and refresh independently on each node within the 30-second TTL window.

Clients can point at any node's IP as their DNS server. If a node goes down, Pi-hole's conditional forwarding retries against another node (standard DNS retry behaviour).

### Implementation in picloud-network

The DNS server lives in `picloud-network` — the crate already responsible for mDNS, TLS, and certificate management.

```
picloud-network/src/
├── dns/
│   ├── server.rs      — UDP/TCP listener on port 53, query dispatch
│   ├── resolver.rs    — cache lookup → SPARQL query → response assembly
│   ├── cache.rs       — in-memory TTL cache with event-driven invalidation
│   ├── records.rs     — A, SRV, TXT, PTR record construction from RDF data
│   └── events.rs      — platform event subscription, cache invalidation
```

**DNS library:** `hickory-dns` (formerly trust-dns) — pure Rust, actively maintained, supports authoritative server mode, compiles to ARM64.

### Pi-hole configuration

One conditional forwarding rule points the tenant domain at any cluster node:

```
# Pi-hole Admin → Settings → DNS → Conditional Forwarding
Domain: picloud.local
DNS Server: 192.168.1.101  # any node IP — others used as fallback
```

For clusters with a custom domain at init time:
```
Domain: acme.local
DNS Server: 192.168.1.101
```

No other Pi-hole configuration needed. Pi-hole continues to handle all external resolution, ad blocking, and DHCP as before.

### Rationale
- The RDF graph already contains every hostname and address — DNS is a read-only projection of existing data, not a new data store
- Authoritative-only design keeps the implementation minimal — no recursive resolver, no upstream forwarder, no root hint management
- Delegating external resolution to Pi-hole + Unbound respects the operator's existing investment and keeps concerns separated
- Event-driven cache invalidation means workload reschedules are visible to clients within seconds without requiring zero-TTL records
- `hickory-dns` is the only pure Rust DNS library with authoritative server support — consistent with ADR-001
- Every node runs DNS independently — no single point of failure, no leader election needed for DNS

**Consequences:**
- `picloud-network` gains a `dns/` module
- `hickory-dns` is added as a workspace dependency
- Port 53 must be open on all cluster nodes (added to `deploy/setup-node.sh`)
- The platform ontology gains `pc:hostname` as a property on `pc:Ingress`, `pc:Node`, and `pc:Product`
- `picloud cluster init` output should include the conditional forwarding rule to paste into Pi-hole
- DNS queries are logged as OTel spans — slow Oxigraph queries surface in telemetry

**Test coverage:**

Scenario tests:
- `dns_a_records.rs` — query A records for the cluster root (`picloud.local`), a node hostname, a product hostname, and an ingress hostname. Assert each returns the correct IPv4 address matching the RDF graph.
- `dns_srv_records.rs` — query `_sparql._tcp.photo-app.picloud.local`. Assert the SRV record returns the correct host and port for the product's SPARQL endpoint.
- `dns_txt_records.rs` — query `photo-app.picloud.local` TXT record. Assert it contains the ontology IRI and product version.
- `dns_cache_invalidation.rs` — reschedule a container workload to a different node. Assert the DNS A record for the workload's ingress hostname updates to the new node's IP within 30 seconds (well before TTL expiry).
- `dns_nxdomain.rs` — query a hostname that does not exist in the cluster. Assert NXDOMAIN response with no fallthrough.

Protocol probes:
- RFC 1034/1035 DNS protocol compliance: assert response format is valid DNS wire format. Assert NXDOMAIN for unknown names (no recursion, no fallthrough).
- Assert response TTL = 30 seconds for all positive answers.

Exit criteria:
- DNS query response: < 5 ms on cache hit, < 50 ms on cache miss (SPARQL lookup).
- Cache invalidation after `WorkloadRescheduled`: < 30 seconds.
- NXDOMAIN for unknown names: 100% of attempts, zero recursion.

---

## ADR-053: Node Certificate Issuance and Enrollment

**Status:** Accepted

**Context:** Every node in a PiCloud cluster communicates over mTLS (ADR-027). A new node needs a certificate signed by the cluster CA to participate. The CA private key lives in the cluster — only the cluster can issue node certificates. This creates a bootstrap problem: a node needs a cert to join, but needs to join to get a cert.

Two operational contexts have different security requirements:

- **Home lab / trusted network** — the network is the trust boundary. Auto-enrolling any node that appears on the network is acceptable and eliminates operational friction entirely.
- **Secured environment** — network presence alone is not sufficient. A token must be pre-provisioned to authorise each new node.

**Decision:** PiCloud supports two enrollment modes configured at `cluster init`. Both use the same two-phase join and the same CA infrastructure. The mode is a cluster-wide setting — it applies to all nodes.

### The two-phase join (both modes)

**Phase 1 — Pre-auth enrollment (plain TLS, no client cert required)**

The cluster leader exposes a dedicated enrollment endpoint at `https://picloud.local/enroll`. This endpoint accepts plain TLS (server cert only — clients present no client cert). It does exactly one thing: issue node certificates.

```
New node (no cert yet)
  → discovers cluster via mDNS
  → generates ephemeral keypair locally
  → POST https://picloud.local/enroll
      { csr: <DER-encoded CSR>, token: <enrollment_token | null> }
  → cluster validates (mode-dependent — see below)
  → cluster CA signs CSR with node identity
  → returns signed certificate + cluster CA certificate
  → node stores cert and CA cert on disk
  → enrollment token invalidated (token mode only)
```

**Phase 2 — Full join (cert in hand)**

```
New node (cert issued)
  → connects to leader via mTLS ✓
  → presents cluster CA cert for server verification ✓
  → Raft join proceeds ✓
  → NodeJoined event emitted ✓
```

### Mode A — Auto-enroll (default)

Any node that discovers the cluster via mDNS and presents a valid CSR receives a certificate. No token required. Network presence is the authorisation.

```bash
picloud cluster init --domain picloud.local
# Auto-enroll is the default — no additional flags needed
```

**Security model:** The local network is the trust boundary. Any device on the network can join the cluster. Suitable when the network is controlled (home lab, dedicated VLAN, isolated switch).

**Safeguard:** Even in auto-enroll mode, the cluster ID and CA fingerprint are checked on every subsequent connection. A rogue node that somehow gets a cert can only participate if it also passes Raft membership — which requires the existing cluster to accept it. The cluster can revoke a node certificate at any time via `picloud node remove`.

### Mode B — Token enrollment

A node must present a valid enrollment token to receive a certificate. Tokens are single-use, time-limited, and issued by an existing cluster admin.

```bash
picloud cluster init --domain acme.local --enrollment-mode token
```

**Generating an enrollment token:**
```bash
picloud node enrollment-token --ttl 2h
→ Token: picloud-enroll-a3f9b2c1d4e5f6...
→ Expires: 2025-07-01T14:00:00Z
→ Single use — invalidated after first use
```

**Distributing the token to a new node:**
The token is placed in the node's config before boot. Two delivery mechanisms:

```bash
# Option 1 — environment variable in systemd service override
sudo systemctl edit picloud
# Add:
[Service]
Environment=PICLOUD_ENROLLMENT_TOKEN=picloud-enroll-a3f9b2c1...

# Option 2 — config file
echo "enrollment_token = picloud-enroll-a3f9b2c1..." \
  > /home/ubuntu/picloud/config.toml
```

On startup, `picloud-server` reads the token, uses it once to enroll, then removes it from config. The token is never stored after use.

### CA architecture

**The CA lives in the cluster, replicated via Raft.**

The CA private key is generated at `cluster init`, encrypted at rest with the cluster's master key, and stored in Raft state. Every node has a copy of the encrypted CA key — if the leader fails, the new leader has the key and can continue issuing certificates immediately.

The CA certificate is embedded in the cluster identity (ADR-042) alongside the cluster ID. Every node knows the CA certificate at join time — it is returned in the enrollment response.

**Certificate lifetime:**
- Node certificates: 90 days, auto-renewed 7 days before expiry
- Workload certificates: 24 hours, auto-renewed 1 hour before expiry
- Ingress/TLS certificates: 90 days, auto-renewed 14 days before expiry

**Auto-renewal:** The platform tracks certificate expiry in the RDF graph. An inference rule (ADR-038) fires an `AlertFired` event when any certificate is within its renewal window. The certificate management component in `picloud-network` subscribes to this event and initiates renewal automatically.

### Certificate revocation

When a node is removed from the cluster (`picloud node remove`):
1. `NodeRemoved` event emitted
2. Node's certificate added to an in-memory CRL (Certificate Revocation List) stored in Raft state
3. All other nodes refresh their CRL from Raft state
4. The removed node's mTLS connections are rejected within one Raft heartbeat cycle

### Enrollment endpoint security

The `/enroll` endpoint is the most sensitive surface in the platform:

- Served over TLS with the cluster's CA certificate — clients can verify they are talking to the legitimate cluster
- Rate limited — maximum 5 enrollment attempts per minute per IP
- In auto-enroll mode: logs every enrollment as a `NodeEnrolled` event with the node's address
- In token mode: token is single-use and time-limited
- CSR validation: the CSR must contain only the node's hostname in the Subject — no wildcard SANs, no IP SANs other than the node's own address
- Enrollment is always logged as a platform event — `NodeEnrolled` or `NodeEnrollmentRejected`

### Node identity in certificates

Every node certificate carries:
```
Subject: CN=pi-node-01.picloud.local
SAN: DNS:pi-node-01.picloud.local, IP:192.168.1.101
Issuer: CN=PiCloud CA, O=picloud.local, cluster-id={uuid}
```

The cluster ID is embedded in the Issuer — a certificate issued by a different cluster (different cluster ID) is rejected even if it chains to the same CA.

### Implementation in picloud-network

```
picloud-network/src/
├── ca/
│   ├── mod.rs         — CA module root
│   ├── authority.rs   — CA key management, certificate signing
│   ├── enrollment.rs  — enrollment endpoint handler, CSR validation
│   ├── renewal.rs     — certificate expiry tracking, auto-renewal
│   └── revocation.rs  — CRL management, Raft-replicated
└── dns/               — (existing)
```

**Crates used:**
- `rcgen` — pure Rust certificate generation and CSR handling (already in workspace)
- `x509-parser` — certificate parsing and validation (already in workspace)
- `rustls` — TLS configuration (already in workspace)

No new dependencies required.

### CLI commands

```bash
# Generate enrollment token (token mode only)
picloud node enrollment-token --ttl 2h

# List active enrollment tokens
picloud node enrollment-tokens

# Revoke an enrollment token
picloud node revoke-token <token-id>

# Remove a node and revoke its certificate
picloud node remove pi-node-05

# List all node certificates and their expiry
picloud node certs

# Manually trigger certificate renewal for a node
picloud node renew-cert pi-node-01
```

### Configuration at cluster init

```bash
# Auto-enroll (default — for trusted networks)
picloud cluster init --domain picloud.local

# Token enrollment (for secured environments)
picloud cluster init --domain acme.local --enrollment-mode token

# Token enrollment with BYO CA
picloud cluster init \
  --domain acme.local \
  --enrollment-mode token \
  --ca-cert ./ca.pem \
  --ca-key  ./ca-key.pem
```

**Rationale:**
- Two modes with a clear default eliminates friction for the primary use case (home lab) while making the secure path available without custom implementation
- Same two-phase join in both modes means one code path, one security model — only the authorisation check differs
- CA in Raft state means no single point of failure for certificate issuance — any node that becomes leader can immediately issue certs
- All enrollment events in the platform log — `NodeEnrolled`, `NodeEnrollmentRejected` — mean the cluster always knows who joined and when
- `rcgen` and `x509-parser` are already in the workspace — zero new dependencies
- Auto-renewal via inference rules and event subscriptions means certificate expiry is handled the same way as any other platform alert — consistently and observably

**Consequences:**
- `picloud-network` gains a `ca/` module
- The enrollment endpoint must be started before Raft join — it is the first HTTP endpoint brought up at node startup
- In auto-enroll mode, the cluster should log a prominent warning at init time so operators know the security model
- Certificate expiry tracking adds `pc:certExpiresAt` and `pc:certFingerprint` to the node's RDF triples
- The master key used to encrypt the CA private key at rest must be derived from the cluster ID — losing the cluster ID means losing access to the CA

**Test coverage:**

Scenario tests:
- `auto_enroll_mode.rs` — configure a cluster in auto-enroll mode. Power on a new node. Assert `NodeEnrolled` event within 30 seconds of mDNS discovery, correct node certificate issued, node participates in Raft.
- `token_enroll_single_use.rs` — generate an enrollment token. Use it once to enroll a node. Assert `NodeEnrolled` event. Attempt to reuse the token on a second node. Assert `NodeEnrollmentRejected` event.
- `token_enroll_expiry.rs` — generate a token with a 30-second TTL. Wait 45 seconds. Attempt enrollment. Assert `NodeEnrollmentRejected` with an expiry reason.
- `cert_revocation.rs` — remove a node via `picloud node remove`. Assert a `NodeRemoved` event, the CRL updated in Raft, and the removed node's subsequent mTLS connections rejected within 5 seconds (one heartbeat cycle).
- `csr_wildcard_rejection.rs` — submit a CSR with a wildcard SAN (`*.picloud.local`). Assert the enrollment endpoint returns 400 and no certificate is issued.

Protocol probes:
- X.509 CSR validation: assert CSR Subject contains only the node hostname. Assert no wildcard SANs accepted. Assert IP SAN matches only the enrolling node's address.

Exit criteria:
- Auto-enroll: node joined and participating in Raft within 30 seconds of power-on.
- Token single-use: second use rejected 100% of the time.
- Revocation: rejected mTLS connections within 5 seconds of `NodeRemoved` event.
- Wildcard CSR: rejected 100% of the time.

---

## ADR-054: Test-Augmented ADR Template

**Status:** Accepted

**Context:** PiCloud is a distributed platform with no external dependencies and a strong engineering culture of measuring and validating everything on real hardware. As the system grows, architectural decisions made early become invisible assumptions. Without explicit testability defined at decision time, test coverage is added retroactively (or not at all), and the tests that are added tend to test the implementation rather than the decision.

The three test suite designs established alongside this document — Scenario Harness, Chaos + Invariants, and Protocol Compliance — provide a structured vocabulary for expressing what "working correctly" means for any decision. This vocabulary must be applied at the point of making a decision, not after the fact.

**Decision:** Every ADR must include a `Test coverage` section. This section is mandatory for all new ADRs and must be present in all existing ADRs before the feature they govern enters implementation.

**Template — the `Test coverage` section must contain:**

- **Scenario tests** — one or more named scenarios from the Scenario Harness that validate the happy path and key edge cases for this decision. Each entry names the scenario file and states what it asserts.
- **Invariants** — properties that must hold continuously, including during and after faults. Each invariant is a falsifiable statement with a defined check method (SPARQL query, DNS probe, or metric threshold).
- **Protocol probes** *(only for decisions that introduce a protocol boundary)* — which RFC or specification the probe validates, and the specific assertions made.
- **Exit criteria** — measurable, pass/fail thresholds. These are the criteria that must be green before the feature is considered complete. Vague criteria such as "DNS works" are not acceptable. Every criterion must end with a number or a percentage.

**Consequences:**
- New ADRs cannot be merged without a `Test coverage` section.
- The test coverage section is the primary input for the `picloud-test` scenario catalogue. Tests are not invented separately — they are derived from ADRs.
- If a decision is difficult to test, that is a signal the decision is underspecified. Rewrite the decision, not the tests.
- All existing ADRs have been retrofitted with test coverage sections as part of this ADR's introduction.

**Rejected alternatives:**
- **Test coverage in the PRD** — PRD sections are feature-level. Test logic for a specific technical decision is too specific to live at that level and would be orphaned from the rationale it validates.
- **Separate test specification document** — a separate doc drifts from the decisions it covers. Co-location ensures the tests are updated when the decision is revised.
- **Tests only in code** — code-level tests are correct for unit and integration coverage but lack the narrative context of why a test exists. The ADR section is the human-readable contract; the code is the enforcement of it.

**Test coverage:**

Scenario tests:
- `adr_test_coverage_completeness.rs` — parse all ADRs in the repository. Assert every ADR that has a status of `Accepted` contains a `Test coverage` section with at least one scenario test and at least one exit criterion.
- `scenario_catalogue_sync.rs` — parse the scenario catalogue in `picloud-test/scenarios/`. Assert every scenario named in an ADR `Test coverage` section has a corresponding `.rs` file in the catalogue.

Exit criteria:
- 100% of Accepted ADRs contain a `Test coverage` section.
- 100% of ADR-named scenarios have a corresponding file in the `picloud-test` catalogue.

---

## ADR-055: Capability as a First-Class Interface Contract

**Status:** Accepted

**Context:** As the number of Products in a PiCloud cluster grows, shared functionality emerges naturally. The naive solution — letting one Product depend directly on another that happens to implement the functionality — violates the stable dependency principle. A dependency on `photo-app` for GPS resolution means consumers are coupled to `photo-app`'s deployment lifecycle, versioning, and ownership. If `photo-app` is deprecated, every consumer breaks. If the GPS logic needs to evolve, the photo-app team must accommodate all consumers' timelines.

The root problem is that PiCloud lacked a way to express a named, versioned *interface contract* independently of any *implementation*. Products are hermetically sealed deployment units — they are the wrong primitive for expressing a shared capability that multiple Products may implement over time.

**Decision:** Introduce `capability` as a first-class, cluster-scoped resource type. A capability is a pure interface contract: a named, versioned declaration of an event schema and SHACL shapes that describes a service interaction. A capability has no workload, no container, no code. It declares only the contract.

Products separately declare `implements` (they fulfil a capability's contract) and `capabilities` (they depend on a capability being available in the cluster). Consumers bind to the capability, not to any specific Product.

**Resource definition:**

```bicep
// Cluster-scoped — no product: field
capability 'gps-to-place' = {
  version: '1.0.0'
  description: 'Translates GPS coordinates to a named place with confidence score'
  ontology: './capabilities/gps-to-place.ttl'
  shapes:   './capabilities/gps-to-place.shacl'
  events: {
    input:  'CoordinatesReceived'
    output: 'PlaceResolved'
  }
}

// Photo-app is the first implementor — it does not own the capability
product 'photo-app' = {
  version: '2.0.0'
  implements: ['gps-to-place@1.0.0']
}

// Maps-app depends on the capability, not on photo-app
product 'maps-app' = {
  version: '1.0.0'
  capabilities: [
    { capability: 'gps-to-place', minVersion: '1.0.0' }
  ]
}
```

**Enforcement rules (applied at `resource apply` time):**

1. A `capability` must declare at least one `input` event and one `output` event — capabilities with no declared interface are rejected.
2. A `capability` must declare either `ontology` or `shapes` (or both) — the contract must be expressed in the type system, not just in prose.
3. A Product declaring `implements` must have an `event-subscription` to the capability's `input` event type and must emit the capability's `output` event type — the platform validates structural conformance against the declared SHACL shapes at deploy time.
4. A Product declaring a `capabilities` dependency will fail `resource apply` if no Product in the cluster currently declares `implements` for that capability at the required `minVersion`.
5. A `capability` cannot be deleted while any Product declares a `capabilities` dependency on it — cascading deletion is blocked.
6. Dependency direction is enforced: a Product that `implements` a capability cannot also declare a `capabilities` dependency on a capability it does not itself implement.

**Capability lifecycle events:**

- `CapabilityDeclared` — capability resource received and validated
- `CapabilityReady` — at least one implementing Product is deployed and structurally conformant
- `CapabilityImplementorAdded` — a new Product declares `implements` for an existing capability
- `CapabilityImplementorRemoved` — an implementing Product is removed
- `CapabilityUnfulfilled` — no implementing Product exists; all consumers receive this event
- `CapabilityDeleted` — capability removed (only when no consumers exist)

**Routing:**

The platform resolves which Product currently implements the required capability and routes the `input` event to that Product's event bus. If multiple Products implement the same capability, the platform selects the implementor with the highest version that satisfies the consumer's `minVersion` constraint. One active implementor per capability in Phase 1.

**Rationale:**
- Separating interface (capability) from implementation (Product) is the structural enforcement of the stable dependency principle — consumers never take a direct dependency on a mutable deployment unit
- Capabilities are expressed in the platform's native type system (RDF ontology + SHACL) — no separate contract registry is needed
- The `CapabilityUnfulfilled` event gives consumers observable signal when their dependency is broken
- Requiring at least one implementor before a consumer can deploy prevents consumers from deploying against phantom contracts

**Rejected alternatives:**
- **Direct product-to-product dependencies** — creates tight coupling between deployment lifecycles.
- **Capability as a sub-resource of the implementing Product** — embeds the same ownership problem one level down.
- **Shared library / SDK** — moves the contract into code, loses platform-level validation and discoverability.
- **Service mesh with named service contracts** — adds sidecar network layer, violates single-binary goal.

**Consequences:**
- `capability` is a new cluster-scoped resource type
- The `product` resource gains `implements` and `capabilities` fields
- `resource apply` gains a capability resolution validation step
- `picloud capability list` surfaces all capabilities, implementors, consumers, and fulfilment status

**Test coverage:**

Scenario tests:
- `capability_declaration.rs` — declare a `capability` resource via `picloud resource apply`. Assert a `CapabilityDeclared` event is emitted. Assert the capability appears in the cluster RDF graph as a `pc:Capability` node with correct `pc:version`, `pc:inputEvent`, and `pc:outputEvent` triples.
- `capability_implements_shacl_validation.rs` — deploy a Product that declares `implements: ['gps-to-place@1.0.0']` but whose workload does not subscribe to `CoordinatesReceived`. Assert `resource apply` fails with a SHACL conformance error. Assert no `CapabilityImplementorAdded` event is emitted.
- `capability_consumer_blocked_without_implementor.rs` — attempt to deploy `maps-app` with a `capabilities` dependency on `gps-to-place` when no Product implements it. Assert `resource apply` fails with a `CapabilityUnfulfilled` error. Assert `maps-app` is not deployed.
- `capability_routing.rs` — deploy `photo-app` implementing `gps-to-place`. Deploy `maps-app` consuming it. Emit a `CoordinatesReceived` event from `maps-app`. Assert a `PlaceResolved` event is routed back through `photo-app` and arrives on `maps-app`'s event bus within 2 seconds.
- `capability_version_selection.rs` — deploy two Products implementing `gps-to-place` at `v1.0.0` and `v1.1.0`. Deploy a consumer requiring `minVersion: '1.0.0'`. Emit `CoordinatesReceived`. Assert the `v1.1.0` implementor handles the event (highest satisfying version wins).
- `capability_implementor_removed_unfulfilled.rs` — remove the only implementing Product. Assert `CapabilityUnfulfilled` event is emitted within 10 seconds. Assert the event is delivered to all consumer Products' event buses.
- `capability_deletion_guard.rs` — attempt `picloud resource delete capability/gps-to-place` while `maps-app` declares a dependency on it. Assert the delete is rejected with a dependency error. Assert the capability remains in the cluster graph.

Invariants:
- The cluster graph contains exactly one active implementor per capability version at any time. Verified by SPARQL every 5 seconds during Chaos runs: `SELECT (COUNT(?impl) AS ?n) WHERE { ?impl pc:implements ?cap ; pc:status pc:Active }` — a count of 0 with active consumers is a failure; a count > 1 for the same version is a failure.
- Any Product with a declared `capabilities` dependency has a resolvable implementor at the required `minVersion`. Verified by SPARQL every 60 seconds: `SELECT ?consumer ?cap WHERE { ?consumer pc:requiresCapability ?cap . FILTER NOT EXISTS { ?impl pc:implements ?cap ; pc:status pc:Active } }` — any result row is a failure.
- Dependency direction: no `implements` Product has a `capabilities` dependency on a Product it does not implement. Verified at every `resource apply` and as a nightly SPARQL invariant check.

Chaos scenarios:
- Kill the implementing Product's workload mid-routing. Assert `CapabilityUnfulfilled` event emitted. Assert in-flight `CoordinatesReceived` events are not silently dropped — they either complete or emit a `CapabilityRoutingFailed` event.
- Deploy a second implementor at a higher version mid-flight. Assert routing switches to the new implementor without dropping in-flight events.

Exit criteria:
- `resource apply` validation (capability enforcement): < 500 ms.
- Capability event routing overhead vs direct product-to-product event routing: < 5 ms added latency at p99, verified across 10,000 routed events.
- `CapabilityUnfulfilled` emitted within 10 seconds of implementing Product removal: 100% of test runs.
- Consumer `resource apply` blocked when capability is unfulfilled: 100% of attempts across 50 test runs.
- SHACL conformance check catches incorrect `implements` declaration: 100% of malformed deployments rejected.

---

## ADR-056: Data Products and Data Domains as First-Class Analytical Sharing Primitives

**Status:** Accepted

**Context:** PiCloud Products maintain internal RDF state in per-product named graphs, queryable via IAM-gated SPARQL endpoints. The original design permitted cross-product SPARQL access as the mechanism for sharing data between Products. This decision was informed by Data Mesh thinking but did not complete the model.

The problem: cross-product access to a Product's internal graph exposes operational state — the live, mutable projection of the event log. Consumers take an implicit dependency on the producer's internal schema. When the producer refactors its graph for operational reasons, consumers break silently. There is no publication decision, no contract, no SLO, and no way to discover what data is available across the cluster.

The root issue is a missing distinction between two planes:

- **Operational graph** — a Product's internal RDF state. Private. Reflects live operational data. Schema evolves freely as the domain evolves.
- **Analytical graph** — a curated, versioned, published projection of selected domain data. Stable contract. Declared freshness SLO. Explicitly shared.

This ADR also introduces **data domains** as a governance grouping that spans multiple Products. A data domain is not a deployment unit — it is an organisational and discoverability boundary that groups related data products across the cluster.

**Decision:** Introduce two new resource types:

1. `data-domain` — a cluster-scoped governance namespace that groups data products. Has a declared steward identity, sensitivity classification, and domain-level SHACL constraints applied to all member data products at `resource apply` time.
2. `data-product` — a product-scoped resource that publishes a curated, versioned analytical projection of a subset of the Product's internal graph into a separate named graph, belonging to exactly one `data-domain`.

Cross-product SPARQL access to internal Product graphs is removed. All cross-product data sharing must go through explicitly published `data-product` resources.

**Resource definitions:**

```bicep
// Cluster-scoped
data-domain 'geospatial' = {
  description: 'All location and mapping data products across the cluster'
  steward:     'identity/alice'
  sensitivity: 'internal'
}

// Product-scoped — published by photo-app, belongs to the geospatial domain
data-product 'photo-locations' = {
  product:     'photo-app'
  domain:      'geospatial'
  version:     '1.0.0'
  description: 'Geo-tagged photo locations aggregated by resolved place'
  ontology:    './data-products/photo-locations.ttl'
  shapes:      './data-products/photo-locations.shacl'
  projection:  './data-products/photo-locations.rq'
  freshness: {
    maxAge:   '15m'
    triggers: ['PlaceResolved', 'PhotoDeleted']
  }
  access: {
    visibility: 'cluster'
    roles:      ['data-consumer']
  }
}

// Consumer depends on the data product contract, not on photo-app
product 'maps-app' = {
  version:      '1.0.0'
  dataProducts: [
    { source: 'photo-app/photo-locations', minVersion: '1.0.0' }
  ]
}
```

**Architecture: named graph separation**

```
Internal operational graph:   https://picloud.local/products/photo-app/graph
Published data product graph: https://picloud.local/products/photo-app/data-products/photo-locations/graph
```

When a trigger event arrives, the platform runs the `projection` SPARQL CONSTRUCT against the internal graph and atomically replaces the data product named graph with the result. Consumers query the published graph only.

**Freshness model — push only**

Projections rebuild exclusively on declared trigger events. No polling, no scheduled refresh, no on-query materialisation. Requiring explicit triggers forces producers to reason about which state changes make the analytical output stale — this gap surfaces at design time rather than being discovered by confused consumers in production. `freshness.maxAge` is an SLO declaration, not a scheduling mechanism. The platform monitors actual staleness and emits `DataProductSLOBreached` when exceeded.

**Enforcement rules (applied at `resource apply` time):**

1. A `data-product` must declare at least one `triggers` event.
2. A `data-product` must declare `freshness.maxAge`.
3. A `data-product` must belong to exactly one `data-domain`.
4. A `data-product` must declare `ontology` or `shapes` (or both).
5. A `data-product` with `visibility: cluster` requires the declaring Product to have at least one `data-consumer` role defined in its IAM scope.
6. A consumer Product declaring `dataProducts` dependencies fails `resource apply` if the referenced data product does not exist at the required `minVersion`.
7. A `data-domain` cannot be deleted while any `data-product` is assigned to it.
8. A `data-product` cannot be deleted while any Product declares a `dataProducts` dependency on it.
9. Cross-product SPARQL queries targeting another Product's internal named graph are rejected at the HTTP layer with `403 Forbidden`.

**Composition with capabilities (ADR-055):**

A capability's output event is a first-class trigger for a data product projection rebuild. The capability is the operational act; the data product is the analytical record of accumulated results.

```bicep
data-product 'photo-locations' = {
  freshness: {
    triggers: ['PlaceResolved']   // ADR-055 capability output drives analytical refresh
  }
}
```

**Data product lifecycle events:**

- `DataDomainDeclared`, `DataDomainDeleted`
- `DataProductDeclared`, `DataProductReady`, `DataProductRefreshed`
- `DataProductSLOBreached`, `DataProductSLORestored`
- `DataProductDeleted`

**Breaking change:** Prior cross-product SPARQL access to internal graphs is removed. Existing consumers must migrate to declared `data-product` resources.

**Rationale:**
- Hard named graph separation enforces the operational/analytical boundary in the storage layer — no convention to accidentally violate
- Push-only freshness forces producers to reason about their event model at design time
- Removing direct cross-product SPARQL access eliminates the escape hatch that would make data products optional in practice
- `data-domain` provides the discoverability surface Data Mesh's self-serve principle requires

**Rejected alternatives:**
- **Direct cross-product SPARQL access with conventions** — the status quo. Conventions are not enforced. Schema coupling grows silently.
- **Event-sourced data products (consumers replay events)** — correct for some use cases but requires every consumer to maintain their own projection infrastructure.
- **Separate analytical store (Parquet/DataFusion)** — conflicts with the RDF-native architecture. Parquet is the right primitive for telemetry (ADR-046), not domain knowledge.

**Consequences:**
- `DataProductProjector` runs SPARQL CONSTRUCT projections on trigger events and manages published named graphs
- `DataProductSLOMonitor` tracks staleness against declared `maxAge` and emits breach/restore events
- The HTTP layer enforces the cross-product SPARQL access restriction
- `picloud data-product list` and `picloud data-domain list` CLI commands

**Test coverage:**

Scenario tests:
- `data_domain_declaration.rs` — declare a `data-domain` resource. Assert `DataDomainDeclared` event emitted. Assert the domain appears in the cluster graph with correct `pc:steward`, `pc:sensitivity`, and `pc:description` triples.
- `data_product_field_validation.rs` — attempt to declare a `data-product` missing each mandatory field in turn (`triggers`, `maxAge`, `domain`, `shapes`/`ontology`). Assert each attempt is rejected at `resource apply` with a specific validation error. Assert no partial resource state is created in the cluster graph.
- `data_product_projection_on_trigger.rs` — deploy `photo-app` with a `data-product 'photo-locations'` declaring `triggers: ['PlaceResolved']`. Emit a `PlaceResolved` event. Assert the SPARQL CONSTRUCT projection runs. Assert the data product named graph (`…/data-products/photo-locations/graph`) is populated with triples. Assert a `DataProductRefreshed` event is emitted with non-zero triple count, duration, and timestamp within `freshness.maxAge`.
- `data_product_named_graph_separation.rs` — after a projection run, query both the internal operational graph (`…/products/photo-app/graph`) and the data product graph (`…/data-products/photo-locations/graph`). Assert they are distinct named graphs. Assert the data product graph contains only triples produced by the declared CONSTRUCT query — no triples from the internal graph appear unless the CONSTRUCT explicitly produces them.
- `data_product_atomic_swap.rs` — trigger a projection rebuild while `maps-app` is issuing SPARQL queries against the data product graph at 20 queries/second. Assert zero query errors during the swap. Assert no query returns a mix of triples from the old and new projection (partial state).
- `cross_product_internal_graph_blocked.rs` — authenticate as a `maps-app` workload identity. Attempt a SPARQL query directly against `https://picloud.local/products/photo-app/graph`. Assert `403 Forbidden`. Assert a `UnauthorisedGraphAccess` event is emitted in the platform log. Repeat with platform-admin identity — assert `200 OK` (admin exemption verified).
- `data_product_consumer_blocked_without_product.rs` — attempt to deploy `maps-app` with a `dataProducts` dependency on `photo-app/photo-locations` when that data product does not exist. Assert `resource apply` fails with a `DataProductNotFound` error. Assert `maps-app` is not deployed.
- `data_product_slo_breach_and_restore.rs` — deploy a data product with `maxAge: '2m'`. Stop emitting trigger events. Wait 2 minutes 30 seconds. Assert `DataProductSLOBreached` event emitted. Resume trigger events. Assert the next successful refresh emits `DataProductSLORestored`. Assert the SLO breach is visible in the cluster RDF graph between breach and restore events.
- `data_product_deletion_guard.rs` — attempt to delete `data-product 'photo-locations'` while `maps-app` declares a `dataProducts` dependency on it. Assert the delete is rejected. Assert the data product and its named graph remain intact.
- `data_domain_deletion_guard.rs` — attempt to delete `data-domain 'geospatial'` while `photo-app/photo-locations` is assigned to it. Assert the delete is rejected with a member count error.
- `capability_triggers_data_product.rs` — integration test combining ADR-055 and ADR-056. Deploy `gps-to-place` capability and `photo-locations` data product with `triggers: ['PlaceResolved']`. Emit `CoordinatesReceived` via `maps-app`. Assert the capability routes to `photo-app`, `PlaceResolved` is emitted, and the `photo-locations` data product projection is rebuilt — all within 30 seconds end-to-end.

Invariants:
- The data product named graph contains only triples produced by the current CONSTRUCT query. Verified on demand by re-running the CONSTRUCT externally and comparing the result set against the stored graph: any triple in the stored graph absent from the CONSTRUCT result is a failure.
- Cross-product access to internal named graphs returns `403` for all non-owner, non-admin identities. Verified by an HTTP probe against every product's internal graph IRI from every other product's workload identity on every CI run.
- Every data product with at least one trigger event received is refreshed within its declared `maxAge`. Verified by a SPARQL query against the cluster graph every 30 seconds during integration runs: `SELECT ?dp WHERE { ?dp a pc:DataProduct ; pc:lastRefreshed ?t ; pc:maxAge ?m . FILTER((NOW() - ?t) > ?m) }` — any result is a failure.
- The operational graph and data product graph remain distinct named graphs at all times. Verified by asserting the two IRI paths resolve to different `GRAPH` contexts in Oxigraph and never share a named graph identifier.

Chaos scenarios:
- Kill the producer workload mid-projection. Assert the data product graph retains its last valid state (the in-progress projection is discarded, not partially committed). Assert `DataProductRefreshed` is not emitted for the aborted run.
- Flood trigger events (1000 `PlaceResolved` events in 1 second). Assert the projection runner serialises correctly — only one projection run at a time, no interleaved writes to the data product graph. Assert final graph state reflects the last completed projection.

Exit criteria:
- Projection rebuild latency for a 1,000-triple CONSTRUCT result: < 5 seconds from trigger event receipt to `DataProductRefreshed` event, measured across 100 runs.
- Atomic swap: zero partial-state query results across 100 concurrent query-plus-swap test runs.
- Cross-product internal graph access blocked: 100% of unauthorised attempts return `403`, verified across 1,000 probe requests with varied workload identities.
- `DataProductSLOBreached` emitted within 60 seconds of `maxAge` expiry: 95% of test runs (5% tolerance for event bus scheduling jitter).
- Mandatory field validation: 100% of `data-product` declarations missing any required field rejected at `resource apply` before any cluster state is mutated.
- Data product named graph purity: zero extraneous triples present across 100 post-projection inspection runs.
- `data-domain` and `data-product` deletion guards: 100% of guarded deletes rejected when active dependents exist.
