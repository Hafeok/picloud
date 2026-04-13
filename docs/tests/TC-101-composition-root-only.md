---
id: TC-101
title: composition_root_only
type: scenario
status: passing
validates:
  features: []
  adrs:
  - ADR-034
phase: 1
runner: picloud-test
runner-args: run --scenario composition-root-only
last-run: 2026-04-13T20:35:13.782523150+00:00
---

assert that only `picloud-server/src/main.rs` references more than one non-domain slice crate. Any other crate referencing multiple slices is a dependency violation.