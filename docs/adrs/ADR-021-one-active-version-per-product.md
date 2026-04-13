---
id: ADR-021
title: One Active Version Per Product
status: accepted
features: [FT-008, FT-024]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Products are versioned. A decision is needed on whether multiple versions can run simultaneously (for canary deployments, blue-green, etc.).

**Decision:** A Product has exactly one active version at any time. The version is part of the Product's identity. Multiple instances (implementations) of that version can run simultaneously, but they all run the same version.

**Rationale:**
- Eliminates version routing complexity — there is no traffic splitting, no canary percentage, no version-aware load balancing
- Simplifies the IAM model — Product-scoped tokens are always for the active version
- Ontology binding is unambiguous — there is always exactly one schema for a Product
- Consistent with the hermetic Product model — a Product is a well-defined, stable deployment unit

**Rejected alternatives:**
- **Multi-version with traffic splitting (canary/blue-green)** — adds version-aware routing, traffic splitting percentages, and version-scoped IAM complexity without clear benefit on a small Pi cluster.
- **In-place rolling update** — creates a window where mixed versions serve traffic simultaneously, complicating debugging, IAM, and ontology binding.

**Upgrade path:** Deploying a new Product version is an atomic cutover. The platform provisions all resources for the new version in full. Only when every resource reaches `ResourceReady` does the platform cut traffic over to the new version and tear down the old one. If any resource fails to reach `ResourceReady`, the deployment is aborted and the old version remains live. There is no partial cutover — the cluster is never in a state where two versions are simultaneously serving traffic.