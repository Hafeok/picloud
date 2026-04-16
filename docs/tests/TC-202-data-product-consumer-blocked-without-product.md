---
id: TC-202
title: data_product_consumer_blocked_without_product
type: scenario
status: failing
validates:
  features:
  - FT-069
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_product_consumer_blocked_without_product
last-run: 2026-04-15T14:29:59.558362753+00:00
last-run-duration: 0.6s
failure-message: "No matching test function found (0 tests ran)"
---

attempt to deploy `maps-app` with a `dataProducts` dependency on `photo-app/photo-locations` when that data product does not exist. Assert `resource apply` fails with a `DataProductNotFound` error. Assert `maps-app` is not deployed.