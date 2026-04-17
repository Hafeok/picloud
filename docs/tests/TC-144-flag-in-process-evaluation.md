---
id: TC-144
title: flag_in_process_evaluation
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-044
phase: 1
runner: cargo-test
runner-args: "flag_in_process_evaluation"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.8s
failure-message: "No matching test function found (0 tests ran)"
---

after SDK initialisation, measure flag evaluation latency. Assert all evaluations are in-process (zero network round-trips) after initial load.