# PiCloud Feature Checklist

> Auto-maintained by the `/picloud-implement` skill.
> Each feature maps to a PRD section and/or ADR.
> Status: [ ] not started, [~] partial/stub, [x] implemented, [T] tested, [V] verified on cluster
>
> Last updated: 2026-04-10

---

## Core Platform (ADR-001 — ADR-006)

- [V] **Rust single binary** (ADR-001) — one binary runs on every node
- [V] **Raft consensus** (ADR-002) — openraft with in-memory log store, leader election, append/vote/snapshot RPCs
- [V] **mDNS discovery** (ADR-003) — domain-scoped `_pc-{hash}._tcp.local.`, peer add/remove, self-filter
- [V] **Event sourcing** (ADR-004) — append-only event log, idempotent dedup, broadcast pub/sub
- [V] **RDF graph projection** (ADR-005) — Oxigraph projector handles 16+ event types, SPARQL query
- [V] **Oxigraph triplestore** (ADR-006) — in-memory + optional RocksDB, cursor-based replay

## Event & State Model (ADR-007, ADR-008, ADR-031, ADR-035)

- [V] **Declarative resource syntax** (ADR-007) — .picloud HCL-like parser
- [V] **Eventually consistent commands** (ADR-008) — POST /api/commands with SSE correlation
- [V] **Event schema versioning** (ADR-031) — schema IRIs in EventEnvelope
- [V] **Event replay** (ADR-035) — PersistentEventLog with JSON-lines file, replay on startup
- [T] **Log compaction** (ADR-035) — keeps recent 1000 of 10000 events

## IAM (ADR-009, ADR-017, ADR-025, ADR-026, ADR-027)

- [V] **Identity provider** (ADR-009) — HMAC-SHA256 tokens, claims with aud/scopes/permissions
- [V] **OIDC provider** (ADR-017) — .well-known/openid-configuration, JWKS, token endpoint
- [T] **Passkey/FIDO2 auth** (ADR-025) — challenge generation + response acceptance scaffolded
- [x] **Passkey verification** (ADR-025) — CBOR/COSE key decoding via ciborium, ES256 verification
- [T] **Bootstrap token exchange** (ADR-026) — three-tier recovery, enrollment tokens
- [T] **mTLS workload identity** (ADR-027) — cert signing via platform CA
- [T] **Secret store** (ADR-009) — AES-256-GCM encryption via ring, HKDF key derivation

## Product Model (ADR-016, ADR-018, ADR-019, ADR-021, ADR-023)

- [V] **Product as deployment unit** (ADR-016) — product IRI, hermetic isolation
- [V] **Product event bus** (ADR-018) — per-product event store
- [T] **Per-product SPARQL** (ADR-019) — query_product() with named graphs
- [T] **One active version** (ADR-021) — enforced in provisioner
- [T] **Ontology per product version** (ADR-023) — /products/{p}/ontology endpoint

## Storage (ADR-011, ADR-012, ADR-013, ADR-024)

- [V] **Block storage** (ADR-011) — LocalStorageBackend with directory-backed volumes
- [T] **Volume allocation** (ADR-012) — create/delete/capacity tracking
- [V] **Replication** (ADR-013) — StorageReplicator with SHA-256 manifest, HTTP file sync
- [T] **Storage intent model** (ADR-024) — FullReplication / Quorum / Local durability tiers

## Workloads (ADR-010)

- [T] **OCI containers** (ADR-010) — podman/docker detection, CLI-based scheduling
- [T] **Raw binaries** (ADR-010) — tokio::process spawn, restart policies
- [T] **Health/restart** (ADR-010) — Always/OnFailure/Never policies with background monitor

## Networking (ADR-014, ADR-020)

- [V] **HTTP server** (ADR-014) — axum with IRI-based routing on port 7443
- [V] **Internal DNS** (ADR-014) — mDNS-based service discovery
- [T] **Cluster graph as service registry** (ADR-020) — RDF triples for node/service discovery

## Ingress & Proxy (ADR-028, ADR-030)

- [T] **Ingress router** (ADR-028) — IngressRouter with longest-prefix-wins, external/internal tables
- [T] **Proxy forwarding** (ADR-028) — reqwest with connect/read timeouts, 502/503 handling
- [~] **Platform CA with BYO-CA** (ADR-030) — PlatformCa generates certs, BYO-CA not supported yet

## Observability (ADR-040, ADR-041, ADR-045, ADR-046)

- [T] **Hardware metrics agent** (ADR-040) — MetricRecorded events
- [T] **Alert rules** (ADR-041) — SPARQL CONSTRUCT with AlertFired events
- [T] **OpenTelemetry** (ADR-045) — OtelAggregator started in composition root
- [T] **Time-series storage** (ADR-046) — JsonlTelemetryStore

