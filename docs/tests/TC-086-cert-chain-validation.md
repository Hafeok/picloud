---
id: TC-086
title: cert_chain_validation
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-030
phase: 1
runner: picloud-test
runner-args: "cert-chain-validation"
---

extract a node certificate and verify the full chain: leaf → cluster CA (or BYO CA). Assert the chain is valid and the CA fingerprint in the `Issuer` field matches the cluster identity.