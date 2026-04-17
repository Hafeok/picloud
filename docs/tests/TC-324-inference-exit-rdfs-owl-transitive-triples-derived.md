---
id: TC-324
title: Inference exit — RDFS/OWL transitive triples derived
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc324_inference_exit_rdfs_owl_transitive_triples_derived"
validates:
  features: [FT-054]
  adrs: [ADR-039]
phase: 3
last-run: 2026-04-17T07:00:31.652247286+00:00
last-run-duration: 0.5s
---

## Description

Exit criteria: End-to-end validation of RDFS/OWL inference across platform
and product graphs:

1. RDFS subclass inference active after ontology deployment (depth-3 hierarchy)
2. OWL transitive-property closure inferred for depth-3 chains
3. Inference materialised during ontology load (automatic, no manual trigger)
4. Inferred triples queryable in both default and product named graphs
5. SPARQL queries automatically include inferred triples
6. Multiple transitive properties coexist correctly (routeConnects, containedIn, partOf)
7. Mixed RDFS + OWL inference in same ontology