## Tagging & Groups (ADR-036, ADR-037, ADR-038, ADR-039)

- [T] **Universal tagging** (ADR-036) — TagAdded/TagRemoved events, CLI tag commands
- [T] **Groups as IAM resource** (ADR-037) — SPARQL CONSTRUCT membership rules
- [T] **Inference rules** (ADR-038) — InferenceEngine reconciliation loop
- [T] **RDFS/OWL inference** (ADR-039) — embedded via Oxigraph

## Configuration & Feature Flags (ADR-043, ADR-044)

- [T] **Product config store** (ADR-043) — ConfigChanged events, HTTP endpoints
- [T] **Feature flags** (ADR-044) — FeatureFlagChanged events, builder support

## Cluster Identity (ADR-042)

- [V] **Cluster ID + domain** (ADR-042) — ClusterIdentity with ca_fingerprint, enrollment_mode
- [V] **Domain-scoped mDNS** (ADR-042) — hash-based service type isolation
- [V] **Node join validation** (ADR-042) — cluster ID + CA fingerprint matching

## SDK Generation (ADR-033)

- [T] **Multi-language SDKs** (ADR-033) — Rust, TypeScript, .NET template-based generation
- [T] **Package registry publish** (ADR-033) — dry-run mode, cargo/npm/dotnet commands

## IaC & CLI (ADR-015, PRD 12)

- [V] **CLI binary** (ADR-015) — full clap subcommand tree, HTTP-only (no slice imports)
- [T] **Idempotent execution** (ADR-015) — dedup via correlation_id

---

## ADR-047: Volume Snapshots & Offsite Backup

- [V] **Snapshot create/list/restore/delete** — LocalSnapshotManager with fs::copy_dir_all
- [T] **S3 backup upload** — S3BackupClient PUT with AES-256 encryption
- [T] **S3 backup download** — GET from S3 endpoint
- [T] **S3 backup delete** — DELETE from S3 endpoint
- [T] **S3 list_backups()** — quick-xml ListBucketResult parsing implemented
- [V] **Volume CLI** — `picloud volume snapshots/backups/restore` commands
- [T] **BackupCompleted projection** — RDF triples for backup records

## ADR-048: Native Ingress Router

- [V] **IRI routing** — route_iri() with 10+ path variants, all resources dereferenceable
- [V] **Content negotiation** — ContentType::from_accept() for Turtle/JSON-LD/JSON/HTML
- [T] **Proxy forwarding** — reqwest-based with timeouts and 502/503
- [T] **DrainState struct** — AtomicBool + Notify, 30s grace period
- [V] **Drain enforcement** — DrainState wired into AppState, checked in handle_ingress_proxy
- [T] **TLS/SNI cert issuance** — ensure_cert() calls platform CA sign_ingress_cert()
- [T] **ResolvesServerCert** — rustls SNI resolver implemented with PEM parsing

## ADR-049: .picloud Compiler & Ontology

- [V] **Parser** — HCL-like .picloud to ParsedFile/ResourceDeclaration
- [V] **Compiler** — ParsedFile to Turtle RDF with proper IRIs
- [V] **Offline validator** — required fields + cross-reference checks
- [V] **platform.ttl** — OWL ontology with 40+ classes and properties
- [V] **shapes.ttl** — SHACL shapes for 13 resource types
- [T] **Embedded ontology** — include_str!() for platform.ttl + shapes.ttl
- [V] **SHACL validation** — Oxigraph-based sh:minCount validation via SPARQL
- [T] **Online validation** — cross-resource reference checks via HEAD requests
- [V] **Docs generator** — generates markdown per resource type with property tables

## ADR-050: Builder Pattern CLI

- [V] **picloud new** — product, container, binary, volume, feature-flag, config, inference-rule, ingress, group
- [V] **ResourceBuilder** — fluent API with required field validation
- [V] **Overwrite protection** — refuses without --overwrite flag

## ADR-051: Product IAM — Roles, Scopes, Audience

- [V] **OIDC discovery** — .well-known/openid-configuration + JWKS
- [V] **Token endpoint** — client_credentials grant, proper OAuth errors
- [T] **Token exchange** (RFC 8693) — audience + scopes + actor claim
- [T] **resolve_roles()** — role resolution with flattened permissions
- [T] **RoleAssigned/RoleRevoked events** — domain event types defined
- [T] **TokenExchanged event** — domain event type defined
- [T] **OWL role inheritance** — SPARQL-based rdfs:subClassOf* traversal with permission flattening
- [V] **M2M client_credentials** — scope validation against registered scopes, audience in token
- [V] **Audience validation middleware** — enforced on product-scoped endpoints via validate_product_audience()

