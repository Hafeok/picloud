---
id: ADR-006
title: Oxigraph as Embedded Triplestore
status: accepted
features: [FT-002, FT-016]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Both the platform's internal state (ADR-005) and application RDF storage require an embedded triplestore. The triplestore must be embeddable in a Rust binary, support SPARQL 1.1, and run on ARM64.

**Decision:** Use Oxigraph as the embedded triplestore for both platform state and per-product RDF stores.

**Rationale:**
- Pure Rust — embeds directly into the PiCloud binary, no separate process
- Full SPARQL 1.1 support including SPARQL Update
- Named graph support — platform and per-product graphs coexist in one instance
- Actively maintained
- Consistent with ADR-001 (Rust stack)

**Rejected alternatives:**
- **Apache Jena** — JVM dependency. Ruled out.
- **RDFox** — not open source.
- **External triplestore (Fuseki, Stardog)** — external process dependency. Violates single-binary goal.