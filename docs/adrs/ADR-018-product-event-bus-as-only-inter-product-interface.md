---
id: ADR-018
title: Product Event Bus as Only Inter-Product Interface
status: accepted
features: [FT-008, FT-032, FT-084]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Products need to react to events in other Products (e.g. "when a user is created in user-service, create a profile in photo-app"). Direct network calls between Products would couple them tightly and make the dependency graph opaque.

**Decision:** Products cannot make direct network calls to each other. The only interfaces between Products are: (1) events emitted to the platform event bus, and (2) SPARQL queries against an explicitly exposed product graph. Both are declared as resources.

**Rationale:**
- Enforces loose coupling at the platform level, not just by convention
- All inter-product dependencies are visible in resource files — the dependency graph is auditable
- Event-driven communication enables temporal decoupling — the subscribing Product does not need to be running when the event is emitted
- Consistent with the event-sourcing foundation of the platform

**Rejected alternatives:**
- **Direct HTTP between products** — creates tight coupling, makes the dependency graph opaque, and prevents temporal decoupling between products.
- **Shared database between products** — violates product isolation, creates hidden data dependencies, and makes independent deployment impossible.

**Consequences:**
- Synchronous request-response between Products is not possible by design
- Cross-product data consistency is eventual, not immediate
- Teams building Products must design their domain events carefully — event schemas are a public API