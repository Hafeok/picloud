---
id: TC-109
title: tag_remove
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-036
phase: 1
---

remove a tag. Assert `TagRemoved` event in log and tag triple absent from graph within projection latency budget.