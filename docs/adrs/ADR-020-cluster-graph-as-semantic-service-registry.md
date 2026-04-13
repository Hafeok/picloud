---
id: ADR-020
title: Cluster Graph as Semantic Service Registry
status: accepted
features: [FT-005, FT-085]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** As the number of Products grows, operators need to discover what Products exist, what events they emit, what graphs they expose, and what ontologies they declare — without reading source files.

**Decision:** The cluster-level RDF graph is a semantic service registry. It contains all Products, their versions, their SPARQL endpoints, their subscribable event types, and their ontology declarations. All of this is queryable via SPARQL.

**Rationale:**
- The cluster is self-documenting by construction — no separate service catalog required
- LLMs can query the cluster graph to understand the deployed system before generating code
- New Products can discover existing Products' interfaces through graph queries
- Consistent with RDF as the universal data model for the platform

**Rejected alternatives:**
- **Separate service catalog (Consul-style)** — introduces a separate system with its own data model when the RDF graph already contains all the necessary information.
- **No service registry** — operators would need to read source files or configuration to discover deployed products and their interfaces.