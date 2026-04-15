---
id: TC-264
title: Per-product Oxigraph instance created and serves SPARQL
type: scenario
status: passing
runner: cargo-test
runner-args: "tc264_per_product_oxigraph_instance_created_and_serves_sparql"
validates:
  features: [FT-051]
  adrs: [ADR-006, ADR-019]
phase: 3
last-run: 2026-04-15T12:50:34.156564048+00:00
last-run-duration: 0.7s
---

## Description

Scenario: a product declares an `rdf-store` resource, the platform creates a
per-product Oxigraph instance, and the product can issue SPARQL queries and
updates against that store in isolation from other products.

Verifies:
- RdfStore resource is projected with correct type (`picloud:RdfStore`)
- SPARQL endpoint and backing volume IRIs are stored as triples
- Per-product Oxigraph instance is created and isolated
- SPARQL INSERT DATA / SELECT / ASK all work against the product store
- Stores are isolated — one product cannot see another's data
- Store can be dropped cleanly