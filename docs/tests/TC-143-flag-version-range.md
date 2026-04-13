---
id: TC-143
title: flag_version_range
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-044
phase: 1
runner: picloud-test
runner-args: "flag-version-range"
---

deploy flag with `version: 2..4`. Assert active for versions 2, 3, 4 and inactive for versions 1 and 5.