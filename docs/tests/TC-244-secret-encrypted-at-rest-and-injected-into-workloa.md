---
id: TC-244
title: Secret encrypted at rest and injected into workload environment
type: scenario
status: passing
runner: cargo-test
runner-args: "tc244_secret_encrypted_at_rest_and_injected_into_workload_environment"
validates:
  features: [FT-030]
  adrs: []
phase: 2
last-run: 2026-04-14T07:57:37.702932179+00:00
---

## Description

End-to-end scenario verifying that secrets stored via the SecretStore are encrypted at rest using AES-256-GCM and are correctly injected into workload environment variables at schedule time.

### Steps

1. Store secrets via `SecretStore::store_secret` (encrypted with AES-256-GCM)
2. Verify secrets can be retrieved (decrypted) correctly
3. Verify different key material produces isolated encryption (cross-store access fails)
4. Schedule a workload with `EnvValue::Secret` references in its env map
5. Verify the workload is scheduled successfully (env resolution worked)
6. Verify product isolation — same secret name in different products yields different values
7. Verify graceful handling of missing secrets (no crash)