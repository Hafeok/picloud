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
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 0.7s
---

remove a tag. Assert `TagRemoved` event in log and tag triple absent from graph within projection latency budget.