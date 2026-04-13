---
id: TC-037
title: volume_mount_restart
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-012
phase: 1
runner: scripts/run-tc.sh
runner-args: "volume-mount-restart"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

restart the `picloud-server` process on the node hosting the volume. Assert the volume remains mounted and the sentinel file is still readable after restart.