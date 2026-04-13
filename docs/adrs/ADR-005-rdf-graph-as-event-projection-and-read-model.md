---
id: ADR-005
title: RDF Graph as Event Projection and Read Model
status: accepted
features: [FT-002, FT-016, FT-094]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

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