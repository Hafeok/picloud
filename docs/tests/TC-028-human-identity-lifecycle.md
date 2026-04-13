---
id: TC-028
title: human_identity_lifecycle
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-009
phase: 1
runner: picloud-test
runner-args: "human-identity-lifecycle"
---

create a human identity, issue a token via CLI device flow, decode the JWT, assert `iss`, `sub`, `aud`, `exp`, `iat` claims are present and correct.