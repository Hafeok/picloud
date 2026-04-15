---
id: TC-205
title: data_domain_deletion_guard
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_domain_deletion_guard
last-run: 2026-04-15T14:29:59.558362753+00:00
last-run-duration: 0.5s
failure-message: "No matching test function found (0 tests ran)"
---

attempt to delete `data-domain 'geospatial'` while `photo-app/photo-locations` is assigned to it. Assert the delete is rejected with a member count error.