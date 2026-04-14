---
id: TC-245
title: Delete Product cascades to all child containers, volumes, and identities
type: scenario
status: passing
runner: cargo-test
runner-args: "tc245_delete_product_cascades_to_all_child_containers_volumes_and_identities"
validates:
  features: [FT-031]
  adrs: []
phase: 2
last-run: 2026-04-14T08:03:06.890546797+00:00
---

## Description

Deploy a product with containers, volumes, and workload identities.
Delete the product via a ProductDeleted event. Verify that every child resource
is removed from both the default graph and the product's named graph.