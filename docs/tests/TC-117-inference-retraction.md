---
id: TC-117
title: inference_retraction
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-038
phase: 1
runner: cargo-test
runner-args: "inference_retraction"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.9s
failure-message: "No matching test function found (0 tests ran)"
---

clear the condition. Assert the produced triples are retracted from the graph and the corresponding resolved event is emitted within 2 seconds.