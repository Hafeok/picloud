---
id: TC-205
title: data_domain_deletion_guard
type: scenario
status: passing
validates:
  features:
  - FT-065
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_domain_deletion_guard
last-run: 2026-04-17T09:03:38.439227156+00:00
last-run-duration: 0.8s
---

attempt to delete `data-domain 'geospatial'` while `photo-app/photo-locations` is assigned to it. Assert the delete is rejected with a member count error.