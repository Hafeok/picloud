---
id: TC-144
title: flag_in_process_evaluation
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-044
phase: 1
runner: cargo-test
runner-args: "flag_in_process_evaluation"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

after SDK initialisation, measure flag evaluation latency. Assert all evaluations are in-process (zero network round-trips) after initial load.