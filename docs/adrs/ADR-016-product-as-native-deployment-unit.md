---
id: ADR-016
title: Product as Native Deployment Unit
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Workloads need a deployment boundary — a unit that groups related resources, provides an IAM scope, and has a lifecycle (deploy, update, delete). Without this, operators manage individual resources with no grouping concept.

**Decision:** Every workload in PiCloud is deployed as a Product. A Product is a versioned, hermetically sealed deployment boundary. It groups all resources needed for an application: containers, volumes, identities, RDF stores, event subscriptions, and ontologies. Deleting a Product cascades deletion to all its resources.

**Rationale:**
- Maps directly to how developers think about applications — "deploy the photo app", not "deploy container A and volume B and identity C"
- Versioning is built into the Product concept — a Product at version 1.0.0 is a distinct identity from 1.1.0
- IAM scoping per Product means access control is at the application level, not the resource level
- Cascading deletion prevents orphaned resources
- One active version per Product prevents version sprawl and simplifies the operational model