---
id: TC-086
title: cert_chain_validation
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-030
phase: 1
runner: cargo-test
runner-args: "tc086_cert_chain_validation"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

extract a node certificate and verify the full chain: leaf → cluster CA (or BYO CA). Assert the chain is valid and the CA fingerprint in the `Issuer` field matches the cluster identity.