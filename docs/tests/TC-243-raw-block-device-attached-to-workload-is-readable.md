---
id: TC-243
title: Raw block device attached to workload is readable and writable
type: scenario
status: passing
runner: cargo-test
runner-args: "tc243_raw_block_device_attached_to_workload_is_readable_and_writable"
validates:
  features: [FT-029]
  adrs: []
phase: 1
last-run: 2026-04-14T07:52:51.355234084+00:00
---

## Description

Allocates a raw block device volume via `StorageBackend::allocate_volume` with
`VolumeType::RawBlock`, then verifies that the resulting device path:

1. Exists on disk as a regular file (Phase 1 simulation of a block device).
2. Has the declared size (1 GiB sparse file).
3. Is writable — arbitrary data written at offset 0 and at offset 4096.
4. Is readable — data read back matches what was written.
5. Correctly decreases available capacity.
6. Is cleaned up on `delete_volume` and capacity is restored.