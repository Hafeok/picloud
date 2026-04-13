---
id: TC-098
title: sdk_publish
type: scenario
status: failing
validates:
  features:
  - FT-010
  adrs:
  - ADR-033
phase: 1
runner: picloud-test
runner-args: "sdk-publish"
---

run `picloud sdk publish` against a live cluster configured with a local test registry. Assert packages appear in the test registry within 5 minutes.