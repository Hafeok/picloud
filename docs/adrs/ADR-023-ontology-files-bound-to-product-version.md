---
id: ADR-023
title: Ontology Files Bound to Product Version
status: accepted
features: [FT-008, FT-053]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** A Product's RDF graph has a schema. That schema may evolve as the Product evolves. Consumers need to know which schema they are querying.

**Decision:** Ontology files (`.ttl` or `.shacl`) are declared as `ontology` resources in the Product's resource file and bound to the Product version. The platform serves the ontology file from the cluster graph. When a new Product version is deployed, the ontology is updated atomically with the rest of the Product's resources.

**Rationale:**
- Schema and implementation are versioned together — no schema/implementation drift
- Consumers can discover the exact schema for any Product version from the cluster graph
- SHACL files provide validation shapes — the platform can optionally validate graph updates against them

**Rejected alternatives:**
- **Unversioned ontology (latest only)** — consumers cannot know which schema they are querying when the ontology changes, breaking backward compatibility.
- **Ontology managed outside the product lifecycle** — decouples schema from implementation, enabling drift between what the product stores and what consumers expect.