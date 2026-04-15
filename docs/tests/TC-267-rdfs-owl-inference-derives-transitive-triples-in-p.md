---
id: TC-267
title: RDFS/OWL inference derives transitive triples in product graph
type: scenario
status: passing
runner: cargo-test
runner-args: "tc267_rdfs_owl_inference_derives_transitive_triples_in_product_graph"
validates:
  features: [FT-054]
  adrs: [ADR-039]
phase: 3
last-run: 2026-04-15T13:18:17.678816994+00:00
last-run-duration: 0.6s
---

## Description

Scenario test: Deploy a product with an ontology containing both RDFS subClassOf
hierarchies and OWL TransitiveProperty declarations. Verify that:

1. RDFS subclass inference derives transitive type triples (depth-3 chain:
   StagingContainer < ProductionContainer < Container)
2. OWL transitive-property inference derives transitive relationship triples
   (depth-3 chain: photo-app dependsOn user-service dependsOn auth-service dependsOn cert-service)
3. Inferred triples exist in the product's named graph
4. Transitive chains of depth >= 3 are fully materialised
5. SPARQL queries return both asserted and inferred triples