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
last-run: 2026-04-13T21:47:42.689812716+00:00
---

clear the condition. Assert the produced triples are retracted from the graph and the corresponding resolved event is emitted within 2 seconds.