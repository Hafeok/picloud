---
id: TC-141
title: flag_version_evaluation
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-044
phase: 1
runner: cargo-test
runner-args: "flag_version_evaluation"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 1.0s
failure-message: "No matching test function found (0 tests ran)"
---

deploy flag `new-upload-flow` with `version: = 2`. Deploy workload at version 2. Assert flag evaluates as active. Deploy another workload at version 1. Assert flag evaluates as inactive.