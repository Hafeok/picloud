---
id: ADR-028
title: Low Coupling, High Cohesion as a Structural Platform Constraint
status: accepted
features: [FT-006, FT-074]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

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

**Rejected alternatives:**
- **Coupling by convention** — relying on developer discipline rather than platform enforcement means coupling will inevitably appear as the number of products grows.
- **Shared service layer between products** — a shared service creates a coupling point that defeats the independence of the product model.