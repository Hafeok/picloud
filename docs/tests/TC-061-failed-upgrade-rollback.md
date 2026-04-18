---
id: TC-061
title: failed_upgrade_rollback
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-021
phase: 1
runner: scripts/run-tc.sh
runner-args: "failed-upgrade-rollback"
last-run: 2026-04-18T15:41:57.068082457+00:00
last-run-duration: 0.0s
---

deploy product v1, then apply v2 where one required resource is deliberately misconfigured. Assert v2 deployment fails, v1 resources remain `picloud:Running`, and no v2 resources are left in the graph.