## ADR-052: Integrated DNS Server

- [V] **DNS cache** — 30s TTL, hostname-keyed invalidation, eviction task
- [T] **DNS resolver** — cache-first, SPARQL query fallback, authority check
- [T] **DNS record types** — A, SRV, TXT, PTR with SPARQL templates
- [T] **Event-driven invalidation** — 8 event types trigger cache invalidation
- [V] **DNS server startup** — binds port 53, logs permission warning
- [V] **DNS wire format** — hickory-proto Message parsing, query dispatch, response serialization
- [ ] **Pi-hole integration** — config hint function exists, not tested

## ADR-053: Node Certificate Enrollment

- [V] **ClusterCa** — rcgen CA generation with AES-256-GCM key encryption
- [T] **Node CSR signing** — sign_node_csr with 90-day lifetime
- [T] **Workload cert signing** — sign_workload_cert with 24h lifetime
- [T] **Ingress cert signing** — sign_ingress_cert with 90-day lifetime
- [T] **Enrollment tokens** — issue/consume/revoke with TTL and single-use
- [T] **Auto enrollment mode** — EnrollmentMode::Auto path
- [T] **Token enrollment mode** — EnrollmentMode::Token path
- [T] **Revocation list** — CRL with fingerprint-based lookup
- [V] **EnrollmentMode in ClusterIdentity** — Auto/Token with serde default
- [T] **Cert renewal dispatch** — IRI-based cert type detection, CA method dispatch
- [V] **Enrollment HTTP endpoint** — POST /api/enroll-node with CSR signing via platform CA

## ADR-054: Embedded OCI Registry

