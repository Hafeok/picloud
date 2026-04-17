---
id: FT-054
title: RDFS/OWL inference enabled on platform and product graphs
phase: 3
status: planned
depends-on: []
adrs:
- ADR-039
- ADR-006
tests:
- TC-267
- TC-324
domains: []
domains-acknowledged: {}
---

## Description

RDFS and OWL 2 RL inference is enabled on both the platform graph and all product named graphs (ADR-039). Oxigraph's built-in reasoner materializes inferred triples from ontology axioms declared in `.ttl` files.

### What inference provides

**RDFS subclass inference:**
```turtle
picloud:ProductionContainer rdfs:subClassOf picloud:Container .
```
A query for `?x a picloud:Container` automatically includes `picloud:ProductionContainer` instances.

**OWL property transitivity:**
```turtle
picloud:dependsOn rdf:type owl:TransitiveProperty .
```
If A dependsOn B and B dependsOn C, the reasoner infers A dependsOn C.

**Ontology-driven IAM:**
```turtle
picloud:AdminRole rdfs:subClassOf picloud:OperatorRole .
```
Permission checks for `picloud:OperatorRole` automatically apply to admins.

### Scope

- **RDFS inference** — subclass hierarchies, property inheritance, domain/range propagation
- **OWL 2 RL** — transitive properties, equivalences, intersections, inverse properties
- **Excluded** — full OWL DL reasoning is out of scope (computationally intractable for a live platform)

### Interaction with SPARQL CONSTRUCT rules

Two inference layers work together:
- **RDFS/OWL** (this feature) — structural facts from ontology axioms. Always live, no trigger needed.
- **SPARQL CONSTRUCT rules** (FT-057) — operational rules for group membership, alerts, derived state. Event-driven with reconciliation.

### Behaviour

- Inference is always active — inferred triples are materialized immediately when ontology files are loaded
- Ontology changes (new subclass declarations) take effect on Product deployment
- Inferred triples are queryable alongside asserted triples — SPARQL queries see both transparently
