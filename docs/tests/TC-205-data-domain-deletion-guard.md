---
id: TC-205
title: data_domain_deletion_guard
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
---

attempt to delete `data-domain 'geospatial'` while `photo-app/photo-locations` is assigned to it. Assert the delete is rejected with a member count error.