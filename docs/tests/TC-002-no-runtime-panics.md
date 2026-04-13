---
id: TC-002
title: no_runtime_panics
type: scenario
status: passing
validates:
  features: []
  adrs:
  - ADR-001
phase: 1
runner: picloud-test
runner-args: "no_runtime_panics"
---

the full scenario harness runs to completion. Any Rust `panic!` in the binary is captured by the test runner and counted as a test failure.