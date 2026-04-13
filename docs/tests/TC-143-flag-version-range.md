---
id: TC-143
title: flag_version_range
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-044
phase: 1
runner: cargo-test
runner-args: "flag_version_range"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

deploy flag with `version: 2..4`. Assert active for versions 2, 3, 4 and inactive for versions 1 and 5.