- [T] **Domain types** — ImagePushed/Deleted/TagUpdated, RegistryGC events, Registry/Repository resources
- [T] **Registry trait** — RegistryBackend in picloud-domain traits with 11 methods
- [T] **picloud-registry crate** — LocalRegistryBackend with fs-backed blob store
- [T] **OCI Distribution API** — v2 manifest/blob/tag endpoints via /v2/* wildcard
- [T] **Blob storage** — content-addressed sha256, digest verification, auto-dedup
- [T] **Garbage collection** — mark-and-sweep with manifest walking
- [T] **RDF projection** — Oxigraph projector handles ImagePushed/Deleted/TagUpdated/GC events, 6 tests
- [T] **IAM integration** — bearer token validation on /v2/* endpoints, OCI WWW-Authenticate, 9 tests
- [T] **Compiler warning** — warn on non-local registry image refs in validate_offline
- [T] **CLI commands** — picloud image push/import/list/inspect/delete, registry gc/status
- [T] **Composition root wiring** — registry in src/main.rs with env config

## ADR-054: Test-Augmented ADR Template

- [T] **ADR test coverage completeness** — scenario parses ADRs, asserts Test coverage section present
- [T] **Scenario catalogue sync** — scenario verifies every ADR-named .rs file exists

## ADR-055: Capability as a First-Class Interface Contract

- [T] **Capability resource type** — cluster-scoped, version/ontology/shapes/input+output events
- [T] **CapabilityDependency type** — capability name + minVersion on Product
- [T] **Product `implements` field** — list of capability@version refs
- [T] **Product `capabilities` field** — list of CapabilityDependency objects
- [T] **Capability events** — CapabilityDeclared/Ready/ImplementorAdded/Removed/Unfulfilled/Deleted/RoutingFailed
- [T] **Capability error variants** — NotFound/Unfulfilled/DeletionBlocked/ShaclConformanceFailed/RoutingFailed
- [T] **CapabilityResolver trait** — resolve_implementor, route_capability_event, list_capabilities
- [T] **RDF projection** — CapabilityDeclared/Ready/ImplementorAdded/Removed/Deleted projected to Oxigraph
- [T] **Parser declarations** — capability type in JSON and bicep .picloud files with validation
- [T] **IRI routing** — /capabilities/{name} route
- [T] **HTTP apply** — CapabilityDeclared event emitted on resource apply
- [T] **CLI** — `picloud capability list` via SPARQL
- [T] **Compiler** — capability maps to Capability RDF class
- [ ] **Capability-aware event routing** — platform routes input events to highest-version implementor
- [ ] **SHACL conformance check** — validate implements declarations at deploy time
- [ ] **Capability consumer blocking** — block deploy if required capability unfulfilled

## ADR-056: Data Products and Data Domains

- [T] **DataDomain resource type** — cluster-scoped, steward/sensitivity/description
- [T] **DataProduct resource type** — product-scoped, projection/freshness/access/domain
- [T] **DataProductDependency type** — source + minVersion on Product
- [T] **Product `dataProducts` field** — list of DataProductDependency objects
- [T] **FreshnessConfig / DataProductAccess types** — maxAge, triggers, visibility, roles
- [T] **DataSensitivity enum** — Public/Internal/Confidential/Restricted
- [T] **Data domain events** — DataDomainDeclared/Deleted
- [T] **Data product events** — DataProductDeclared/Ready/Refreshed/SLOBreached/SLORestored/Deleted
- [T] **Data product error variants** — DomainNotFound/DomainDeletionBlocked/ProductNotFound/DeletionBlocked/CrossProductGraphAccessDenied
- [T] **DataProductProjector trait** — refresh_projection, query_data_product
- [T] **DataProductSLOMonitor trait** — check_freshness with breach/restore actions
- [T] **RDF projection** — DataDomainDeclared/Deleted/DataProductDeclared/Refreshed/Deleted projected
- [T] **Parser declarations** — data-domain and data-product types in JSON and bicep with validation
- [T] **IRI routing** — /data-domains/{name} and /products/{p}/data-products/{n}/graph routes
- [T] **HTTP apply** — DataDomainDeclared/DataProductDeclared events emitted on resource apply
- [T] **CLI** — `picloud data-domain list` and `picloud data-product list` via SPARQL
- [T] **Compiler** — DataDomain/DataProduct map to RDF classes
- [T] **IriBuilder** — cluster_resource() and data_product_graph() methods
- [ ] **Projection runner** — SPARQL CONSTRUCT on trigger events, atomic graph swap
- [ ] **Freshness monitor** — maxAge tracking, SLOBreached/Restored events
- [ ] **Cross-product internal graph access blocked** — 403 for non-owner, non-admin identities
- [ ] **Consumer dependency validation** — block deploy if referenced data product missing

## ADR-055 (legacy): Staged Platform Upgrade via Isolated Staging Cluster

- [T] **Upgrade compatibility scenario** — triple count comparison pre/post upgrade
- [T] **Rolling upgrade sequence scenario** — followers-first order, leader quorum polling
- [T] **Upgrade gate enforcement scenario** — mock-fail scenario halts pipeline
- [x] **Rolling upgrade subcommand** — `picloud-test upgrade --binary <path>`, SSH/SCP-based

## ADR-056 (legacy): Shared Hardware Staging with Port Isolation

- [T] **Server config file** — `--config <path>` TOML with all ports/storage/domain configurable
- [T] **Shared hardware isolation scenario** — SPARQL cross-contamination check
- [T] **Port non-conflict scenario** — systemd service + HTTP health check on both ports

## ADR-057 (removed): OTel Test Data Versioning via Resource Attributes

- [x] **DNS TXT version lookup** — hickory-resolver TXT query with fallback chain
- [x] **OTel tracer init** — resource attributes per ADR-057, OTLP export
- [T] **Version attribute present scenario** — asserts picloud.platform_version on all spans
- [T] **Version matches cluster scenario** — asserts version matches DNS TXT record

## ADR-058 (removed): picloud-test as First-Class Workspace Crate

- [T] **Crate scaffold** — workspace member, binary, harness, config, scenarios, invariants, probes
- [T] **Test crate builds independently scenario** — cargo check -p picloud-test
- [T] **No internal imports scenario** — Cargo.toml dependency audit
- [x] **Scenario runner** — Scenario trait, TestContext, filter, OTel recording
- [x] **Assertion helpers** — SPARQL, HTTP, DNS lookup, count/status assertions
- [x] **Leader invariant** — SPARQL leader presence check

---

## PRD Gaps (not covered by any ADR)

- [ ] **Node drain** (PRD 10) — graceful workload migration before node removal
- [ ] **Multi-node Raft persistence** — in-memory log store only (disk deferred)
- [ ] **NFS mount support** (PRD 9) — NAS backup via NFS subprocess mount
- [ ] **Rate limiting** (PRD 11) — no HTTP rate limiting
- [ ] **Audit log** (PRD 8) — security audit events not implemented
- [ ] **Backup encryption key rotation** — single key, no rotation
- [ ] **Cross-node snapshot restore** — restore only works on origin node

---

## Legend

| Symbol | Meaning |
|---|---|
| `[ ]` | Not started |
| `[~]` | Partial implementation or stub |
| `[x]` | Implemented (code exists, compiles) |
| `[T]` | Unit tested (tests pass) |
| `[V]` | Verified on Pi cluster (E2E tested) |
