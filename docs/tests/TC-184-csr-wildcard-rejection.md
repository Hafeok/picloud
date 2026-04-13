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
---

submit a CSR with a wildcard SAN (`*.picloud.local`). Assert the enrollment endpoint returns 400 and no certificate is issued.