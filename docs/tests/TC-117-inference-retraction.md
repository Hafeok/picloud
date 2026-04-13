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
runner: picloud-test
runner-args: "inference-retraction"
---

clear the condition. Assert the produced triples are retracted from the graph and the corresponding resolved event is emitted within 2 seconds.