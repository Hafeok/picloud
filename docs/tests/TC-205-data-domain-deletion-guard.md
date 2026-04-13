---
id: TC-205
title: data_domain_deletion_guard
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: "data_domain_deletion_guard"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

attempt to delete `data-domain 'geospatial'` while `photo-app/photo-locations` is assigned to it. Assert the delete is rejected with a member count error.