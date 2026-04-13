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
---

restart the `picloud-server` process on the node hosting the volume. Assert the volume remains mounted and the sentinel file is still readable after restart.