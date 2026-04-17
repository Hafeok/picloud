---
id: TC-352
title: picloud-test cluster-wide suite is idempotent across repeated runs
type: scenario
status: failing
runner: cargo-test
runner-args: "tc352_picloud_test_suite_is_idempotent"
validates:
  features: []
  adrs: [ADR-054]
phase: 4
---

## Description

Regression guard for test-harness idempotency, triggered by the 2026-04-17
Pi 5 run where a second invocation of the full suite saw three extra
failures (`multi-node-persistence`, `multi-node-replication`) with
`409 Conflict: product '<name>' already exists` and five cascade
`SKIP — could not seed test product`.

**Invariant under test:** running `picloud-test run` twice back-to-back
against the same cluster must produce the same pass/fail outcome. Each
scenario that seeds state (products, volumes, workloads) must either:

1. Delete its seeded resources in teardown (preferred), or
2. Apply with `--overwrite` / PUT semantics so re-seeding is a no-op, or
3. Use a unique resource name per run (e.g. suffixed with test-run UUID).

**Shape of the Rust test:**

1. Point the harness at a live single-node cluster fixture.
2. Invoke the full suite twice in sequence without any manual cleanup.
3. Assert run2's pass/fail counts match run1's exactly, with zero
   `already exists` errors and zero `could not seed` skips.

Failing this test means the harness is leaking state and masking real
regressions behind pollution-caused failures.
