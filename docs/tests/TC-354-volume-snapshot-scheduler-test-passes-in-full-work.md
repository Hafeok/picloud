---
id: TC-354
title: Volume snapshot scheduler test passes in full workspace cargo test run
type: scenario
status: failing
validates:
  features:
  - FT-033
  adrs:
  - ADR-047
phase: 4
runner: cargo-test
runner-args: tc354_volume_snapshot_scheduler_deterministic_under_workspace
last-run: 2026-04-18T11:08:58.593422020+00:00
last-run-duration: 0.6s
failure-message: "No matching test function found (0 tests ran)"
---

## Description

Regression guard for flaky behavior observed on the Pi 5 build node
(node3, 2026-04-17): running `cargo test --workspace` at the repo root
reported a single failure in
`implementation::tests::tc246_volume_snapshot_created_on_schedule_and_restorable_to_new_volume`
(1 failed / 24 passed), but running the same test in isolation
(`cargo test --workspace tc246`) passed. This is a test-isolation or
shared-state bug in the snapshot scheduler test — not a defect in the
scheduler itself.

**Invariant under test:** `tc246_volume_snapshot_created_on_schedule_and_restorable_to_new_volume`
must succeed deterministically whether invoked in isolation or alongside
the full workspace test suite, including parallel execution of neighboring
snapshot / volume tests in the same binary.

**Shape of the Rust test:**

1. Identify the global or process-scoped state that TC-246's implementation
   shares with siblings — e.g. a static clock, a `/tmp` path not qualified
   by a unique test id, a `OnceLock` scheduler, or a filesystem directory
   reused across tests.
2. Add an adversarial test that runs TC-246's body concurrently with the
   sibling tests that were present in the failing run
   (`tc248`, `tc250`, other `tc2XX` snapshot tests in the same file), using
   `#[tokio::test(flavor = "multi_thread")]` or a spawned helper.
3. Assert TC-246 produces the same assertions in that adversarial run.
4. The fix should isolate shared state (unique tempdirs per test, per-test
   clock) so this test passes without requiring `--test-threads=1`.