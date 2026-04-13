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
---

assert that only `picloud-server/src/main.rs` references more than one non-domain slice crate. Any other crate referencing multiple slices is a dependency violation.