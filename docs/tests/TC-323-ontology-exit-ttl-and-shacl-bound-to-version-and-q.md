---
id: TC-323
title: Ontology exit — .ttl and .shacl bound to version and queryable
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc323_ontology_exit_ttl_and_shacl_bound_to_version_and_queryable"
validates:
  features: [FT-053]
  adrs: [ADR-023]
phase: 3
last-run: 2026-04-15T13:10:51.914325866+00:00
last-run-duration: 0.5s
---

## Description

Exit criteria for FT-053: End-to-end validation that ontology resources (.ttl and .shacl) are
first-class platform resources with full lifecycle support.

Validates:
1. ResourceDeclared projects Ontology type with metadata (filePath, format, servedAt)
2. ProductDeployed creates versioned ontology IRI
3. OntologyLoaded loads Turtle/SHACL content into RDF graph
4. Turtle classes (rdfs:Class) are queryable via SPARQL
5. SHACL shapes (sh:NodeShape, sh:targetClass, sh:property) are queryable
6. RDFS subclass inference materialises instance types (rdfs9)
7. Both .ttl and .shacl formats work
8. Ontology triples land in the product's named graph
9. Ontology resources are bound to versioned IRIs
10. Cross-cutting SPARQL counts all ontology resources
11. CONSTRUCT queries return ontology metadata