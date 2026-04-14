---
id: TC-247
title: Offsite backup uploads encrypted incremental snapshot to S3 endpoint
type: scenario
status: passing
runner: cargo-test
runner-args: "tc247_offsite_backup_uploads_encrypted_incremental_snapshot_to_s3_endpoint"
validates:
  features: [FT-034]
  adrs: []
phase: 2
last-run: 2026-04-14T08:16:54.429362383+00:00
---

## Description

Create a snapshot, run an offsite backup with encryption enabled. Verify that encrypted chunks are uploaded to the backup target, stored data is NOT plaintext, a second identical backup is fully deduplicated (zero new uploads), and a partially changed backup only uploads the diff chunks.