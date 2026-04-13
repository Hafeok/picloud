---
id: TC-240
title: CLI cluster init and resource apply create resources end-to-end
type: scenario
status: passing
runner: cargo-test
runner-args: "tc240_cli_cluster_init_and_resource_apply"
validates:
  features: [FT-022]
  adrs: []
phase: 1
last-run: 2026-04-13T21:20:38.946040726+00:00
---

## Description

End-to-end scenario exercising the four primary CLI commands against a live HTTP server:

1. **cluster init** — GET / returns valid cluster metadata (type, domain)
2. **resource apply** — POST /api/apply with a product, volume, and container; all three resources are declared and stored as events
3. **resource status** — GET /products/:name returns the product with projected child resources (Volume, Container) from the RDF graph
4. **identity create** — POST /api/commands with IdentityCreated; the command is accepted and the event is appended to the log

Validates that state flows correctly through the event log → RDF projection → SPARQL query pipeline.