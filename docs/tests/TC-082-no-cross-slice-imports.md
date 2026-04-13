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
runner: cargo-test
runner-args: "tc082_no_cross_slice_imports"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

run `cargo deny` or a custom lint that scans `Cargo.toml` for any `picloud-*` dependency in any slice other than `picloud-domain`. Assert zero violations.