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
runner: cargo-test
runner-args: "tag_remove"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

remove a tag. Assert `TagRemoved` event in log and tag triple absent from graph within projection latency budget.