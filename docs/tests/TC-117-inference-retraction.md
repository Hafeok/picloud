---
id: TC-117
title: inference_retraction
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-038
phase: 1
runner: cargo-test
runner-args: "inference_retraction"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 2.2s
---

clear the condition. Assert the produced triples are retracted from the graph and the corresponding resolved event is emitted within 2 seconds.