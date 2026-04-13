---
id: ADR-039
title: Embedded RDFS/OWL Inference via Oxigraph
status: accepted
features: [FT-009, FT-054]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Beyond SPARQL CONSTRUCT rules (ADR-038), structural knowledge about the platform's ontology should be automatically materialised. Class hierarchies, property inheritance, and equivalences declared in `.ttl` ontology files should produce inferred triples without requiring explicit CONSTRUCT rules for each case.

**Decision:** Oxigraph's built-in RDFS inference is enabled on the platform graph and all product named graphs. OWL 2 RL axioms declared in ontology files are automatically applied. Inferred triples are materialised alongside asserted triples and are queryable via SPARQL.

**What this gives for free:**

*RDFS subclass inference:*
```turtle
picloud:ProductionContainer rdfs:subClassOf picloud:Container .
```
Any SPARQL query for `picloud:Container` automatically includes `picloud:ProductionContainer` instances — no CONSTRUCT rule needed.

*OWL property transitivity:*
```turtle
picloud:dependsOn rdf:type owl:TransitiveProperty .
```
If `photo-app dependsOn user-service` and `user-service dependsOn auth-service`, the reasoner infers `photo-app dependsOn auth-service`.

*Ontology-driven IAM:*
```turtle
picloud:AdminRole rdfs:subClassOf picloud:OperatorRole .
```
Any permission check for `picloud:OperatorRole` automatically applies to admins.

**Scope:** RDFS inference + OWL 2 RL profile. Full OWL DL reasoning is explicitly out of scope — it is computationally intractable for a live platform.

**Rationale:**
- Zero additional infrastructure — Oxigraph handles this natively (ADR-006)
- Ontology files already deployed with products (ADR-023) — RDFS/OWL axioms are declared there
- Structural inference is always live — no schedule, no trigger, no rule to maintain
- Complements ADR-038 — RDFS/OWL handles structural facts, CONSTRUCT handles operational rules

**Rejected alternatives:**
- **External reasoner (Pellet, HermiT)** — adds a JVM dependency and network hop for inference, contradicting the single-binary zero-dependency model.
- **No inference** — loses subclass hierarchies and property inheritance that make the RDF graph navigable and self-describing.

**Consequences:**
- Product ontology authors must understand RDFS/OWL 2 RL — this is documented in the SDK
- Inferred triples increase graph size — Oxigraph's materialisation must be monitored
- Ontology changes (new subclass declarations) take effect immediately on deployment