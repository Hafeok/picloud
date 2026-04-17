---
id: TC-148
title: otel_does_not_starve_raft
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-045
phase: 1
runner: cargo-test
runner-args: "otel_does_not_starve_raft"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.8s
failure-message: "No matching test function found (0 tests ran)"
---

generate 10,000 OTel spans per second for 60 seconds. During this burst, measure Raft append latency. Assert Raft p99 append latency does not increase by more than 20% compared to baseline.