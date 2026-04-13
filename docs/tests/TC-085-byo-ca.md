---
id: TC-085
title: byo_ca
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-030
phase: 1
runner: cargo-test
runner-args: "tc085_byo_ca"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

init a cluster with `--ca-cert ./test-ca.pem --ca-key ./test-ca-key.pem`. Verify all issued node certificates chain to the provided CA, not a platform-generated one.