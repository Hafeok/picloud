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
runner: picloud-test
runner-args: "flag-in-process-evaluation"
---

after SDK initialisation, measure flag evaluation latency. Assert all evaluations are in-process (zero network round-trips) after initial load.