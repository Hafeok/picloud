---
id: TC-304
title: Offsite backup exit — backup uploaded encrypted to S3 endpoint
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc304_offsite_backup_exit_backup_uploaded_encrypted_to_s3_endpoint"
validates:
  features: [FT-034]
  adrs: []
phase: 2
last-run: 2026-04-14T08:16:54.429362383+00:00
---

## Description

Exit criterion: given a volume with snapshot data, after running the offsite backup manager the data is stored encrypted on the S3-compatible target and can be restored to its original form. Confirms the end-to-end contract: plaintext → encrypt → upload → download → decrypt → original plaintext. Also verifies decryption with a wrong key fails.