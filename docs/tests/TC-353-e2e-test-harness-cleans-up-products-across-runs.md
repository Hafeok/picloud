---
id: TC-353
title: E2E test harness cleans up products across runs
type: scenario
status: failing
validates:
  features:
  - FT-007
  - FT-024
  adrs: []
phase: 4
runner: cargo-test
runner-args: tc353_e2e_harness_cleans_up_products_across_runs
last-run: 2026-04-17T19:13:27.759507317+00:00
last-run-duration: 0.6s
failure-message: "No matching test function found (0 tests ran)"
---

## Description

Regression guard for the `multi-node-persistence` and `multi-node-replication`
E2E scenarios, which both failed on the Pi 5 cluster (2026-04-17) with
`apply product failed (409 Conflict): product 'persist-test-e2e' already
exists — use picloud resource apply --overwrite or delete first` and the
equivalent for `repl-test-e2e`. The conflicts cascaded into five SKIPs
(`direct-network-blocked`, `ontology-served`, `product-sparql-endpoint`,
`sparql-iam-enforcement`, `sparql-direct-mtls`) because seeding failed.

**Invariant under test:** every scenario in the `picloud-test` harness that
seeds a test product must be able to run cleanly regardless of whether a
prior run aborted before teardown. Either (a) the harness applies products
with `--overwrite` by default, (b) the harness deletes any lingering
test-scoped product before seeding, or (c) each run uses a unique product
name so prior-run artifacts cannot collide.

**Shape of the Rust test:**

1. Build a temporary cluster config pointing at an in-process
   `picloud-server`.
2. Pre-seed the event log with a product whose name matches the harness's
   default test-scope identifier (e.g. `persist-test-e2e`).
3. Invoke the harness's product-seeding helper (the same path used by the
   failing scenarios).
4. Assert it succeeds and returns a usable Product handle, with no 409
   Conflict surfaced.
5. Repeat 2–4 with a different scenario's seed name to confirm the fix
   works for more than one collision.

A manual workaround (running `picloud resource delete product/<name>` before
the E2E run) must NOT be required.