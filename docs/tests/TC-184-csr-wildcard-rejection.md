---
id: TC-184
title: csr_wildcard_rejection
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-053
phase: 1
runner: cargo-test
runner-args: "tc184_csr_wildcard_rejection"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

submit a CSR with a wildcard SAN (`*.picloud.local`). Assert the enrollment endpoint returns 400 and no certificate is issued.