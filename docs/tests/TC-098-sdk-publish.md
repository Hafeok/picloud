---
id: TC-098
title: sdk_publish
type: scenario
status: passing
validates:
  features:
  - FT-010
  - FT-087
  adrs:
  - ADR-033
phase: 1
runner: picloud-test
runner-args: run --scenario sdk-publish
last-run: 2026-04-15T17:44:47.553113370+00:00
last-run-duration: 0.0s
---

run `picloud sdk publish` against a live cluster configured with a local test registry. Assert packages appear in the test registry within 5 minutes.