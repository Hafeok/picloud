---
id: TC-036
title: raw_block_device
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-012
phase: 1
runner: scripts/run-tc.sh
runner-args: "raw-block-device"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

allocate a raw block device volume. Assert the block device node (e.g. `/dev/xvdb`) is present inside the container. Write a known pattern to the device, read it back, assert byte-identical.