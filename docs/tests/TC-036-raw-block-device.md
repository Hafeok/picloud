---
id: TC-036
title: raw_block_device
type: scenario
status: unimplemented
validates:
  features:
  - FT-004
  adrs:
  - ADR-012
phase: 1
---

allocate a raw block device volume. Assert the block device node (e.g. `/dev/xvdb`) is present inside the container. Write a known pattern to the device, read it back, assert byte-identical.