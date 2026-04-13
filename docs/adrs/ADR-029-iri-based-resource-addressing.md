---
id: ADR-029
title: IRI-Based Resource Addressing
status: accepted
features: [FT-001]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** RDF is an HTTP-native technology. IRIs (Internationalized Resource Identifiers) are both the unique identifier and the dereferenceable location of every RDF resource. If the platform assigns opaque internal IDs to resources rather than IRIs, RDF tooling cannot navigate the graph by following links. The cluster graph becomes a closed system rather than a Linked Data platform.

**Decision:** Every resource in PiCloud has a canonical IRI rooted at the cluster domain (`picloud.local` by default). IRIs follow a path-based hierarchy that reflects the resource model. Every IRI is dereferenceable over HTTPS. The platform serves RDF representations at every resource IRI via HTTP content negotiation.

**IRI scheme — path-based (not subdomain-based):**
```
https://picloud.local/                                           # cluster root
https://picloud.local/nodes/{node-name}                         # node
https://picloud.local/products/{product-name}                   # product
https://picloud.local/products/{product-name}/{type}/{name}     # resource
https://picloud.local/products/{product-name}/graph             # SPARQL endpoint
https://picloud.local/products/{product-name}/ontology          # ontology
https://picloud.local/products/{product-name}/events            # event stream
```

**Content negotiation at every IRI:**
```
Accept: text/turtle            → Turtle RDF representation
Accept: application/ld+json    → JSON-LD representation
Accept: application/json       → Plain JSON representation
Accept: text/html              → Human-readable view (future portal)
```

**Why path-based over subdomain-based:**
- Aligned with Linked Data and REST conventions — the path hierarchy reflects the resource hierarchy
- Single TLS certificate per product scope — no wildcard certificates required
- IRIs are meaningful by inspection — the path encodes type and ownership
- Subdomain-per-resource would require a wildcard cert and would not convey hierarchy

**Rationale:**
- RDF tools, SPARQL clients, and LLMs can navigate the entire cluster by dereferencing IRIs and following links — the cluster is a Linked Data platform by construction
- The cluster root IRI returns a description of all Products and their IRI spaces — self-documenting without any additional service catalog
- DNS and HTTP are the lowest common denominator for interoperability — any client that speaks HTTP can interact with the platform
- IRI stability (resources keep their IRI when rescheduled) means RDF triples in external systems remain valid
- Content negotiation means the same IRI serves both machine consumers (Turtle, JSON-LD) and future human interfaces (HTML)

**Rejected alternatives:**
- **Opaque internal IDs (UUIDs)** — breaks RDF Linked Data navigation; external tools and LLMs cannot follow links to explore the cluster.
- **Subdomain-based addressing** — requires wildcard TLS certificates, does not convey resource hierarchy, and does not align with Linked Data conventions.

**Consequences:**
- The platform must run an HTTP server on every node serving the canonical IRI space
- The internal DNS resolver must resolve `picloud.local` to the cluster ingress
- TLS certificates must be issued for `picloud.local` by the platform's built-in CA — external clients need to trust this CA
- Resource IRIs must be assigned at declaration time and remain stable for the lifetime of the resource