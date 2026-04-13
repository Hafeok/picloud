---
id: ADR-019
title: Per-Product SPARQL Endpoint and Ontology Exposure
status: accepted
features: [FT-008, FT-051, FT-052, FT-074]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Products accumulate domain knowledge in their RDF stores. Other Products and operators need to query this knowledge. The schema of that knowledge needs to be discoverable without reading source code.

**Decision:** Every Product with an `rdf-store` resource gets an IAM-gated SPARQL 1.1 endpoint automatically. Every Product declares its ontology as a `.ttl` or `.shacl` file, which is bound to the Product version and served by the platform.

**Rationale:**
- SPARQL is the standard query language for RDF — no custom query API needed
- IAM-gating means SPARQL endpoints respect the same access control as all other resources
- Ontology files are the schema contract for a Product's graph — consumers can understand the domain before querying
- Binding ontology to Product version means consumers always know which schema they are querying

**Rejected alternatives:**
- **Custom query API per product** — reinvents a query language that SPARQL already provides, fragmenting the platform's data access model.
- **Shared cluster-level SPARQL only** — loses product-level IAM scoping and mixes product data in a single query surface, breaking isolation.