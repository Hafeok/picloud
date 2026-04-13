---
id: TC-141
title: flag_version_evaluation
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-044
phase: 1
---

deploy flag `new-upload-flow` with `version: = 2`. Deploy workload at version 2. Assert flag evaluates as active. Deploy another workload at version 1. Assert flag evaluates as inactive.