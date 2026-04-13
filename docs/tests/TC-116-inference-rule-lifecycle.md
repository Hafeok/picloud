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
---

deploy an `inference-rule` resource. Trigger the condition (inject a matching `MetricRecorded` event). Assert produced triples appear in the graph and the correct assertion event is emitted.