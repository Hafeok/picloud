---
id: TC-266
title: Ontology .ttl and .shacl files bound to product version and queryable
type: scenario
status: passing
runner: cargo-test
runner-args: "tc266_ontology_ttl_and_shacl_files_bound_to_product_version_and_queryable"
validates:
  features: [FT-053]
  adrs: [ADR-023]
phase: 3
last-run: 2026-04-17T06:59:34.302056672+00:00
last-run-duration: 0.5s
---

## Description

Scenario test for ontology resource type (.ttl and .shacl files bound to product version).

Validates the full ontology lifecycle:
1. Deploy product with version
2. Declare Ontology resources (both Turtle and SHACL formats) with metadata (file_path, format, served_at)
3. Load ontology content (Turtle triples and SHACL shapes)
4. Verify ontology resources are projected with correct rdf:type (picloud:Ontology)
5. Verify ontology metadata (filePath, format) is queryable via SPARQL
6. Verify Turtle classes (rdfs:Class) are loaded and queryable
7. Verify SHACL shapes (sh:NodeShape, sh:targetClass) are loaded and queryable
8. Verify RDFS subclass inference is materialised
9. Verify versioned ontology IRI is bound to the product
10. Verify ontology triples exist in the product's named graph
11. Verify ontology resources are bound to the versioned IRI