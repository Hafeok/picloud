---
id: ADR-011
title: Block Storage Before RDF Application Storage
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Both block storage and RDF application storage (per-product Oxigraph) are in scope. Block storage is a dependency for RDF storage (Oxigraph needs a persistent block volume). Implementing both simultaneously adds unnecessary complexity to Phase 1.

**Decision:** Block storage is implemented in Phase 1. Per-product RDF storage is implemented in Phase 3.

**Rationale:**
- Block storage is a dependency of RDF storage — correct ordering
- Block storage is needed for containers in Phase 1 regardless
- Phasing reduces the surface area of Phase 1 to the minimum needed for a working cluster
- RDF application storage builds on the same block storage primitives — no rework required