---
id: TC-297
title: CLI exit — cluster init, resource apply, resource status complete
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc297_cli_exit_cluster_init_resource_apply_resource_status"
validates:
  features: [FT-022]
  adrs: []
phase: 1
last-run: 2026-04-13T21:20:38.946040726+00:00
---

## Description

Exit-criteria gate test for FT-022. Verifies all four CLI operations produce correct HTTP responses and leave the system in a consistent state:

- **cluster init** returns valid cluster metadata with domain
- **cluster status** (health endpoint) responds OK
- **resource apply** creates 3 resources (product + volume + container), all in declared status
- **resource status** returns projected resources with correct types (Volume, Container) from the RDF graph
- **identity create** is accepted and stored in the event log
- **Final state**: event log contains all 4 events, and the product is discoverable via SPARQL