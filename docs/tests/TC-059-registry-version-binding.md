---
id: TC-059
title: registry_version_binding
type: scenario
status: passing
validates:
  features:
  - FT-005
  adrs:
  - ADR-020
phase: 1
runner: scripts/run-tc.sh
runner-args: "registry-version-binding"
last-run: 2026-04-13T19:48:54.098720974+00:00
---

deploy product v1, then upgrade to v2. Assert the cluster graph reflects the new version and the old version's resources are no longer present.