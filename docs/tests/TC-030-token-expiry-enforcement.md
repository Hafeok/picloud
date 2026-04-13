---
id: TC-030
title: token_expiry_enforcement
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-009
phase: 1
runner: picloud-test
runner-args: "token-expiry-enforcement"
---

issue a token, wait for it to expire, present it to an IAM-gated endpoint, assert 401 with `WWW-Authenticate` header.