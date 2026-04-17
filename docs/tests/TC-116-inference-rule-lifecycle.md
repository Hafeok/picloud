---
id: TC-116
title: inference_rule_lifecycle
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-038
phase: 1
runner: cargo-test
runner-args: "inference_rule_lifecycle"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.9s
failure-message: "No matching test function found (0 tests ran)"
---

deploy an `inference-rule` resource. Trigger the condition (inject a matching `MetricRecorded` event). Assert produced triples appear in the graph and the correct assertion event is emitted.