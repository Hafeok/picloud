---
id: TC-059
title: registry_version_binding
type: scenario
status: failing
validates:
  features:
  - FT-005
  adrs:
  - ADR-020
phase: 1
runner: picloud-test
runner-args: "registry-version-binding"
---

deploy product v1, then upgrade to v2. Assert the cluster graph reflects the new version and the old version's resources are no longer present.