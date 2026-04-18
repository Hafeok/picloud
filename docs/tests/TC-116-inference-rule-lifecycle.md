---
id: TC-116
title: inference_rule_lifecycle
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-038
phase: 1
runner: cargo-test
runner-args: "inference_rule_lifecycle"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 3.2s
---

deploy an `inference-rule` resource. Trigger the condition (inject a matching `MetricRecorded` event). Assert produced triples appear in the graph and the correct assertion event is emitted.