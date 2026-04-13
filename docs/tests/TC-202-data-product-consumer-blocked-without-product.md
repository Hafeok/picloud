---
id: TC-202
title: data_product_consumer_blocked_without_product
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
---

attempt to deploy `maps-app` with a `dataProducts` dependency on `photo-app/photo-locations` when that data product does not exist. Assert `resource apply` fails with a `DataProductNotFound` error. Assert `maps-app` is not deployed.