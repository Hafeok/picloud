---
id: TC-204
title: data_product_deletion_guard
type: scenario
status: passing
validates:
  features:
  - FT-066
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_product_deletion_guard
last-run: 2026-04-17T09:17:31.910412362+00:00
last-run-duration: 0.6s
---

attempt to delete `data-product 'photo-locations'` while `maps-app` declares a `dataProducts` dependency on it. Assert the delete is rejected. Assert the data product and its named graph remain intact.