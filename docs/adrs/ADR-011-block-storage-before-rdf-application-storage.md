---
id: ADR-011
title: Block Storage Before RDF Application Storage
status: accepted
features:
- FT-004
- FT-018
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:0be00a0212314fd603370a358326a2f001260c9c4c4093036ffcd54d0fadb9c0
---

**Status:** Accepted

**Context:** Both block storage and RDF application storage (per-product Oxigraph) are in scope. Block storage is a dependency for RDF storage (Oxigraph needs a persistent block volume). Implementing both simultaneously adds unnecessary complexity to Phase 1.

**Decision:** Block storage is implemented in Phase 1. Per-product RDF storage is implemented in Phase 3.

**Rationale:**
- Block storage is a dependency of RDF storage — correct ordering
- Block storage is needed for containers in Phase 1 regardless
- Phasing reduces the surface area of Phase 1 to the minimum needed for a working cluster
- RDF application storage builds on the same block storage primitives — no rework required

**Rejected alternatives:**
- **Parallel implementation** — implementing both simultaneously increases Phase 1 surface area and risks delays in the core block storage path that other capabilities depend on.
- **RDF storage first** — RDF storage depends on block storage for persistence; reversing the order would require temporary in-memory-only storage that is later replaced.