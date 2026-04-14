---
id: TC-301
title: Secret management exit — secrets encrypted at rest and injected
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc301_secret_management_exit_secrets_encrypted_at_rest_and_injected"
validates:
  features: [FT-030]
  adrs: []
phase: 2
last-run: 2026-04-14T07:57:37.702932179+00:00
---

## Description

Exit criterion for FT-030. Validates the full secret management lifecycle: create, read, update, delete, workload injection, and product isolation.

### Acceptance Criteria

1. Secrets are encrypted at rest (AES-256-GCM with HKDF-SHA256 key derivation)
2. Full CRUD lifecycle works (store, get, update via re-store, delete)
3. Deleted secrets are no longer retrievable
4. Secrets are injected into workload environment via `EnvValue::Secret` resolution
5. Product isolation enforced — secrets scoped to product namespace