---
id: TC-082
title: no_cross_slice_imports
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-028
phase: 1
---

run `cargo deny` or a custom lint that scans `Cargo.toml` for any `picloud-*` dependency in any slice other than `picloud-domain`. Assert zero violations.