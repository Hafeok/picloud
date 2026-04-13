---
id: TC-204
title: data_product_deletion_guard
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
---

attempt to delete `data-product 'photo-locations'` while `maps-app` declares a `dataProducts` dependency on it. Assert the delete is rejected. Assert the data product and its named graph remain intact.