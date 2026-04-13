---
id: TC-028
title: human_identity_lifecycle
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-009
phase: 1
runner: cargo-test
runner-args: "tc028_human_identity_lifecycle"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

create a human identity, issue a token via CLI device flow, decode the JWT, assert `iss`, `sub`, `aud`, `exp`, `iat` claims are present and correct.