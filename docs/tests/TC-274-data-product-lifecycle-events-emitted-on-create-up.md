---
id: TC-274
title: Data product lifecycle events emitted on create, update, delete
type: scenario
status: passing
runner: cargo-test
runner-args: "tc274_data_product_lifecycle_events_emitted_on_create_update_delete"
validates:
  features: [FT-070]
  adrs: [ADR-056]
phase: 3
last-run: 2026-04-17T09:57:55.549312407+00:00
last-run-duration: 0.6s
---

## Description

Scenario test for data product lifecycle events (FT-070). Verifies that the three
core lifecycle events — DataProductDeclared (create), DataProductUpdated (update),
and DataProductDeleted (delete) — are properly emitted and projected into the RDF
catalog. Checks that metadata (name, product, domain, version, maxAge) is correctly
set on creation, atomically updated on modification, and fully removed on deletion.