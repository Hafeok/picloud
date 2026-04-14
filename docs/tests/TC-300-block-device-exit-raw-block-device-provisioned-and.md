---
id: TC-300
title: Block device exit — raw block device provisioned and accessible
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc300_block_device_exit_raw_block_device_provisioned_and_accessible"
validates:
  features: [FT-029]
  adrs: []
phase: 1
last-run: 2026-04-14T07:52:51.355234084+00:00
---

## Description

Exit criteria validation for raw block device support (FT-029). Verifies:

1. A raw block volume can be provisioned without error.
2. The device path exists on disk as a file.
3. The file has the declared size (2 GiB).
4. Replication targets are populated for FullReplication durability.
5. The device is accessible for read and write operations.
6. Mounted volumes still work alongside raw block volumes.
7. Capacity tracking accounts for both volume types correctly.