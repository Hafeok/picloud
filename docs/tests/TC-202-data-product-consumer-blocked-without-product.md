---
id: TC-202
title: data_product_consumer_blocked_without_product
type: scenario
status: passing
validates:
  features:
  - FT-069
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_product_consumer_blocked_without_product
last-run: 2026-04-17T09:57:30.023726834+00:00
last-run-duration: 0.7s
---

attempt to deploy `maps-app` with a `dataProducts` dependency on `photo-app/photo-locations` when that data product does not exist. Assert `resource apply` fails with a `DataProductNotFound` error. Assert `maps-app` is not deployed.