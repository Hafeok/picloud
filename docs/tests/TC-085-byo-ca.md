---
id: TC-085
title: byo_ca
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-030
phase: 1
runner: picloud-test
runner-args: "byo-ca"
---

init a cluster with `--ca-cert ./test-ca.pem --ca-key ./test-ca-key.pem`. Verify all issued node certificates chain to the provided CA, not a platform-generated one.